//! `cargo tops lock --write`: rewrite the `[tools]` values the tools print
//! now, and nothing else.
//!
//! Only a plain `key = "value"` line under a `[tools]` header is rewritten, so
//! comments, order and spacing are kept byte for byte. Any other form of the
//! same key (quoted or dotted key, inline table, literal or multi-line
//! string) is refused, and the lock is left as it was. The edited text is read
//! back before it is written.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, Metadata, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{bail, Context, Result};

use super::probe::probe_in;
use super::{found_versions, pin_matches, read_lock, row, unmatched_keys, Line};

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// `cargo tops lock --write` on the lock at `path`, read as `text`.
pub(crate) fn write_command(
    dir: &Path,
    path: &Path,
    text: &str,
    pins: &BTreeMap<String, String>,
) -> Result<ExitCode> {
    let target = std::fs::canonicalize(path).with_context(|| format!("find {}", path.display()))?;
    ensure_writable(path, &target)?;
    let found = found_versions(pins, &mut probe_in(dir));
    let changes = changes(pins, &found);
    let written = write_values(text, &changes)?;
    check_read_back(&written, &changes)?;
    if written != text {
        replace_file(path, &written).with_context(|| format!("write {}", path.display()))?;
    }
    for line in write_lines(pins, &found) {
        println!("{line}");
    }
    // The keys that name no tool are not written, and a misspelt one is said.
    for line in unmatched_keys(pins).iter().map(Line::text) {
        println!("{line}");
    }
    Ok(ExitCode::SUCCESS)
}

/// Refuses read-only targets (`mode & 0o222 == 0`) and hard-linked targets
/// (`nlink > 1`) before creating any temporary file or renaming over the lock.
fn ensure_writable(path: &Path, target: &Path) -> Result<Metadata> {
    let meta = std::fs::metadata(target).with_context(|| format!("read {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let mode = meta.permissions().mode();
        if mode & 0o222 == 0 {
            bail!(
                "{} is read-only (mode {:o}); refusing to overwrite",
                path.display(),
                mode & 0o777
            );
        }
        let nlink = meta.nlink();
        if nlink > 1 {
            bail!(
                "{} has {nlink} hard links; refusing to overwrite",
                path.display()
            );
        }
    }
    #[cfg(not(unix))]
    if meta.permissions().readonly() {
        bail!("{} is read-only; refusing to overwrite", path.display());
    }
    Ok(meta)
}

/// Writes `contents` to `path` through a file of its own in the same directory,
/// which is renamed over it, so a crash leaves the old lock or the new one. A
/// link is followed, so it stays a link, and the permissions are kept.
pub(crate) fn replace_file(path: &Path, contents: &str) -> Result<()> {
    replace_file_with_counter(path, contents, &TEMP_COUNTER)
}

fn replace_file_with_counter(path: &Path, contents: &str, counter: &AtomicUsize) -> Result<()> {
    let target = std::fs::canonicalize(path).context("find the file")?;
    let meta = ensure_writable(path, &target)?;
    let (temp, mut file) = create_unique_temp(&target, counter)?;
    let written = write_temp(&target, &temp, &mut file, &meta, contents);
    if written.is_err() {
        std::fs::remove_file(&temp).ok();
    }
    written
}

fn create_unique_temp(target: &Path, counter: &AtomicUsize) -> Result<(PathBuf, File)> {
    let file_name = target.file_name().context("a file name")?;
    for _ in 0..64 {
        let seq = counter.fetch_add(1, Ordering::Relaxed);
        let mut name = file_name.to_os_string();
        name.push(format!(".tmp-{}-{seq}", std::process::id()));
        let temp = target.with_file_name(name);
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {}
            Err(err) => return Err(anyhow::Error::new(err).context("create the temporary file")),
        }
    }
    bail!("could not create unique temporary file")
}

fn write_temp(
    target: &Path,
    temp: &Path,
    file: &mut File,
    meta: &Metadata,
    contents: &str,
) -> Result<()> {
    file.write_all(contents.as_bytes()).context("write it")?;
    file.sync_all().context("sync it")?;
    std::fs::set_permissions(temp, meta.permissions()).context("keep the permissions")?;
    std::fs::rename(temp, target).context("rename it into place")
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
    let pins = read_lock(written)
        .context("the edited lock does not read back")?
        .unwrap_or_default();
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
    use crate::lock::probe::Scratch;

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

    #[test]
    fn a_file_is_replaced_through_a_sibling_and_a_link_stays_a_link() {
        let scratch = Scratch::new("replace");
        let real = scratch.0.join("real.lock");
        let link = scratch.0.join("link.lock");
        std::fs::write(&real, "old\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::{symlink, PermissionsExt};
            std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).expect("chmod");
            symlink(&real, &link).expect("link");
            replace_file(&link, "new\n").expect("replace through the link");
            assert!(std::fs::symlink_metadata(&link)
                .expect("link")
                .file_type()
                .is_symlink());
            assert_eq!(std::fs::read_to_string(&real).expect("read"), "new\n");
            let mode = std::fs::metadata(&real).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o640);
        }
        let names: Vec<String> = std::fs::read_dir(&scratch.0)
            .expect("dir")
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .collect();
        assert!(
            names.iter().all(|name| !name.contains(".tmp-")),
            "{names:?}"
        );
        assert!(replace_file(&scratch.0.join("missing.lock"), "x").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn replace_file_skips_preexisting_temp_files_and_symlinks_with_create_new() {
        use std::os::unix::fs::symlink;
        let scratch = Scratch::new("create-new");
        let target = scratch.0.join("rust-tops.lock");
        std::fs::write(&target, "old\n").expect("write");
        let pid = std::process::id();
        let collide_file = scratch.0.join(format!("rust-tops.lock.tmp-{pid}-0"));
        let collide_link = scratch.0.join(format!("rust-tops.lock.tmp-{pid}-1"));
        let sentinel = scratch.0.join("sentinel");
        std::fs::write(&collide_file, "keep-temp\n").expect("write collide file");
        std::fs::write(&sentinel, "untouched\n").expect("write sentinel");
        symlink(&sentinel, &collide_link).expect("symlink collide");

        let counter = AtomicUsize::new(0);
        replace_file_with_counter(&target, "new\n", &counter).expect("replace");
        assert_eq!(std::fs::read_to_string(&target).expect("read"), "new\n");
        assert_eq!(
            std::fs::read_to_string(&collide_file).expect("read collide"),
            "keep-temp\n"
        );
        assert_eq!(
            std::fs::read_to_string(&sentinel).expect("read sentinel"),
            "untouched\n"
        );
        assert!(counter.load(Ordering::Relaxed) >= 3);

        // Non-AlreadyExists errors (such as PermissionDenied on a read-only parent
        // directory) must fail immediately rather than retrying.
        use std::os::unix::fs::PermissionsExt;
        let ro_dir = scratch.0.join("rodir");
        std::fs::create_dir(&ro_dir).expect("mkdir rodir");
        let ro_target = ro_dir.join("rust-tops.lock");
        std::fs::write(&ro_target, "old\n").expect("write ro_target");
        std::fs::set_permissions(&ro_dir, std::fs::Permissions::from_mode(0o555)).expect("chmod");
        let ro_counter = AtomicUsize::new(0);
        let err = replace_file_with_counter(&ro_target, "new\n", &ro_counter);
        std::fs::set_permissions(&ro_dir, std::fs::Permissions::from_mode(0o755)).ok();
        assert!(err.is_err(), "expected error in read-only parent dir");
        assert_eq!(ro_counter.load(Ordering::Relaxed), 1);
    }

    #[cfg(unix)]
    #[test]
    fn replace_file_refuses_readonly_and_hardlinked_targets() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("refuse");
        let ro = scratch.0.join("ro.lock");
        std::fs::write(&ro, "old\n").expect("write ro");
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o444)).expect("chmod");
        let err = replace_file(&ro, "new\n").expect_err("refuse readonly");
        assert!(err.to_string().contains(&ro.display().to_string()), "{err}");
        assert!(err.to_string().contains("read-only"), "{err}");
        assert_eq!(std::fs::read_to_string(&ro).expect("read ro"), "old\n");
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o644)).expect("restore");

        let hl_a = scratch.0.join("a.lock");
        let hl_b = scratch.0.join("b.lock");
        std::fs::write(&hl_a, "old\n").expect("write hl");
        std::fs::hard_link(&hl_a, &hl_b).expect("hard_link");
        let err = replace_file(&hl_a, "new\n").expect_err("refuse hard link");
        assert!(
            err.to_string().contains(&hl_a.display().to_string()),
            "{err}"
        );
        assert!(err.to_string().contains("hard link"), "{err}");
        assert_eq!(std::fs::read_to_string(&hl_a).expect("read a"), "old\n");
        assert_eq!(std::fs::read_to_string(&hl_b).expect("read b"), "old\n");
    }
}
