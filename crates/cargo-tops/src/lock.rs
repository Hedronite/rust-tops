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
use std::path::Path;
use std::process::{Command, ExitCode};

use anyhow::{Context, Result};

/// One pinned tool: its lock key, the argv that prints its version, and the
/// text the first line starts with before the version.
pub(crate) struct Row {
    pub(crate) key: &'static str,
    pub(crate) argv: &'static [&'static str],
    pub(crate) prefix: &'static str,
    /// Other argvs that must exit 0 for the tool to count as present.
    pub(crate) present: &'static [&'static [&'static str]],
}

const RUSTC: &[&str] = &["rustc", "--version"];

/// The table. Order is the order `--verify` prints.
pub(crate) const ROWS: [Row; 11] = [
    Row {
        key: "rustc",
        argv: RUSTC,
        prefix: "rustc ",
        present: &[],
    },
    Row {
        key: "clippy",
        argv: RUSTC,
        prefix: "rustc ",
        present: &[&["cargo", "clippy", "--version"]],
    },
    Row {
        key: "rustfmt",
        argv: RUSTC,
        prefix: "rustc ",
        present: &[&["cargo", "fmt", "--version"]],
    },
    Row {
        key: "cargo-nextest",
        argv: &["cargo", "nextest", "--version"],
        prefix: "cargo-nextest ",
        present: &[],
    },
    Row {
        key: "cargo-llvm-cov",
        argv: &["cargo", "llvm-cov", "--version"],
        prefix: "cargo-llvm-cov ",
        present: &[],
    },
    Row {
        key: "cargo-mutants",
        argv: &["cargo", "mutants", "--version"],
        prefix: "cargo-mutants ",
        present: &[],
    },
    Row {
        key: "cargo-crap",
        argv: &["cargo", "crap", "--version"],
        prefix: "cargo-crap ",
        present: &[],
    },
    Row {
        key: "cargo-deny",
        argv: &["cargo", "deny", "--version"],
        prefix: "cargo-deny ",
        present: &[],
    },
    Row {
        key: "cargo-fuzz",
        argv: &["cargo", "fuzz", "--version"],
        prefix: "cargo-fuzz ",
        present: &[],
    },
    Row {
        key: "kani-verifier",
        argv: &["cargo", "kani", "--version"],
        prefix: "Kani Rust Verifier ",
        present: &[],
    },
    Row {
        key: "nushell",
        argv: &["nu", "--version"],
        prefix: "",
        present: &[],
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
    // Only digits and dots are left, so a malformed version has an empty part:
    // `1..2`, `.5`, or nothing at all.
    version
        .split('.')
        .all(|part| !part.is_empty())
        .then(|| version.to_string())
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
    /// The tool could not be run, or exited non-zero; `asked` is the command.
    Missing {
        asked: String,
    },
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
            Verdict::Missing { .. } => format!("missing  {key}: pin {pin}, not found"),
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

/// A probe that runs each argv in `dir` and returns its stdout, or `None`
/// when it could not run or failed.
pub(crate) fn probe_in(dir: &Path) -> impl FnMut(&[&str]) -> Option<String> + '_ {
    move |argv| {
        let (program, args) = argv.split_first()?;
        let output = Command::new(program)
            .args(args)
            .current_dir(dir)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// The row for a lock key.
pub(crate) fn row(key: &str) -> Option<&'static Row> {
    ROWS.iter().find(|row| row.key == key)
}

/// The pinned tools the given layers need, each with its pin, in table order.
/// A tool with no pin in the lock is left out: nothing to check.
pub(crate) fn pins_for<'a>(
    layers: impl IntoIterator<Item = &'a str>,
    pins: &BTreeMap<String, String>,
) -> Vec<(&'static Row, String)> {
    tools_for(layers)
        .into_iter()
        .filter_map(|key| Some((row(key)?, pins.get(key)?.clone())))
        .collect()
}

/// Asks one pinned tool. `probe` runs an argv and returns its stdout.
pub(crate) fn check_row(
    row: &Row,
    pin: &str,
    probe: &mut dyn FnMut(&[&str]) -> Option<String>,
) -> Line {
    let verdict = match probe(row.argv) {
        None => Verdict::Missing {
            asked: row.argv.join(" "),
        },
        Some(out) => match row.present.iter().find(|argv| probe(argv).is_none()) {
            Some(argv) => Verdict::Missing {
                asked: argv.join(" "),
            },
            None => version_verdict(row, pin, &out),
        },
    };
    Line {
        key: row.key.to_string(),
        pin: pin.to_string(),
        verdict,
    }
}

/// What the line a tool printed says about its pin. A line with no version in
/// the shape of the row is a mismatch, and the line is named as found.
fn version_verdict(row: &Row, pin: &str, out: &str) -> Verdict {
    let Some(found) = parse_version(row, out) else {
        return Verdict::Mismatch {
            found: out.lines().next().unwrap_or_default().trim().to_string(),
        };
    };
    if pin_matches(pin, &found) {
        Verdict::Ok { found }
    } else {
        Verdict::Mismatch { found }
    }
}

/// Checks the pins. With `only`, the tools in it (a tool with no pin is
/// skipped); without, every pin in the lock, and a lock key with no row is
/// skipped.
pub(crate) fn verify(
    pins: &BTreeMap<String, String>,
    only: Option<&[&str]>,
    probe: &mut dyn FnMut(&[&str]) -> Option<String>,
) -> Vec<Line> {
    let mut lines = tool_lines(pins, only, probe);
    if only.is_none() {
        lines.extend(unmatched_keys(pins));
    }
    lines
}

/// One line for each table row the check is about, in table order.
fn tool_lines(
    pins: &BTreeMap<String, String>,
    only: Option<&[&str]>,
    probe: &mut dyn FnMut(&[&str]) -> Option<String>,
) -> Vec<Line> {
    ROWS.iter()
        .filter(|row| only.is_none_or(|only| only.contains(&row.key)))
        .filter_map(|row| match pins.get(row.key) {
            Some(pin) => Some(check_row(row, pin, probe)),
            None => only.map(|_| Line {
                key: row.key.to_string(),
                pin: String::new(),
                verdict: Verdict::Skipped {
                    why: "no pin in rust-tops.lock",
                },
            }),
        })
        .collect()
}

/// The lock keys that name no tool: a library or a runner. Skipped.
fn unmatched_keys(pins: &BTreeMap<String, String>) -> Vec<Line> {
    pins.iter()
        .filter(|(key, _)| row(key).is_none())
        .map(|(key, pin)| Line {
            key: key.clone(),
            pin: pin.clone(),
            verdict: Verdict::Skipped {
                why: "a library or name, not a tool on PATH",
            },
        })
        .collect()
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

/// What `cargo tops lock` was asked to do.
pub(crate) enum Action {
    /// Check the pins; with a tier, only the tools the enabled layers at or
    /// below it need.
    Verify { max_tier: Option<u8> },
    /// Write the versions the tools print into `[tools]`.
    Write,
}

/// `cargo tops lock`. An unreadable lock, or a config that will not load for
/// `--max-tier`, is an error (exit 2); a missing or mismatched tool is exit 3.
pub(crate) fn command(dir: &Path, action: &Action) -> anyhow::Result<ExitCode> {
    let path = dir.join(crate::TOOL_LOCK);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let pins = read_pins(&text).with_context(|| format!("read {}", path.display()))?;
    match action {
        Action::Verify { max_tier } => verify_command(dir, &pins, *max_tier),
        Action::Write => write_command(dir, &path, &text, &pins),
    }
}

/// The tools the enabled layers at or below `tier` need, from the config.
fn tools_in_range(dir: &Path, tier: u8) -> anyhow::Result<Vec<&'static str>> {
    let cfg = crate::load_config(dir)?;
    Ok(tools_for(
        cfg.layers
            .iter()
            .filter(|layer| layer.enabled && crate::tiers::tier_for(layer) <= tier)
            .map(|layer| layer.id.as_str()),
    ))
}

fn verify_command(
    dir: &Path,
    pins: &BTreeMap<String, String>,
    max_tier: Option<u8>,
) -> anyhow::Result<ExitCode> {
    let needed = max_tier.map(|tier| tools_in_range(dir, tier)).transpose()?;
    let lines = verify(pins, needed.as_deref(), &mut probe_in(dir));
    for line in &lines {
        println!("{}", line.text());
    }
    Ok(if lines.iter().all(Line::passes) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(3)
    })
}

/// One line for each pinned tool `--write` is about: the old and new value
/// when they differ, and that a tool that could not be run keeps its value.
fn write_lines(pins: &BTreeMap<String, String>, found: &BTreeMap<String, String>) -> Vec<String> {
    pins.iter()
        .filter(|(key, _)| row(key).is_some())
        .map(|(key, pin)| match found.get(key) {
            Some(now) if now != pin => format!("wrote    {key}: {pin} -> {now}"),
            Some(_) => format!("kept     {key} {pin}"),
            None => format!("kept     {key} {pin}: not found"),
        })
        .collect()
}

fn write_command(
    dir: &Path,
    path: &Path,
    text: &str,
    pins: &BTreeMap<String, String>,
) -> anyhow::Result<ExitCode> {
    let found = found_versions(pins, &mut probe_in(dir));
    for line in write_lines(pins, &found) {
        println!("{line}");
    }
    let written = write_values(text, &found);
    if written != text {
        std::fs::write(path, written).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(ExitCode::SUCCESS)
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
    fn each_row_reads_the_line_a_real_tool_printed() {
        for (key, file, want) in [
            ("rustc", "rustc", "1.99.1"),
            ("cargo-nextest", "cargo-nextest", "0.9.145"),
            ("cargo-llvm-cov", "cargo-llvm-cov", "0.9.1"),
            ("cargo-mutants", "cargo-mutants", "27.1.0"),
            ("cargo-crap", "cargo-crap", "0.9.0"),
            ("cargo-deny", "cargo-deny", "0.18.9"),
            ("cargo-fuzz", "cargo-fuzz", "0.12.0"),
            ("kani-verifier", "kani-verifier", "0.68.0"),
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
    fn clippy_or_rustfmt_that_will_not_run_is_missing_though_rustc_answers() {
        let pins = BTreeMap::from([
            ("clippy".to_string(), "1.99.1".to_string()),
            ("rustfmt".to_string(), "1.99.1".to_string()),
        ]);
        let mut probe = probe_for(&[
            ("rustc", Some("rustc 1.99.1 (x 2026-10-13)\n")),
            ("clippy", None),
            ("fmt", Some("rustfmt 1.10.0-stable (x)\n")),
        ]);
        let texts: Vec<String> = verify(&pins, None, &mut probe)
            .iter()
            .map(Line::text)
            .collect();
        assert_eq!(
            texts,
            [
                "missing  clippy: pin 1.99.1, not found",
                "ok       rustfmt 1.99.1 (pin 1.99.1)",
            ]
        );
    }

    #[test]
    fn a_version_with_an_empty_part_reads_as_none() {
        for out in [
            "cargo-nextest 1..2\n",
            "cargo-nextest .5\n",
            "cargo-nextest \n",
        ] {
            assert_eq!(parse_version(row("cargo-nextest"), out), None, "{out:?}");
        }
        // A suffix after the digits and dots is cut, and a trailing dot goes.
        for (out, want) in [
            ("cargo-nextest 1.2-rc\n", "1.2"),
            ("cargo-nextest 1.\n", "1"),
        ] {
            assert_eq!(
                parse_version(row("cargo-nextest"), out).as_deref(),
                Some(want),
                "{out:?}"
            );
        }
    }

    #[test]
    fn write_lines_say_what_changed_and_leave_library_rows_out() {
        let pins = BTreeMap::from([
            ("cargo-crap".to_string(), "0.9.0".to_string()),
            ("cargo-deny".to_string(), "0.18.9".to_string()),
            ("cargo-nextest".to_string(), "0.9.145".to_string()),
            ("proptest".to_string(), "1".to_string()),
        ]);
        let found = BTreeMap::from([
            ("cargo-deny".to_string(), "0.18.9".to_string()),
            ("cargo-nextest".to_string(), "0.9.150".to_string()),
        ]);
        assert_eq!(
            write_lines(&pins, &found),
            [
                "kept     cargo-crap 0.9.0: not found",
                "kept     cargo-deny 0.18.9",
                "wrote    cargo-nextest: 0.9.145 -> 0.9.150",
            ]
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
