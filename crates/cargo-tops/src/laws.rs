//! The `laws` layer (protocol 1.1.0, §3.1).
//!
//! `gate` ensures the Bend release `laws/bend.lock` pins, writes the cases
//! [`crate::facts`] chose as one Bend program, and runs `bend` on it. The
//! program's only obligation is that each verdict is not `Refuse`; the
//! verdicts themselves come from `LAWS.bend`, and the pass or fail is
//! `bend`'s exit status.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde_json::json;

use crate::facts::{self, Choice, Ctx};
use crate::profiles;

pub const PACK: &str = "LAWS.bend";
pub const LOCK: &str = "laws/bend.lock";
pub const FETCH: &str = "scripts/fetch-bend.sh";
pub const OUT_DIR: &str = "target/rust-tops";

/// Directory that receives `cases.bend` and `cases.json`.
///
/// A nonempty `CARGO_TARGET_DIR` wins. Otherwise, when `Cargo.toml` exists
/// and `cargo metadata` returns JSON, that `target_directory` wins. Otherwise
/// the report is `<project>/target/rust-tops`.
pub fn report_dir(project: &Path) -> PathBuf {
    let from_env = std::env::var("CARGO_TARGET_DIR").ok();
    resolve_report_dir(
        project,
        from_env.as_deref(),
        metadata_target(project).as_deref(),
    )
}

/// Same rule as [`report_dir`], with the two lookups passed in.
pub fn resolve_report_dir(
    project: &Path,
    cargo_target_dir: Option<&str>,
    metadata_target: Option<&str>,
) -> PathBuf {
    if let Some(dir) = nonempty(cargo_target_dir) {
        return PathBuf::from(dir).join("rust-tops");
    }
    if let Some(dir) = nonempty(metadata_target) {
        return PathBuf::from(dir).join("rust-tops");
    }
    project.join(OUT_DIR)
}

/// Path printed in gate output. Stays `target/rust-tops` when the report
/// sits under the project.
pub fn report_display(project: &Path) -> String {
    display_under(project, &report_dir(project))
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

fn display_under(project: &Path, out: &Path) -> String {
    out.strip_prefix(project)
        .ok()
        .map(|rel| rel.display().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| out.display().to_string())
}

fn metadata_target(project: &Path) -> Option<String> {
    let manifest = project.join("Cargo.toml");
    if !manifest.is_file() {
        return None;
    }
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("target_directory")?.as_str().map(str::to_string)
}

/// The `bend` binary that reports the pinned version.
pub struct Judge {
    bend: PathBuf,
    version: String,
}

fn lock_version(raw: &str) -> Option<String> {
    let table: toml::Table = raw.parse().ok()?;
    table
        .get("bend")?
        .get("version")?
        .as_str()
        .map(str::to_string)
}

fn reports_version(bend: &Path, want: &str) -> bool {
    let Ok(out) = Command::new(bend)
        .arg("version")
        .env("BEND_NO_TELEMETRY", "1")
        .output()
    else {
        return false;
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let got = stdout
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1));
    out.status.success() && got == Some(want)
}

fn home_bend() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".bend/bin/bend")).filter(|path| path.is_file())
}

fn path_bend() -> Option<PathBuf> {
    let out = Command::new("sh")
        .args(["-c", "command -v bend"])
        .output()
        .ok()?;
    let found = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !found.is_empty()).then(|| PathBuf::from(found))
}

/// Find the pinned Bend on `PATH` or in `$HOME/.bend`, else run the vendored
/// `scripts/fetch-bend.sh` (sha256-checked, no `curl | sh`). Every miss is a
/// failed gate, never a skip (H-37).
pub fn ensure_bend(dir: &Path) -> Result<Judge, String> {
    let raw = fs::read_to_string(dir.join(LOCK))
        .map_err(|_| format!("laws: {LOCK} is missing; run `cargo tops init` to write it"))?;
    let want = lock_version(&raw).ok_or_else(|| format!("laws: {LOCK} has no [bend] version"))?;
    let found = path_bend()
        .into_iter()
        .chain(home_bend())
        .find(|bend| reports_version(bend, &want));
    if let Some(bend) = found {
        return Ok(Judge {
            bend,
            version: want,
        });
    }
    let script = dir.join(FETCH);
    if !script.is_file() {
        return Err(format!(
            "laws: Bend {want} ({LOCK}) is not installed and {FETCH} is missing; run `cargo tops init`"
        ));
    }
    println!("\n==> [laws] sh {FETCH} (Bend {want} from {LOCK} is not installed)");
    let status = Command::new("sh")
        .arg(&script)
        .current_dir(dir)
        .env("BEND_NO_TELEMETRY", "1")
        .status()
        .map_err(|err| format!("laws: spawn {FETCH}: {err}"))?;
    if !status.success() {
        return Err(format!(
            "laws: {FETCH} failed ({status}); Bend {want} is not installed"
        ));
    }
    home_bend()
        .filter(|bend| reports_version(bend, &want))
        .map(|bend| Judge {
            bend,
            version: want.clone(),
        })
        .ok_or_else(|| {
            format!("laws: {FETCH} finished, but $HOME/.bend/bin/bend does not report {want}")
        })
}

/// One law of the adopter's pack, with the case chosen for it or the reason
/// there is none.
struct Row {
    law: String,
    case: Option<(&'static str, &'static str)>,
    read: Vec<String>,
    reason: Option<String>,
    reason_code: Option<&'static str>,
}

fn pack_laws(pack: &str) -> Vec<String> {
    pack.lines()
        .filter_map(|line| {
            line.strip_prefix("law ")
                .and_then(|rest| rest.strip_suffix(':'))
        })
        .map(|law| law.trim().to_string())
        .collect()
}

fn choose_rows(pack: &str, ctx: &Ctx) -> Vec<Row> {
    pack_laws(pack)
        .into_iter()
        .map(|law| {
            let Some(fact) = facts::find(&law) else {
                let reason = facts::not_evaluated(&law).to_string();
                return Row {
                    law,
                    case: None,
                    read: Vec::new(),
                    reason: Some(reason),
                    reason_code: Some(facts::NO_FACT_READER),
                };
            };
            match (fact.choose)(ctx) {
                Choice::Fact { met, read } => Row {
                    law,
                    case: Some((fact.fun, fact.ctor(met))),
                    read,
                    reason: None,
                    reason_code: None,
                },
                Choice::NotEvaluated { reason, code } => Row {
                    law,
                    case: None,
                    read: Vec::new(),
                    reason: Some(reason),
                    reason_code: Some(code),
                },
            }
        })
        .collect()
}

const PROGRAM_HEAD: &str = "\
# Written by `cargo tops gate` (laws layer, rust-tops 1.1.0 §3.1). Do not edit.
# Each law below applies a LAWS.bend verdict to the case cargo-tops chose.
# The obligation is only that the verdict is not Refuse; bend checks it.
import Base
import ./LAWS.bend as Laws

def not_refuse(v: Laws.Verdict) -> Bool:
  match v:
    case Laws.Refuse{}:
      False{}
    case Laws.Ack{}:
      True{}
    case Laws.Allow{}:
      True{}
";

/// The case file: one obligation per checked law, and a `main` that returns
/// every verdict so `bend` prints them.
fn program(rows: &[Row]) -> String {
    let mut out = PROGRAM_HEAD.to_string();
    let mut calls = Vec::new();
    for row in rows {
        let Some((fun, ctor)) = row.case else {
            continue;
        };
        let call = format!("Laws.{fun}(Laws.{ctor}{{}})");
        let read = row.read.join(", ").replace(['\n', '\r'], " ");
        let _ = write!(
            out,
            "\n# {law}: {ctor}, read {read}\nlaw case_{law}:\n  {{not_refuse({call}) == True{{}} : Bool}}\n\ndef case_{law}():\n  {{==}}\n",
            law = row.law,
        );
        calls.push(call);
    }
    match calls.len() {
        0 => {}
        1 => {
            let _ = write!(out, "\ndef main() -> Laws.Verdict:\n  {}\n", calls[0]);
        }
        n => {
            let types = vec!["Laws.Verdict"; n].join(" & ");
            let _ = write!(out, "\ndef main() -> {types}:\n  ({})\n", calls.join(", "));
        }
    }
    out
}

/// Verdict names in the order `bend` printed `main`'s value.
fn printed_verdicts(output: &str) -> Vec<&str> {
    output
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | ','))
        .filter_map(|token| token.rsplit('.').next())
        .filter_map(|name| name.strip_suffix("{}"))
        .filter(|name| matches!(*name, "Allow" | "Ack" | "Refuse"))
        .collect()
}

/// The law `bend` stopped on, from its `Location: case_<law>` line.
fn failed_law(output: &str) -> Option<&str> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("Location: case_"))
}

struct Judged<'r> {
    rows: &'r [Row],
    verdicts: Vec<String>,
    passed: bool,
    failed_law: Option<String>,
}

impl Judged<'_> {
    /// `bend`'s verdict for row `index`, or what we can honestly say instead.
    fn status(&self, row: &Row, checked_index: usize) -> String {
        if row.case.is_none() {
            return "not-evaluated".into();
        }
        if self.passed {
            return self
                .verdicts
                .get(checked_index)
                .cloned()
                .unwrap_or_else(|| "unread".into());
        }
        if self.failed_law.as_deref() == Some(row.law.as_str()) {
            "Refuse".into()
        } else {
            "unreported".into()
        }
    }

    fn print(&self) {
        let mut checked = 0;
        for row in self.rows {
            let status = self.status(row, checked);
            match (row.case, &row.reason) {
                (Some((_, ctor)), _) => {
                    checked += 1;
                    println!(
                        "  {status:<13} {:<40} {ctor:<22} {}",
                        row.law,
                        row.read.join(", ")
                    );
                }
                (None, reason) => {
                    println!(
                        "  {status:<13} {:<40} {}",
                        row.law,
                        reason.as_deref().unwrap_or("")
                    );
                }
            }
        }
        let skipped = self.rows.len() - checked;
        println!(
            "laws: {checked} checked by bend, {skipped} not-evaluated (not checked, not passed)"
        );
        if !self.passed && self.failed_law.is_some() {
            println!("laws: bend stops at the first refusal; `unreported` laws were not judged");
        }
    }

    fn json(&self, judge: &Judge, exit: Option<i32>, program: &str) -> serde_json::Value {
        let mut checked = 0;
        let laws: Vec<serde_json::Value> = self
            .rows
            .iter()
            .map(|row| {
                let status = self.status(row, checked);
                let entry = match row.case {
                    Some((fun, ctor)) => json!({
                        "law": row.law, "status": "checked", "verdict_fn": fun, "case": ctor,
                        "read": row.read, "verdict": status, "reason_code": null,
                    }),
                    None => json!({
                        "law": row.law, "status": "not-evaluated", "case": null,
                        "read": [], "verdict": null, "reason": row.reason,
                        "reason_code": row.reason_code,
                    }),
                };
                checked += usize::from(row.case.is_some());
                entry
            })
            .collect();
        json!({
            "protocol_layer": "laws",
            "bend": judge.version,
            "program": program,
            "bend_exit": exit,
            "passed": self.passed,
            "laws": laws,
        })
    }
}

/// Write the case file, run `bend`, print and record the result. Returns
/// the gate failures, empty when `bend` accepted every checked law.
pub fn run(judge: &Judge, ctx: &Ctx) -> Result<Vec<String>> {
    let Ok(pack) = fs::read_to_string(ctx.dir.join(PACK)) else {
        return Ok(vec![format!(
            "laws: {PACK} is missing; run `cargo tops init`"
        )]);
    };
    let rows = choose_rows(&pack, ctx);
    let out = report_dir(ctx.dir);
    let shown = display_under(ctx.dir, &out);
    fs::create_dir_all(&out).with_context(|| format!("create {}", out.display()))?;
    fs::write(out.join(PACK), &pack).with_context(|| format!("write {shown}/{PACK}"))?;
    fs::write(out.join("cases.bend"), program(&rows))
        .with_context(|| format!("write {shown}/cases.bend"))?;

    println!(
        "\n==> [laws] bend {shown}/cases.bend (Bend {})",
        judge.version
    );
    if pack != profiles::LAWS_BEND {
        println!(
            "note: {PACK} differs from the pack cargo-tops {} ships; bend judges this repo's copy",
            env!("CARGO_PKG_VERSION")
        );
    }
    let output = Command::new(&judge.bend)
        .arg("cases.bend")
        .current_dir(&out)
        .env("BEND_NO_TELEMETRY", "1")
        .output()
        .with_context(|| format!("spawn {}", judge.bend.display()))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let checked = rows.iter().filter(|row| row.case.is_some()).count();
    let verdicts: Vec<String> = printed_verdicts(&text)
        .into_iter()
        .map(str::to_string)
        .collect();
    let readable = verdicts.len() == checked && !verdicts.iter().any(|v| v == "Refuse");
    let judged = Judged {
        rows: &rows,
        passed: output.status.success() && readable,
        verdicts,
        failed_law: failed_law(&text).map(str::to_string),
    };
    judged.print();

    let mut failures = Vec::new();
    if !output.status.success() {
        for line in text.lines() {
            println!("    | {line}");
        }
        failures.push(match &judged.failed_law {
            Some(law) => format!("laws: bend refused {law} ({shown}/cases.bend)"),
            None => format!("laws: bend exited {} on {shown}/cases.bend", output.status),
        });
    } else if !readable {
        failures.push(format!(
            "laws: bend printed {} verdict(s) for {checked} checked law(s)",
            judged.verdicts.len()
        ));
    }
    let report = serde_json::to_string_pretty(&judged.json(
        judge,
        output.status.code(),
        &format!("{shown}/cases.bend"),
    ))? + "\n";
    fs::write(out.join("cases.json"), report)
        .with_context(|| format!("write {shown}/cases.json"))?;
    Ok(failures)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn row(law: &str, case: Option<(&'static str, &'static str)>) -> Row {
        Row {
            law: law.into(),
            case,
            read: vec!["rust-tops.yaml".into()],
            reason: None,
            reason_code: None,
        }
    }

    #[test]
    fn program_has_one_obligation_per_checked_law_and_prints_every_verdict() {
        let rows = [
            row("fmt_is_tier0", Some(("fmt_verdict", "FmtTier0"))),
            row("evidence_not_self_served", None),
            row("lock_honesty", Some(("lock_verdict", "LockDrift"))),
        ];
        let text = program(&rows);
        assert!(text.contains("import ./LAWS.bend as Laws"));
        assert!(text.contains("law case_fmt_is_tier0:\n  {not_refuse(Laws.fmt_verdict(Laws.FmtTier0{})) == True{} : Bool}"));
        assert!(text.contains("law case_lock_honesty:"));
        assert!(
            !text.contains("evidence_not_self_served"),
            "a not-evaluated law must not be encoded"
        );
        assert!(text.contains(
            "def main() -> Laws.Verdict & Laws.Verdict:\n  (Laws.fmt_verdict(Laws.FmtTier0{}), Laws.lock_verdict(Laws.LockDrift{}))"
        ));
    }

    #[test]
    fn program_without_checked_laws_has_no_main() {
        let text = program(&[row("evidence_not_self_served", None)]);
        assert!(!text.contains("def main"));
        assert!(!text.contains("law case_"));
    }

    #[test]
    fn single_checked_law_returns_one_verdict() {
        let text = program(&[row("fmt_is_tier0", Some(("fmt_verdict", "FmtTier0")))]);
        assert!(text.contains("def main() -> Laws.Verdict:\n  Laws.fmt_verdict(Laws.FmtTier0{})\n"));
    }

    #[test]
    fn reads_the_verdicts_bend_prints() {
        assert_eq!(
            printed_verdicts("(LAWS.Allow{}, LAWS.Ack{}, LAWS.Allow{})\n"),
            ["Allow", "Ack", "Allow"]
        );
        assert_eq!(printed_verdicts("LAWS.Allow{}\n"), ["Allow"]);
        assert!(printed_verdicts("ALL PROOFS CHECK\n").is_empty());
    }

    #[test]
    fn reads_the_law_bend_stopped_on() {
        let out = "SOME PROOFS FAIL\nError:\n- expected : False{}\n- observed : True{}\nLocation: case_lock_honesty\n";
        assert_eq!(failed_law(out), Some("lock_honesty"));
        assert_eq!(failed_law("SOME PROOFS FAIL\nLocation: main\n"), None);
    }

    #[test]
    fn lock_version_reads_the_bend_section() {
        assert_eq!(lock_version(profiles::BEND_LOCK).as_deref(), Some("2.0.36"));
        assert_eq!(lock_version("[bend]\nnote = \"x\"\n"), None);
    }

    #[test]
    fn report_dir_prefers_a_nonempty_cargo_target_dir() {
        let project = Path::new("/proj");
        let got = resolve_report_dir(project, Some("/env-target"), Some("/meta-target"));
        assert_eq!(got, PathBuf::from("/env-target/rust-tops"));
    }

    #[test]
    fn report_dir_uses_metadata_when_the_env_is_empty() {
        let project = Path::new("/proj");
        assert_eq!(
            resolve_report_dir(project, Some("  "), Some("/meta-target")),
            PathBuf::from("/meta-target/rust-tops")
        );
        assert_eq!(
            resolve_report_dir(project, None, Some("/meta-target")),
            PathBuf::from("/meta-target/rust-tops")
        );
    }

    #[test]
    fn metadata_target_reads_target_directory() {
        let dir = std::env::temp_dir().join(format!("cargo-tops-meta-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"meta-fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(dir.join("src/lib.rs"), "// fixture\n").unwrap();
        let got = metadata_target(&dir).expect("cargo metadata");
        assert!(Path::new(&got).is_absolute(), "{got}");
        assert!(got.contains("target"), "{got}");
        let report = resolve_report_dir(&dir, None, Some(&got));
        assert_eq!(report, PathBuf::from(got).join("rust-tops"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn report_dir_falls_back_to_the_project_target() {
        let project = Path::new("/proj");
        assert_eq!(
            resolve_report_dir(project, None, None),
            PathBuf::from("/proj/target/rust-tops")
        );
        assert_eq!(
            resolve_report_dir(project, Some(""), Some("")),
            PathBuf::from("/proj/target/rust-tops")
        );
    }

    #[test]
    fn pack_lists_all_53_laws() {
        let laws = pack_laws(profiles::LAWS_BEND);
        assert_eq!(laws.len(), 53);
        assert_eq!(laws[0], "evidence_not_self_served");
    }
}
