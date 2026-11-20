//! The tool lock (§ tool pins): one table of the eleven pinned tools, how each
//! is asked for its version, and how that line is read.
//!
//! A pin is the `[tools]` value in `rust-tops.lock`. The gate and
//! `cargo tops lock --verify` ask the tool that will run, never a file, so a
//! different binary earlier on `PATH` is a mismatch. `clippy` and `rustfmt`
//! ship with the toolchain, so both are judged by `rustc --version`. Library
//! and name rows (`proptest`, `criterion`, `callgrind-runner`) are not tools
//! on `PATH`; they are skipped.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use anyhow::{Context, Result};

/// One pinned tool: its lock key, the argv that prints its version, and the
/// text the first line starts with before the version.
pub(crate) struct Row {
    pub(crate) key: &'static str,
    pub(crate) argv: &'static [&'static str],
    pub(crate) prefix: &'static str,
}

const RUSTC: &[&str] = &["rustc", "--version"];

/// The table. Order is the order `--verify` prints.
pub(crate) const ROWS: [Row; 11] = [
    Row {
        key: "rustc",
        argv: RUSTC,
        prefix: "rustc ",
    },
    Row {
        key: "clippy",
        argv: RUSTC,
        prefix: "rustc ",
    },
    Row {
        key: "rustfmt",
        argv: RUSTC,
        prefix: "rustc ",
    },
    Row {
        key: "cargo-nextest",
        argv: &["cargo", "nextest", "--version"],
        prefix: "cargo-nextest ",
    },
    Row {
        key: "cargo-llvm-cov",
        argv: &["cargo", "llvm-cov", "--version"],
        prefix: "cargo-llvm-cov ",
    },
    Row {
        key: "cargo-mutants",
        argv: &["cargo", "mutants", "--version"],
        prefix: "cargo-mutants ",
    },
    Row {
        key: "cargo-crap",
        argv: &["cargo", "crap", "--version"],
        prefix: "cargo-crap ",
    },
    Row {
        key: "cargo-deny",
        argv: &["cargo", "deny", "--version"],
        prefix: "cargo-deny ",
    },
    Row {
        key: "cargo-fuzz",
        argv: &["cargo", "fuzz", "--version"],
        prefix: "cargo-fuzz ",
    },
    Row {
        key: "kani-verifier",
        argv: &["cargo", "kani", "--version"],
        prefix: "cargo-kani ",
    },
    Row {
        key: "nushell",
        argv: &["nu", "--version"],
        prefix: "",
    },
];

/// The lock keys a layer's command needs. A layer not listed needs none.
const LAYER_TOOLS: &[(&str, &[&str])] = &[
    ("static-lint", &["rustc", "clippy", "rustfmt", "cargo-deny"]),
    ("unit", &["cargo-nextest"]),
    ("integration", &["cargo-nextest"]),
    ("property", &["cargo-nextest"]),
    ("acceptance", &["cargo-nextest"]),
    ("doc", &["rustc"]),
    ("coverage", &["cargo-llvm-cov", "cargo-nextest"]),
    ("crap", &["cargo-crap"]),
    ("security-advisory", &["cargo-deny"]),
    ("mutation", &["cargo-mutants"]),
    ("fuzz", &["cargo-fuzz"]),
    ("formal-kani", &["kani-verifier"]),
];

/// The tools the given layer ids need, each once, in table order.
pub(crate) fn tools_for<'a>(layers: impl IntoIterator<Item = &'a str>) -> Vec<&'static str> {
    let wanted: BTreeSet<&str> = layers
        .into_iter()
        .filter_map(|id| LAYER_TOOLS.iter().find(|(layer, _)| *layer == id))
        .flat_map(|(_, tools)| tools.iter().copied())
        .collect();
    ROWS.iter()
        .map(|row| row.key)
        .filter(|key| wanted.contains(key))
        .collect()
}

/// The version on the first line of a tool's output, read by the row's rule:
/// the first word after the prefix, cut at the first character that is not a
/// digit or a dot. `None` when the line does not have that shape.
pub(crate) fn parse_version(row: &Row, output: &str) -> Option<String> {
    let line = output.lines().next()?.trim();
    let word = line.strip_prefix(row.prefix)?.split_whitespace().next()?;
    let version: String = word
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let version = version.trim_end_matches('.');
    let well_formed = !version.is_empty()
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    well_formed.then(|| version.to_string())
}

/// Whether `found` satisfies `pin`. Whole dot-separated parts only: `0.12`
/// matches `0.12.3` and not `0.120.0`; `0.12.3` does not match `0.12`.
pub(crate) fn pin_matches(pin: &str, found: &str) -> bool {
    let pin_parts: Vec<&str> = pin.split('.').collect();
    let found_parts: Vec<&str> = found.split('.').collect();
    found_parts.starts_with(&pin_parts)
}

/// What asking one tool came to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Ok {
        found: String,
    },
    /// The tool ran and printed another version, or a line with no version.
    Mismatch {
        found: String,
    },
    /// The tool could not be run, or exited non-zero.
    Missing,
    /// Nothing to ask: a library or name row, or a tool with no pin.
    Skipped {
        why: &'static str,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) key: String,
    pub(crate) pin: String,
    pub(crate) verdict: Verdict,
}

impl Line {
    pub(crate) fn passes(&self) -> bool {
        matches!(self.verdict, Verdict::Ok { .. } | Verdict::Skipped { .. })
    }

    /// The line `--verify` prints, and the detail of a `tool-mismatch`.
    pub(crate) fn text(&self) -> String {
        let Line { key, pin, verdict } = self;
        match verdict {
            Verdict::Ok { found } => format!("ok       {key} {found} (pin {pin})"),
            Verdict::Mismatch { found } => {
                format!("mismatch {key}: pin {pin}, found {found}")
            }
            Verdict::Missing => format!("missing  {key}: pin {pin}, not found"),
            Verdict::Skipped { why } => format!("skipped  {key}: {why}"),
        }
    }
}

/// Reads `[tools]` from a lock text. Values that are not strings are dropped.
pub(crate) fn read_pins(text: &str) -> Result<BTreeMap<String, String>> {
    let lock: toml::Table = toml::from_str(text).context("parse rust-tops.lock")?;
    Ok(lock
        .get("tools")
        .and_then(toml::Value::as_table)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default())
}

/// What `argv` printed on stdout, or `None` when it could not run or failed.
pub(crate) fn system_probe(argv: &[&str]) -> Option<String> {
    let (program, args) = argv.split_first()?;
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Asks one pinned tool. `probe` runs an argv and returns its stdout.
pub(crate) fn check_row(
    row: &Row,
    pin: &str,
    probe: &mut dyn FnMut(&[&str]) -> Option<String>,
) -> Line {
    let verdict = match probe(row.argv) {
        None => Verdict::Missing,
        Some(out) => match parse_version(row, &out) {
            Some(found) if pin_matches(pin, &found) => Verdict::Ok { found },
            Some(found) => Verdict::Mismatch { found },
            None => Verdict::Mismatch {
                found: out.lines().next().unwrap_or_default().trim().to_string(),
            },
        },
    };
    Line {
        key: row.key.to_string(),
        pin: pin.to_string(),
        verdict,
    }
}

/// Checks the pins. With `only`, the tools in it (a tool with no pin is
/// skipped); without, every pin in the lock. A lock key with no row is skipped.
pub(crate) fn verify(
    pins: &BTreeMap<String, String>,
    only: Option<&[&str]>,
    probe: &mut dyn FnMut(&[&str]) -> Option<String>,
) -> Vec<Line> {
    let mut lines = Vec::new();
    for row in &ROWS {
        if only.is_some_and(|only| !only.contains(&row.key)) {
            continue;
        }
        match pins.get(row.key) {
            Some(pin) => lines.push(check_row(row, pin, probe)),
            None if only.is_some() => lines.push(Line {
                key: row.key.to_string(),
                pin: String::new(),
                verdict: Verdict::Skipped {
                    why: "no pin in rust-tops.lock",
                },
            }),
            None => {}
        }
    }
    if only.is_none() {
        for (key, pin) in pins {
            if !ROWS.iter().any(|row| row.key == key) {
                lines.push(Line {
                    key: key.clone(),
                    pin: pin.clone(),
                    verdict: Verdict::Skipped {
                        why: "a library or name, not a tool on PATH",
                    },
                });
            }
        }
    }
    lines
}

/// The version each pinned tool prints now, for `--write`. A tool that is
/// missing or prints no version is left out.
pub(crate) fn found_versions(
    pins: &BTreeMap<String, String>,
    probe: &mut dyn FnMut(&[&str]) -> Option<String>,
) -> BTreeMap<String, String> {
    ROWS.iter()
        .filter(|row| pins.contains_key(row.key))
        .filter_map(|row| {
            let found = parse_version(row, &probe(row.argv)?)?;
            Some((row.key.to_string(), found))
        })
        .collect()
}

/// `text` with the `[tools]` values in `found` replaced. Only the quoted value
/// changes; comments, order, spacing and other tables are kept byte for byte.
pub(crate) fn write_values(text: &str, found: &BTreeMap<String, String>) -> String {
    let mut in_tools = false;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_tools = trimmed.trim_end().starts_with("[tools]");
        }
        out.push_str(&if in_tools {
            replace_value(line, found)
        } else {
            line.to_string()
        });
    }
    out
}

fn replace_value(line: &str, found: &BTreeMap<String, String>) -> String {
    let Some((key, rest)) = line.split_once('=') else {
        return line.to_string();
    };
    let Some(new) = found.get(key.trim()) else {
        return line.to_string();
    };
    let Some(open) = rest.find('"') else {
        return line.to_string();
    };
    let Some(len) = rest[open + 1..].find('"') else {
        return line.to_string();
    };
    let close = open + 1 + len;
    format!("{key}={}{new}{}", &rest[..=open], &rest[close..])
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/versions");

    fn row(key: &str) -> &'static Row {
        ROWS.iter().find(|row| row.key == key).expect("row")
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{FIXTURES}/{name}.txt")).expect("fixture")
    }

    #[test]
    fn the_table_has_the_eleven_tools_once_each() {
        let keys: BTreeSet<&str> = ROWS.iter().map(|row| row.key).collect();
        assert_eq!(keys.len(), 11);
        assert!([
            "rustc",
            "clippy",
            "rustfmt",
            "cargo-nextest",
            "cargo-llvm-cov",
            "cargo-mutants",
            "cargo-crap",
            "cargo-deny",
            "cargo-fuzz",
            "kani-verifier",
            "nushell"
        ]
        .iter()
        .all(|key| keys.contains(key)));
    }

    #[test]
    fn each_row_reads_the_line_a_real_tool_printed_on_tower() {
        for (key, file, want) in [
            ("rustc", "rustc", "1.99.1"),
            ("cargo-nextest", "cargo-nextest", "0.9.145"),
            ("cargo-llvm-cov", "cargo-llvm-cov", "0.9.1"),
            ("cargo-mutants", "cargo-mutants", "27.1.0"),
            ("cargo-crap", "cargo-crap", "0.9.0"),
            ("cargo-deny", "cargo-deny", "0.18.9"),
            ("cargo-fuzz", "cargo-fuzz", "0.12.0"),
            ("nushell", "nushell", "0.108.0"),
        ] {
            assert_eq!(
                parse_version(row(key), &fixture(file)).as_deref(),
                Some(want),
                "{key}"
            );
        }
    }

    #[test]
    fn clippy_and_rustfmt_are_judged_by_the_toolchain_line() {
        let line = fixture("rustc");
        for key in ["clippy", "rustfmt"] {
            assert_eq!(row(key).argv, ["rustc", "--version"]);
            assert_eq!(
                parse_version(row(key), &line).as_deref(),
                Some("1.99.1"),
                "{key}"
            );
        }
    }

    #[test]
    fn kani_reads_the_cargo_kani_line() {
        // No real line is captured: Kani runs a first-time setup that installs
        // a toolchain on any call. The shape is the one the row states.
        assert_eq!(
            parse_version(row("kani-verifier"), "cargo-kani 0.68.0\n").as_deref(),
            Some("0.68.0")
        );
    }

    #[test]
    fn a_line_without_the_prefix_or_a_version_reads_as_none() {
        for out in [
            "",
            "error: no such command: `nextest`",
            "cargo-nextest\n",
            "cargo-nextest next\n",
            "cargo-crap 0.9.0\n",
        ] {
            assert_eq!(parse_version(row("cargo-nextest"), out), None, "{out:?}");
        }
    }

    #[test]
    fn only_the_first_line_is_read() {
        let out = "cargo-nextest 0.9.145 (x)\nrelease: 0.9.99\n";
        assert_eq!(
            parse_version(row("cargo-nextest"), out).as_deref(),
            Some("0.9.145")
        );
    }

    #[test]
    fn a_partial_pin_matches_whole_parts_only() {
        assert!(pin_matches("0.12", "0.12.0"));
        assert!(pin_matches("0.12", "0.12.3"));
        assert!(!pin_matches("0.12", "0.120.0"));
        assert!(!pin_matches("0.12", "0.1.2"));
        assert!(pin_matches("0.9.145", "0.9.145"));
        assert!(!pin_matches("0.9.145", "0.9.14"));
        assert!(!pin_matches("0.12.3", "0.12"));
        assert!(!pin_matches("1", "10.0.0"));
        assert!(pin_matches("1", "1.4.2"));
    }

    fn probe_for<'a>(
        table: &'a [(&'a str, Option<&'a str>)],
    ) -> impl FnMut(&[&str]) -> Option<String> + 'a {
        move |argv| {
            table
                .iter()
                .find(|(tool, _)| argv.contains(tool))
                .and_then(|(_, out)| out.map(str::to_string))
        }
    }

    #[test]
    fn verify_names_tool_pin_and_found_for_a_mismatch() {
        let pins = BTreeMap::from([("cargo-nextest".to_string(), "0.9.145".to_string())]);
        let mut probe = probe_for(&[("nextest", Some("cargo-nextest 0.9.144\n"))]);
        let lines = verify(&pins, None, &mut probe);
        let [line] = lines.as_slice() else {
            panic!("one line: {lines:?}")
        };
        assert!(!line.passes());
        assert_eq!(
            line.text(),
            "mismatch cargo-nextest: pin 0.9.145, found 0.9.144"
        );
    }

    #[test]
    fn a_tool_that_cannot_run_is_missing_and_a_garbled_line_is_a_mismatch() {
        let pins = BTreeMap::from([
            ("cargo-crap".to_string(), "0.9.0".to_string()),
            ("cargo-deny".to_string(), "0.18.9".to_string()),
        ]);
        let mut probe = probe_for(&[("crap", None), ("deny", Some("what\n"))]);
        let lines = verify(&pins, None, &mut probe);
        let texts: Vec<String> = lines.iter().map(Line::text).collect();
        assert_eq!(
            texts,
            [
                "missing  cargo-crap: pin 0.9.0, not found",
                "mismatch cargo-deny: pin 0.18.9, found what",
            ]
        );
    }

    #[test]
    fn library_and_name_rows_are_skipped_and_never_asked() {
        let pins = BTreeMap::from([
            ("proptest".to_string(), "1".to_string()),
            (
                "callgrind-runner".to_string(),
                "gungraun-runner".to_string(),
            ),
        ]);
        let mut probe = |_: &[&str]| -> Option<String> { panic!("asked a library row") };
        let lines = verify(&pins, None, &mut probe);
        assert_eq!(lines.len(), 2);
        assert!(lines
            .iter()
            .all(|line| line.passes() && matches!(line.verdict, Verdict::Skipped { .. })));
    }

    #[test]
    fn only_the_tools_a_layer_needs_are_asked_and_an_unpinned_one_is_skipped() {
        let pins = BTreeMap::from([("cargo-mutants".to_string(), "27.1.0".to_string())]);
        let needed = tools_for(["unit", "mutation"]);
        assert_eq!(needed, ["cargo-nextest", "cargo-mutants"]);
        let mut probe = probe_for(&[("mutants", Some("cargo-mutants 27.1.0\n"))]);
        let lines = verify(&pins, Some(&needed), &mut probe);
        let texts: Vec<String> = lines.iter().map(Line::text).collect();
        assert_eq!(
            texts,
            [
                "skipped  cargo-nextest: no pin in rust-tops.lock",
                "ok       cargo-mutants 27.1.0 (pin 27.1.0)",
            ]
        );
    }

    #[test]
    fn write_changes_only_the_values_it_found() {
        let text = "# keep me\n[tools]\nrustc = \"1.99.1\"  # trailing\n# cargo-nextest note\ncargo-nextest = \"0.9.145\"\nproptest = \"1\"\n\n[notes]\nrustc = \"untouched\"\n";
        let found = BTreeMap::from([
            ("cargo-nextest".to_string(), "0.9.150".to_string()),
            ("rustc".to_string(), "1.99.2".to_string()),
        ]);
        assert_eq!(
            write_values(text, &found),
            "# keep me\n[tools]\nrustc = \"1.99.2\"  # trailing\n# cargo-nextest note\ncargo-nextest = \"0.9.150\"\nproptest = \"1\"\n\n[notes]\nrustc = \"untouched\"\n"
        );
    }

    #[test]
    fn read_pins_reads_tools_only_and_rejects_bad_toml() {
        let pins =
            read_pins("[tools]\nrustc = \"1.99.1\"\nn = 3\n[notes]\nx = \"y\"\n").expect("pins");
        assert_eq!(
            pins,
            BTreeMap::from([("rustc".to_string(), "1.99.1".to_string())])
        );
        assert!(read_pins("[tools\n").is_err());
    }
}
