//! The end of a gate: the summary on stdout, `report.json`, the exit code.
//!
//! `report.json` is written on every run that knows its target: a pass, a
//! failure, a dry run, a missing tool, and a config error. It validates
//! against `schema/gate-report.schema.json`.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde_json::{json, Map, Value};

use super::execute::Executed;
use super::judge::Judgement;
use super::plan::{GatePlan, What};
use super::{Fault, Mode, Options};
use crate::config::TopsConfig;
use crate::facts::{Outcome, Run};
use crate::floors::{CoverageNumbers, CrapNumbers};
use crate::{laws, PROTOCOL_VERSION};

/// The file name under the gate's report directory.
pub(crate) const REPORT: &str = "report.json";

/// What decided the exit code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Pass,
    Failed,
    Config,
    Env,
}

impl Kind {
    fn code(self) -> u8 {
        match self {
            Self::Pass => 0,
            Self::Failed => 1,
            Self::Config => 2,
            Self::Env => 3,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Failed => "fail",
            Self::Config => "config-invalid",
            Self::Env => "environment",
        }
    }
}

/// Remove a report an earlier gate left, so a gate that dies early never
/// leaves a green one behind.
pub(crate) fn clear(report_dir: &Path) {
    fs::remove_file(report_dir.join(REPORT)).ok();
}

/// Name the layers the run leaves out.
pub(crate) fn announce_skipped(plan: &GatePlan<'_>) {
    if !plan.above.is_empty() {
        println!(
            "above --max-tier {}, not judged: {}",
            plan.max_tier,
            plan.above.join(", ")
        );
    }
    if !plan.disabled.is_empty() {
        println!("not run (enabled: false): {}", plan.disabled.join(", "));
    }
}

/// Print the summary, write the report and return the exit code.
pub(crate) fn finish(
    dir: &Path,
    cfg: &TopsConfig,
    opts: Options,
    plan: &GatePlan<'_>,
    done: &Executed<'_>,
    judged: &Judgement,
) -> ExitCode {
    print_summary(opts, done, judged);
    let kind = if judged.failed.is_empty() {
        Kind::Pass
    } else if judged.environment {
        Kind::Env
    } else {
        Kind::Failed
    };
    emit(&Parts {
        dir,
        cfg: Some(cfg),
        opts,
        kind,
        error: None,
        plan: Some(plan),
        done: Some(done),
        judged: Some(judged),
        report_dir: plan.report.clone(),
    })
}

fn print_summary(opts: Options, done: &Executed<'_>, judged: &Judgement) {
    println!();
    println!("{}", judged.evaluated);
    println!("{}", judged.count);
    if opts.mode == Mode::DryRun {
        if done.not_run.is_empty() {
            println!(
                "dry-run: {} command(s) listed, 0 run; a dry run is not a pass",
                done.listed
            );
        } else {
            println!(
                "dry-run: {} command(s) listed, 0 run; {} layer(s) NotRun, so the gate would fail",
                done.listed,
                done.not_run.len()
            );
        }
        for problem in &judged.failed {
            println!("  - {problem}");
        }
    } else if judged.failed.is_empty() {
        if judged.vacuous == 0 {
            println!("ok: {} command(s) passed", judged.ran);
        } else {
            println!(
                "ok: {} command(s) passed, {} vacuous (counted for no law)",
                judged.ran - judged.vacuous,
                judged.vacuous
            );
        }
    } else {
        println!(
            "fail: {} problem(s), {} command(s) run",
            judged.failed.len(),
            judged.ran
        );
        for problem in &judged.failed {
            println!("  - {problem}");
        }
    }
}

/// A gate that stopped before it reached a verdict.
pub(crate) fn fault(
    dir: &Path,
    cfg: Option<&TopsConfig>,
    opts: Options,
    plan: Option<&GatePlan<'_>>,
    fault: &Fault,
) -> ExitCode {
    if !fault.announced {
        eprintln!("error: {:#}", fault.error);
    }
    emit(&Parts {
        dir,
        cfg,
        opts,
        kind: if fault.config {
            Kind::Config
        } else {
            Kind::Env
        },
        error: Some(format!("{:#}", fault.error)),
        plan,
        done: None,
        judged: None,
        report_dir: report_dir(dir, plan),
    })
}

/// A config that did not load or failed `check`. With no directory there is
/// no target to report into.
pub(crate) fn config_invalid(dir: &Path, error: &anyhow::Error, opts: Options) -> ExitCode {
    if !dir.is_dir() {
        return ExitCode::from(Kind::Config.code());
    }
    emit(&Parts {
        dir,
        cfg: None,
        opts,
        kind: Kind::Config,
        error: Some(format!("{error:#}")),
        plan: None,
        done: None,
        judged: None,
        report_dir: report_dir(dir, None),
    })
}

fn report_dir(dir: &Path, plan: Option<&GatePlan<'_>>) -> PathBuf {
    plan.map_or_else(
        || std::path::absolute(laws::artifact_dir(dir)).unwrap_or_else(|_| laws::artifact_dir(dir)),
        |plan| plan.report.clone(),
    )
}

/// Everything the report is built from.
struct Parts<'a> {
    dir: &'a Path,
    cfg: Option<&'a TopsConfig>,
    opts: Options,
    kind: Kind,
    error: Option<String>,
    plan: Option<&'a GatePlan<'a>>,
    done: Option<&'a Executed<'a>>,
    judged: Option<&'a Judgement>,
    report_dir: PathBuf,
}

/// Write the report. A report that cannot be written is an environment
/// fault, unless the config fault is already the exit code.
fn emit(parts: &Parts<'_>) -> ExitCode {
    let mut kind = parts.kind;
    let path = parts.report_dir.join(REPORT);
    let wrote = fs::create_dir_all(&parts.report_dir).and_then(|()| {
        let text =
            serde_json::to_string_pretty(&document(parts, kind)).map_err(std::io::Error::other)?;
        fs::write(&path, format!("{text}\n"))
    });
    if let Err(err) = wrote {
        eprintln!("error: write {}: {err}", path.display());
        if matches!(kind, Kind::Pass | Kind::Failed) {
            kind = Kind::Env;
        }
    }
    ExitCode::from(kind.code())
}

fn git_sha(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()?;
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !sha.is_empty()).then_some(sha)
}

fn crate_name(parts: &Parts<'_>) -> String {
    parts.cfg.map_or_else(
        || {
            parts
                .dir
                .canonicalize()
                .ok()
                .and_then(|abs| abs.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "unknown".to_string())
        },
        |cfg| cfg.crate_.name.clone(),
    )
}

fn document(parts: &Parts<'_>, kind: Kind) -> Value {
    let (layers, disabled) = layer_rows(parts);
    let (laws, checked, total) = law_rows(parts);
    let problems: Vec<String> = parts.judged.map_or_else(
        || parts.error.iter().cloned().collect(),
        |judged| judged.failed.clone(),
    );
    let mut doc = Map::new();
    doc.insert("protocol_version".into(), json!(PROTOCOL_VERSION));
    doc.insert("crate".into(), json!(crate_name(parts)));
    if let Some(cfg) = parts.cfg {
        doc.insert("class".into(), json!(cfg.crate_.class));
    }
    if let Some(sha) = git_sha(parts.dir) {
        doc.insert("git_sha".into(), json!(sha));
    }
    doc.insert("max_tier".into(), json!(parts.opts.max_tier));
    doc.insert("dry_run".into(), json!(parts.opts.mode == Mode::DryRun));
    doc.insert("kind".into(), json!(kind.name()));
    doc.insert("exit_code".into(), json!(kind.code()));
    doc.insert("ok".into(), json!(kind == Kind::Pass));
    if let Some(error) = &parts.error {
        doc.insert("error".into(), json!(error));
    }
    doc.insert("problems".into(), json!(problems));
    doc.insert(
        "count".into(),
        json!(parts.judged.map(|j| j.count.as_str())),
    );
    doc.insert("layers".into(), json!(layers));
    doc.insert("disabled".into(), json!(disabled));
    doc.insert("laws_checked".into(), json!(checked));
    if let Some(total) = total {
        doc.insert("laws_total".into(), json!(total));
    }
    doc.insert("laws".into(), json!(laws));
    Value::Object(doc)
}

fn row(
    id: &str,
    tier: u8,
    verdict: &str,
    argv: &[Vec<String>],
    seconds: f64,
    tool_version: Option<&str>,
) -> Map<String, Value> {
    let mut row = Map::new();
    row.insert("id".into(), json!(id));
    row.insert("tier".into(), json!(tier));
    row.insert("verdict".into(), json!(verdict));
    row.insert("argv".into(), json!(argv));
    row.insert("tool_version".into(), json!(tool_version));
    row.insert("seconds".into(), json!(seconds));
    row
}

/// One row per enabled layer, in `rust-tops.yaml` order, and the ids of the
/// layers that are off.
fn layer_rows(parts: &Parts<'_>) -> (Vec<Value>, Vec<String>) {
    let mut rows = Vec::new();
    let mut disabled = Vec::new();
    let Some(cfg) = parts.cfg else {
        return (rows, disabled);
    };
    for layer in &cfg.layers {
        if !layer.enabled {
            disabled.push(layer.id.clone());
            continue;
        }
        let tier = crate::tiers::tier_for(layer);
        let mut entry = if tier > parts.opts.max_tier {
            let mut entry = row(&layer.id, tier, "not-evaluated", &[], 0.0, None);
            entry.insert(
                "detail".into(),
                json!(format!("above --max-tier {}", parts.opts.max_tier)),
            );
            entry
        } else if layer.id == "laws" {
            laws_row(parts, tier)
        } else {
            unit_row(parts, &layer.id, tier)
        };
        if let Some(Some(What::Shared(from))) = parts.plan.map(|plan| {
            plan.units
                .iter()
                .find(|unit| unit.layer.id == layer.id)
                .map(|unit| &unit.what)
        }) {
            entry.insert("took_from".into(), json!(from));
        }
        rows.push(Value::Object(entry));
    }
    (rows, disabled)
}

/// The verdict of a layer's runs: a failure outranks a missing tool, which
/// outranks a vacuous run.
fn runs_verdict(runs: &[&Run]) -> &'static str {
    let any = |outcome: Outcome| runs.iter().any(|run| run.outcome == outcome);
    if any(Outcome::Failed) {
        "fail"
    } else if any(Outcome::ToolMissing) {
        "tool-missing"
    } else if any(Outcome::Vacuous) {
        "vacuous"
    } else {
        "pass"
    }
}

/// The numbers a layer's floors were judged on, and whether any floor missed.
fn judged_numbers(parts: &Parts<'_>, id: &str) -> (Option<Value>, bool) {
    let Some(measured) = parts.judged.map(|judged| &judged.measured) else {
        return (None, false);
    };
    match id {
        "coverage" => (
            measured.coverage.as_ref().map(CoverageNumbers::to_json),
            measured.coverage_problems > 0,
        ),
        "crap" => (
            measured.crap.as_ref().map(CrapNumbers::to_json),
            measured.crap_problems > 0,
        ),
        _ => (None, false),
    }
}

/// `not-run` when nothing ran for the layer; otherwise its worst run.
fn unit_verdict(done: &Executed<'_>, id: &str) -> &'static str {
    let runs: Vec<&Run> = done.runs.iter().filter(|run| run.layer == id).collect();
    if runs.is_empty() {
        return "not-run";
    }
    runs_verdict(&runs)
}

fn unit_row(parts: &Parts<'_>, id: &str, tier: u8) -> Map<String, Value> {
    let (Some(plan), Some(done)) = (parts.plan, parts.done) else {
        return row(id, tier, "not-run", &[], 0.0, None);
    };
    let (numbers, missed) = judged_numbers(parts, id);
    let verdict = match unit_verdict(done, id) {
        // The run passed and its floors did not.
        "pass" if missed => "fail",
        verdict => verdict,
    };
    let version = done.tool_version(id);
    let mut entry = row(
        id,
        tier,
        verdict,
        &plan.argv_of(id),
        done.seconds(id),
        version.as_deref(),
    );
    let unmapped = plan
        .units
        .iter()
        .any(|unit| unit.layer.id == id && matches!(unit.what, What::NotRun));
    if unmapped {
        entry.insert("detail".into(), json!("enabled and no command is mapped"));
    }
    if let Some(numbers) = numbers {
        entry.insert("judged".into(), numbers);
    }
    entry
}

fn laws_row(parts: &Parts<'_>, tier: u8) -> Map<String, Value> {
    let argv = vec![vec![
        "bend".to_string(),
        parts.report_dir.join("cases.bend").display().to_string(),
    ]];
    let Some(judged) = parts.judged.filter(|judged| judged.bend_ran) else {
        return row("laws", tier, "not-run", &argv, 0.0, None);
    };
    let verdict = if judged.laws_failed { "fail" } else { "pass" };
    let cases = read_cases(&parts.report_dir);
    let version = cases.as_ref().and_then(|cases| cases.get("bend")?.as_str());
    row("laws", tier, verdict, &argv, judged.laws_seconds, version)
}

fn read_cases(report_dir: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(report_dir.join("cases.json")).ok()?).ok()
}

/// The laws as `cases.json` has them, how many were checked, and the total.
/// Without a `cases.json` from this gate there are none to show.
fn law_rows(parts: &Parts<'_>) -> (Vec<Value>, usize, Option<usize>) {
    let total = parts.judged.map(|judged| judged.law_total);
    let ran = parts.judged.is_some_and(|judged| judged.bend_ran);
    let cases = ran.then(|| read_cases(&parts.report_dir)).flatten();
    let Some(list) = cases
        .as_ref()
        .and_then(|cases| cases.get("laws"))
        .and_then(Value::as_array)
    else {
        return (Vec::new(), 0, total);
    };
    let rows: Vec<Value> = list
        .iter()
        .map(|law| {
            json!({
                "law": law.get("law"),
                "status": law.get("status"),
                "verdict": law.get("verdict"),
                "reason_code": law.get("reason_code"),
                "reason": law.get("reason"),
            })
        })
        .collect();
    let checked = list
        .iter()
        .filter(|law| law.get("status").and_then(Value::as_str) == Some("checked"))
        .count();
    (rows, checked, Some(list.len()))
}

#[cfg(test)]
mod tests {
    use super::{clear, REPORT};
    use std::fs;

    #[test]
    fn a_report_an_earlier_gate_left_is_cleared() {
        let dir = std::env::temp_dir().join(format!("cargo-tops-clear-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("mkdir");
        fs::write(dir.join(REPORT), "{}").expect("write");
        clear(&dir);
        assert!(!dir.join(REPORT).exists());
        fs::remove_dir_all(&dir).ok();
    }
}
