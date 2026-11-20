//! `cargo tops lock --write`: rewrite the `[tools]` values the tools print
//! now, and nothing else.
//!
//! Only a plain `key = "value"` line under a `[tools]` header is rewritten, so
//! comments, order and spacing are kept byte for byte. Any other form of the
//! same key (quoted or dotted key, inline table, literal or multi-line
//! string) is refused, and the lock is left as it was. The edited text is read
//! back before it is written.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};

use super::probe::probe_in;
use super::{found_versions, pin_matches, read_pins, row};

/// `cargo tops lock --write` on the lock at `path`, read as `text`.
pub(crate) fn write_command(
    dir: &Path,
    path: &Path,
    text: &str,
    pins: &BTreeMap<String, String>,
) -> Result<ExitCode> {
    let found = found_versions(pins, &mut probe_in(dir));
    let changes = changes(pins, &found);
    let written = write_values(text, &changes)?;
    check_read_back(&written, &changes)?;
    if written != text {
        std::fs::write(path, written).with_context(|| format!("write {}", path.display()))?;
    }
    for line in write_lines(pins, &found) {
        println!("{line}");
    }
    Ok(ExitCode::SUCCESS)
}

/// The values to write: each found version that its pin does not already
/// match. A partial pin such as `0.12` that matches `0.12.3` is left alone.
fn changes(
    pins: &BTreeMap<String, String>,
    found: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    found
        .iter()
        .filter(|(key, now)| pins.get(*key).is_some_and(|pin| !pin_matches(pin, now)))
        .map(|(key, now)| (key.clone(), now.clone()))
        .collect()
}

/// One line for each pinned tool `--write` is about, true of the edit: the old
/// and new value for a changed one, and that a tool with no version to write
/// keeps its value.
pub(crate) fn write_lines(
    pins: &BTreeMap<String, String>,
    found: &BTreeMap<String, String>,
) -> Vec<String> {
    pins.iter()
        .filter(|(key, _)| row(key).is_some())
        .map(|(key, pin)| match found.get(key) {
            Some(now) if !pin_matches(pin, now) => format!("wrote    {key}: {pin} -> {now}"),
            Some(_) => format!("kept     {key} {pin}"),
            None => format!("kept     {key} {pin}: no version to write"),
        })
        .collect()
}

/// `text` with the `[tools]` value of each key in `changes` replaced, or an
/// error naming the keys that are not on a plain `key = "value"` line.
pub(crate) fn write_values(text: &str, changes: &BTreeMap<String, String>) -> Result<String> {
    let mut pending: BTreeSet<&str> = changes.keys().map(String::as_str).collect();
    let mut in_tools = false;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_tools = is_tools_header(trimmed);
        }
        out.push_str(&if in_tools {
            rewritten(line, changes, &mut pending)
        } else {
            line.to_string()
        });
    }
    if pending.is_empty() {
        return Ok(out);
    }
    let keys: Vec<&str> = pending.into_iter().collect();
    bail!(
        "not written, the lock is unchanged: {} not on a plain `key = \"value\"` line under [tools]",
        keys.join(", ")
    )
}

/// `line` of the `[tools]` table, with its value replaced when it is the plain
/// line of a key in `changes`; the key is then no longer pending.
fn rewritten(
    line: &str,
    changes: &BTreeMap<String, String>,
    pending: &mut BTreeSet<&str>,
) -> String {
    for (key, new) in changes {
        if let Some(replaced) = replace_plain(line, key, new) {
            pending.remove(key.as_str());
            return replaced;
        }
    }
    line.to_string()
}

/// Whether a header line (already trimmed at the front) is `[tools]`, with an
/// optional comment after it.
fn is_tools_header(trimmed: &str) -> bool {
    trimmed.strip_prefix("[tools]").is_some_and(|rest| {
        let rest = rest.trim();
        rest.is_empty() || rest.starts_with('#')
    })
}

/// The line with `key`'s value replaced by `new`, when the line is exactly
/// `key = "basic string"` with an optional comment, in any spacing.
fn replace_plain(line: &str, key: &str, new: &str) -> Option<String> {
    let (body, eol) = line.split_at(line.trim_end_matches(['\r', '\n']).len());
    let (indent, rest) = body.split_at(body.len() - body.trim_start().len());
    let after_key = rest.strip_prefix(key)?;
    let quote = after_key.find('"')?;
    let (between, value_and_tail) = after_key.split_at(quote);
    if between.trim() != "=" {
        return None;
    }
    let value_and_tail = value_and_tail.strip_prefix('"')?;
    let (value, tail) = value_and_tail.split_at(value_and_tail.find('"')?);
    let tail = tail.strip_prefix('"')?;
    let plain_tail = tail.trim().is_empty() || tail.trim_start().starts_with('#');
    (plain_tail && !value.contains('\\'))
        .then(|| format!("{indent}{key}{between}\"{new}\"{tail}{eol}"))
}

/// The edited lock must read back with every changed pin at its new value.
fn check_read_back(written: &str, changes: &BTreeMap<String, String>) -> Result<()> {
    let pins = read_pins(written).context("the edited lock does not read back")?;
    for (key, new) in changes {
        if pins.get(key) != Some(new) {
            bail!("the edited lock does not read back with {key} = \"{new}\"; nothing was written");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn a_plain_line_changes_only_its_value() {
        let text = "# keep me\n[tools]\nrustc = \"1.99.1\"  # the toolchain\n# nextest note\ncargo-nextest=\"0.9.145\"#c\r\ncargo-crap = \"0.9.0\"\nproptest = \"1\"\n\n[notes]\nrustc = \"untouched\"\n";
        let changes = map(&[("cargo-nextest", "0.9.150"), ("rustc", "1.99.2")]);
        assert_eq!(
            write_values(text, &changes).expect("written"),
            "# keep me\n[tools]\nrustc = \"1.99.2\"  # the toolchain\n# nextest note\ncargo-nextest=\"0.9.150\"#c\r\ncargo-crap = \"0.9.0\"\nproptest = \"1\"\n\n[notes]\nrustc = \"untouched\"\n"
        );
    }

    #[test]
    fn a_header_with_a_comment_is_the_tools_table_and_a_sub_table_is_not() {
        let text = "[tools] # pins\nrustc = \"1.99.1\"\n[tools.extra]\ncargo-crap = \"0.9.0\"\n";
        let out = write_values(text, &map(&[("rustc", "1.99.2")])).expect("written");
        assert_eq!(
            out,
            "[tools] # pins\nrustc = \"1.99.2\"\n[tools.extra]\ncargo-crap = \"0.9.0\"\n"
        );
        assert!(write_values(text, &map(&[("cargo-crap", "0.9.1")])).is_err());
    }

    #[test]
    fn every_other_form_of_the_key_is_refused_by_name() {
        for line in [
            "\"cargo-crap\" = \"0.9.0\"",
            "cargo-crap = '0.9.0'",
            "cargo-crap = \"\"\"0.9.0\"\"\"",
            "cargo-crap = \"0.9.0\" extra",
            "cargo-crap = { version = \"0.9.0\" }",
            "cargo-crap = \"0\\u002e9.0\"",
            "cargo-crap-x = \"0.9.0\"",
        ] {
            let text = format!("[tools]\n{line}\n");
            let err = write_values(&text, &map(&[("cargo-crap", "0.9.1")])).expect_err(line);
            assert!(err.to_string().contains("cargo-crap"), "{line}: {err}");
            assert!(err.to_string().contains("unchanged"), "{line}: {err}");
        }
    }

    #[test]
    fn a_key_outside_the_tools_table_is_not_rewritten() {
        let text = "[notes]\ncargo-crap = \"0.9.0\"\n";
        assert!(write_values(text, &map(&[("cargo-crap", "0.9.1")])).is_err());
        assert_eq!(
            write_values(text, &BTreeMap::new()).expect("nothing to do"),
            text
        );
    }

    #[test]
    fn only_a_changed_value_is_a_wrote_line() {
        let pins = map(&[
            ("cargo-crap", "0.9.0"),
            ("cargo-deny", "0.18.9"),
            ("cargo-fuzz", "0.12"),
            ("cargo-nextest", "0.9.145"),
            ("proptest", "1"),
        ]);
        let found = map(&[
            ("cargo-deny", "0.18.9"),
            ("cargo-fuzz", "0.12.3"),
            ("cargo-nextest", "0.9.150"),
        ]);
        assert_eq!(
            write_lines(&pins, &found),
            [
                "kept     cargo-crap 0.9.0: no version to write",
                "kept     cargo-deny 0.18.9",
                "kept     cargo-fuzz 0.12",
                "wrote    cargo-nextest: 0.9.145 -> 0.9.150",
            ]
        );
    }

    #[test]
    fn changes_leave_out_a_pin_that_already_matches() {
        let pins = map(&[("cargo-fuzz", "0.12"), ("cargo-crap", "0.9.0")]);
        let found = map(&[("cargo-fuzz", "0.12.3"), ("cargo-crap", "0.9.5")]);
        assert_eq!(changes(&pins, &found), map(&[("cargo-crap", "0.9.5")]));
    }

    #[test]
    fn the_edit_must_read_back_with_every_new_value() {
        let changes = map(&[("cargo-crap", "0.9.5")]);
        assert!(check_read_back("[tools]\ncargo-crap = \"0.9.5\"\n", &changes).is_ok());
        assert!(check_read_back("[tools]\ncargo-crap = \"0.9.0\"\n", &changes).is_err());
        assert!(check_read_back("[tools\n", &changes).is_err());
    }
}
