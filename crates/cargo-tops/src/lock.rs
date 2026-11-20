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
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};

mod probe;
mod write;

pub(crate) use probe::{probe_in, run_probe, PROBE_LIMIT};

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
/// the first word after the prefix. The digits and dots at its front are the
/// version; a `+build` part after them is cut. A word that goes on with
/// anything else (`-rc.1`, `-nightly`) is a pre-release, and the whole word is
/// the version, which no pin matches. `None` when the line does not have that
/// shape.
pub(crate) fn parse_version(row: &Row, output: &str) -> Option<String> {
    let line = output.lines().next()?.trim();
    let word = line.strip_prefix(row.prefix)?.split_whitespace().next()?;
    let (number, rest) = word.split_at(
        word.find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(word.len()),
    );
    let number = number.trim_end_matches('.');
    if number.is_empty() {
        return None;
    }
    let version = if rest.is_empty() || rest.starts_with('+') {
        number
    } else {
        word
    };
    // A malformed version has an empty part: `1..2`, `.5`, or nothing at all.
    version
        .split('.')
        .all(|part| !part.is_empty())
        .then(|| version.to_string())
}

/// Whether `found` satisfies `pin`. Whole dot-separated parts only: `0.12`
/// matches `0.12.3` and not `0.120.0`; `0.12.3` does not match `0.12`. A
/// pre-release (`0.12.0-rc.1`) matches no pin.
pub(crate) fn pin_matches(pin: &str, found: &str) -> bool {
    let pin_parts: Vec<&str> = pin.split('.').collect();
    let found_parts: Vec<&str> = found.split('.').collect();
    !found.contains('-') && found_parts.starts_with(&pin_parts)
}

/// Whether `text` is dot-separated digits, the only shape a pin or a version
/// written to the lock has.
pub(crate) fn is_dotted_digits(text: &str) -> bool {
    text.split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
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
    /// The tool could not be run, or did not exit 0 in time: `asked` is the
    /// command and `why` how it failed.
    Missing {
        asked: String,
        why: String,
    },
    /// Nothing to ask: a library or name row, or a tool with no pin.
    Skipped {
        why: &'static str,
    },
    /// A lock key that names no tool rust-tops knows: not checked.
    Unknown,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) key: String,
    pub(crate) pin: String,
    /// The version command of the tool.
    pub(crate) asked: String,
    pub(crate) verdict: Verdict,
}

/// Why a layer cannot run on a pinned tool: the same words on stdout, in the
/// layer's `detail` and in its problems.
pub(crate) struct Stop {
    /// `true` for another version, `false` for a tool that would not answer.
    pub(crate) mismatch: bool,
    /// The command that was asked.
    pub(crate) asked: String,
    pub(crate) text: String,
}

impl Line {
    pub(crate) fn passes(&self) -> bool {
        matches!(
            self.verdict,
            Verdict::Ok { .. } | Verdict::Skipped { .. } | Verdict::Unknown
        )
    }

    /// The line `--verify` prints, and the detail of a stopped layer.
    pub(crate) fn text(&self) -> String {
        let Line {
            key, pin, verdict, ..
        } = self;
        match verdict {
            Verdict::Ok { found } => format!("ok       {key} {found} (pin {pin})"),
            Verdict::Mismatch { found } => {
                format!("mismatch {key}: pin {pin}, found {found}")
            }
            Verdict::Missing { asked, why } => {
                format!("missing  {key}: pin {pin}, `{asked}` {why}")
            }
            Verdict::Skipped { why } => format!("skipped  {key}: {why}"),
            Verdict::Unknown => format!("unknown  {key}: not a tool rust-tops knows, not checked"),
        }
    }

    /// What stops a layer: another version or a tool that would not answer.
    pub(crate) fn stop(&self) -> Option<Stop> {
        let (mismatch, asked) = match &self.verdict {
            Verdict::Mismatch { .. } => (true, self.asked.clone()),
            Verdict::Missing { asked, .. } => (false, asked.clone()),
            _ => return None,
        };
        Some(Stop {
            mismatch,
            asked,
            text: self.text(),
        })
    }
}

/// Reads `[tools]` from a lock text: `None` when there is no such table. A
/// `tools` that is not a table, a value that is not a string, and a pin of a
/// tool that is not dotted digits are errors naming the key.
pub(crate) fn read_lock(text: &str) -> Result<Option<BTreeMap<String, String>>> {
    let lock: toml::Table = toml::from_str(text).context("parse rust-tops.lock")?;
    let Some(tools) = lock.get("tools") else {
        return Ok(None);
    };
    let tools = tools
        .as_table()
        .ok_or_else(|| anyhow!("[tools] must be a table, found {}", tools.type_str()))?;
    tools
        .iter()
        .map(|(key, value)| Ok((key.clone(), pin_of(key, value)?)))
        .collect::<Result<_>>()
        .map(Some)
}

/// The pin `value` is for `key`.
fn pin_of(key: &str, value: &toml::Value) -> Result<String> {
    let pin = value.as_str().ok_or_else(|| {
        anyhow!(
            "[tools] {key}: a pin is a string such as \"0.9.0\", found {}",
            value.type_str()
        )
    })?;
    if row(key).is_some() && !is_dotted_digits(pin) {
        bail!(
            "[tools] {key} = {pin:?}: a pin is dot-separated digits such as \"0.9.0\" or \"0.9\""
        );
    }
    Ok(pin.to_string())
}

/// The pins of a lock text; no `[tools]` table is no pins.
pub(crate) fn read_pins(text: &str) -> Result<BTreeMap<String, String>> {
    Ok(read_lock(text)?.unwrap_or_default())
}

/// What a probe returns: the stdout of the command, or why it did not answer.
type Probe<'a> = dyn FnMut(&[&str]) -> Result<String, String> + 'a;

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
pub(crate) fn check_row(row: &Row, pin: &str, probe: &mut Probe<'_>) -> Line {
    let verdict = match probe(row.argv) {
        Err(why) => Verdict::Missing {
            asked: row.argv.join(" "),
            why,
        },
        Ok(out) => match row
            .present
            .iter()
            .find_map(|argv| probe(argv).err().map(|why| (argv.join(" "), why)))
        {
            Some((asked, why)) => Verdict::Missing { asked, why },
            None => version_verdict(row, pin, &out),
        },
    };
    Line {
        key: row.key.to_string(),
        pin: pin.to_string(),
        asked: row.argv.join(" "),
        verdict,
    }
}

/// What the line a tool printed says about its pin. A line with no version in
/// the shape of the row is a mismatch, and the line is named as found.
fn version_verdict(row: &Row, pin: &str, out: &str) -> Verdict {
    let Some(found) = parse_version(row, out) else {
        let line = out.lines().next().unwrap_or_default().trim();
        return Verdict::Mismatch {
            found: if line.is_empty() {
                "no version line".to_string()
            } else {
                line.to_string()
            },
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
/// skipped (a library or a runner) or unknown (anything else).
pub(crate) fn verify(
    pins: &BTreeMap<String, String>,
    only: Option<&[&str]>,
    probe: &mut Probe<'_>,
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
    probe: &mut Probe<'_>,
) -> Vec<Line> {
    ROWS.iter()
        .filter(|row| only.is_none_or(|only| only.contains(&row.key)))
        .filter_map(|row| match pins.get(row.key) {
            Some(pin) => Some(check_row(row, pin, probe)),
            None => only.map(|_| Line {
                key: row.key.to_string(),
                pin: String::new(),
                asked: row.argv.join(" "),
                verdict: Verdict::Skipped {
                    why: "no pin in rust-tops.lock",
                },
            }),
        })
        .collect()
}

/// The keys that are pinned in the lock but are not in the table of tools:
/// the three libraries and runners that are not tools on `PATH` are skipped,
/// and any other key (a misspelt tool, say) is named as unknown.
const NOT_TOOLS: [&str; 3] = ["proptest", "criterion", "callgrind-runner"];

fn unmatched_keys(pins: &BTreeMap<String, String>) -> Vec<Line> {
    pins.iter()
        .filter(|(key, _)| row(key).is_none())
        .map(|(key, pin)| Line {
            key: key.clone(),
            pin: pin.clone(),
            asked: String::new(),
            verdict: if NOT_TOOLS.contains(&key.as_str()) {
                Verdict::Skipped {
                    why: "a library or name, not a tool on PATH",
                }
            } else {
                Verdict::Unknown
            },
        })
        .collect()
}

/// The version each pinned tool prints now, for `--write`: the tools that
/// answered with a release version, as dot-separated digits. A tool that did
/// not answer, or printed a pre-release, is left out.
pub(crate) fn found_versions(
    pins: &BTreeMap<String, String>,
    probe: &mut Probe<'_>,
) -> BTreeMap<String, String> {
    ROWS.iter()
        .filter(|row| pins.contains_key(row.key))
        .filter_map(|row| Some((row.key.to_string(), release_version(row, probe)?)))
        .collect()
}

/// The release version `row`'s tool prints, when the tool and the commands that
/// show it is present all answer.
fn release_version(row: &Row, probe: &mut Probe<'_>) -> Option<String> {
    let out = probe(row.argv).ok()?;
    if row.present.iter().any(|argv| probe(argv).is_err()) {
        return None;
    }
    parse_version(row, &out).filter(|found| is_dotted_digits(found))
}

/// What `cargo tops lock` was asked to do.
pub(crate) enum Action {
    /// Check the pins; with a tier, only the tools the enabled layers at or
    /// below it need.
    Verify { max_tier: Option<u8> },
    /// Write the versions the tools print into `[tools]`.
    Write,
}

/// `cargo tops lock`. A lock that is unreadable, has no `[tools]` table or
/// holds a pin that cannot be read, or a config that will not load for
/// `--max-tier`, is an error (exit 2); a missing or mismatched tool is exit 3.
pub(crate) fn command(dir: &Path, action: &Action) -> anyhow::Result<ExitCode> {
    let path = dir.join(crate::TOOL_LOCK);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let pins = read_lock(&text)
        .with_context(|| format!("read {}", path.display()))?
        .ok_or_else(|| anyhow!("{} has no [tools] table", path.display()))?;
    match action {
        Action::Verify { max_tier } => verify_command(dir, &pins, *max_tier),
        Action::Write => write::write_command(dir, &path, &text, &pins),
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
    if lines.is_empty() {
        println!("nothing pinned in [tools]");
    }
    for line in &lines {
        println!("{}", line.text());
    }
    Ok(if lines.iter().all(Line::passes) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(3)
    })
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
                "missing  clippy: pin 1.99.1, `cargo clippy --version` exited 101",
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
        // A build part is cut, and a trailing dot goes; a pre-release keeps its
        // whole word, which no pin matches.
        for (out, want) in [
            ("cargo-nextest 1.2+build.5\n", "1.2"),
            ("cargo-nextest 1.\n", "1"),
            ("cargo-nextest 1.2-rc.1\n", "1.2-rc.1"),
            ("cargo-nextest 1.2.3-nightly (x)\n", "1.2.3-nightly"),
            ("cargo-nextest 1.2abc\n", "1.2abc"),
        ] {
            assert_eq!(
                parse_version(row("cargo-nextest"), out).as_deref(),
                Some(want),
                "{out:?}"
            );
        }
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
        // A pre-release matches no pin, however few parts the pin has.
        for pin in ["0", "0.9", "0.9.0"] {
            assert!(!pin_matches(pin, "0.9.0-rc.1"), "{pin}");
            assert!(!pin_matches(pin, "0.9.0-nightly"), "{pin}");
        }
    }

    #[test]
    fn dotted_digits_are_the_only_pins_and_versions_written() {
        for good in ["1", "0.12", "0.9.145"] {
            assert!(is_dotted_digits(good), "{good}");
        }
        for bad in [
            "",
            "1.",
            ".1",
            "1..2",
            "v1",
            "1.x",
            "=1",
            "1.0-rc",
            "gungraun-runner",
        ] {
            assert!(!is_dotted_digits(bad), "{bad:?}");
        }
    }

    fn probe_for<'a>(
        table: &'a [(&'a str, Option<&'a str>)],
    ) -> impl FnMut(&[&str]) -> Result<String, String> + 'a {
        move |argv| {
            table
                .iter()
                .find(|(tool, _)| argv.contains(tool))
                .and_then(|(_, out)| out.map(str::to_string))
                .ok_or_else(|| "exited 101".to_string())
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
                "missing  cargo-crap: pin 0.9.0, `cargo crap --version` exited 101",
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
        let mut probe = |_: &[&str]| -> Result<String, String> { panic!("asked a library row") };
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
    fn read_lock_reads_strings_and_names_every_other_kind_of_value() {
        let pins = read_pins("[tools]\nrustc = \"1.99.1\"\nproptest = \"1\"\ncallgrind-runner = \"gungraun-runner\"\n[notes]\nx = \"y\"\n").expect("pins");
        assert_eq!(pins.len(), 3);
        assert_eq!(
            pins.get("callgrind-runner").map(String::as_str),
            Some("gungraun-runner")
        );
        assert_eq!(read_lock("[notes]\nx = \"y\"\n").expect("no tools"), None);
        assert!(read_pins("[notes]\n").expect("no pins").is_empty());
        for (lock, words) in [
            ("[tools\n", "parse"),
            (
                "[tools]\nrustc = 1\n",
                "[tools] rustc: a pin is a string such as \"0.9.0\", found integer",
            ),
            ("[tools]\nrustc = 1.5\n", "found float"),
            ("[tools]\nrustc = { v = 1 }\n", "found table"),
            ("[tools]\nrustc = [\"1\"]\n", "found array"),
            ("[tools]\nrustc = true\n", "found boolean"),
            ("tools = \"x\"\n", "[tools] must be a table, found string"),
            ("[[tools]]\n", "[tools] must be a table, found array"),
            (
                "[tools]\nrustc = \"=1\"\n",
                "[tools] rustc = \"=1\": a pin is dot-separated digits",
            ),
        ] {
            let err = read_lock(lock).expect_err(lock);
            assert!(format!("{err:#}").contains(words), "{lock}: {err:#}");
        }
    }

    #[test]
    fn an_empty_or_garbled_line_is_a_mismatch_that_says_what_was_printed() {
        let pins = BTreeMap::from([("cargo-crap".to_string(), "0.9.0".to_string())]);
        for (out, found) in [
            ("", "no version line"),
            ("\n", "no version line"),
            ("what\n", "what"),
        ] {
            let mut probe = |_: &[&str]| Ok(out.to_string());
            let lines = verify(&pins, None, &mut probe);
            assert_eq!(
                lines.first().map(Line::text),
                Some(format!("mismatch cargo-crap: pin 0.9.0, found {found}")),
                "{out:?}"
            );
        }
    }

    #[test]
    fn a_failed_probe_carries_its_command_and_reason_into_the_stop() {
        let pins = BTreeMap::from([("cargo-crap".to_string(), "0.9.0".to_string())]);
        let mut probe = |_: &[&str]| Err("timed out after 10s".to_string());
        let lines = verify(&pins, None, &mut probe);
        let stop = lines.first().and_then(Line::stop).expect("a stop");
        assert!(!stop.mismatch);
        assert_eq!(stop.asked, "cargo crap --version");
        assert_eq!(
            stop.text,
            "missing  cargo-crap: pin 0.9.0, `cargo crap --version` timed out after 10s"
        );
        let mut ok = |_: &[&str]| Ok("cargo-crap 0.9.1\n".to_string());
        let lines = verify(&pins, None, &mut ok);
        let stop = lines.first().and_then(Line::stop).expect("a stop");
        assert!(stop.mismatch);
        assert_eq!(stop.asked, "cargo crap --version");
        let mut fine = |_: &[&str]| Ok("cargo-crap 0.9.0\n".to_string());
        assert!(verify(&pins, None, &mut fine)
            .first()
            .and_then(Line::stop)
            .is_none());
    }

    #[test]
    fn a_failing_presence_command_names_that_command() {
        let pins = BTreeMap::from([("rustfmt".to_string(), "1.99.1".to_string())]);
        let mut probe = probe_for(&[("rustc", Some("rustc 1.99.1 (x)\n")), ("fmt", None)]);
        let lines = verify(&pins, None, &mut probe);
        assert_eq!(
            lines.first().map(Line::text).as_deref(),
            Some("missing  rustfmt: pin 1.99.1, `cargo fmt --version` exited 101")
        );
    }

    #[test]
    fn unknown_keys_pass_with_a_warning_and_only_the_named_libraries_are_skipped() {
        let pins = BTreeMap::from([
            ("criterion".to_string(), "0.8".to_string()),
            ("cargo-nexttest".to_string(), "0.9.145".to_string()),
        ]);
        let lines = verify(
            &pins,
            None,
            &mut |_: &[&str]| Err("never asked".to_string()),
        );
        let texts: Vec<String> = lines.iter().map(Line::text).collect();
        assert_eq!(
            texts,
            [
                "unknown  cargo-nexttest: not a tool rust-tops knows, not checked",
                "skipped  criterion: a library or name, not a tool on PATH",
            ]
        );
        assert!(lines.iter().all(Line::passes));
    }

    #[test]
    fn found_versions_leave_out_a_tool_that_will_not_answer_and_a_pre_release() {
        let pins = BTreeMap::from([
            ("cargo-crap".to_string(), "0.9.0".to_string()),
            ("cargo-deny".to_string(), "0.18.9".to_string()),
            ("cargo-fuzz".to_string(), "0.12".to_string()),
            ("clippy".to_string(), "1.99.1".to_string()),
            ("rustc".to_string(), "1.99.1".to_string()),
        ]);
        let mut probe = probe_for(&[
            ("crap", None),
            ("deny", Some("cargo-deny 0.18.9-rc.1\n")),
            ("fuzz", Some("cargo-fuzz 0.12.3\n")),
            ("clippy", None),
            ("rustc", Some("rustc 1.99.2 (x)\n")),
        ]);
        assert_eq!(
            found_versions(&pins, &mut probe),
            BTreeMap::from([
                ("cargo-fuzz".to_string(), "0.12.3".to_string()),
                ("rustc".to_string(), "1.99.2".to_string()),
            ])
        );
    }
}
