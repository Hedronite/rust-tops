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
use super::judge::{self, Judgement};
use super::plan::{GatePlan, What};
use super::{Fault, Mode, Options};
use crate::config::TopsConfig;
use crate::floors::{CoverageNumbers, CrapNumbers};
use crate::steps::{self, Base};
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
    let kind = if judged.failed.is_empty() {
        Kind::Pass
    } else if judged.environment {
        Kind::Env
    } else {
        Kind::Failed
    };
    // The report is written before the summary, so the summary's last line
    // is true: a report that could not be written ends it with `fail:`.
    let (code, unwritten) = write_report(&Parts {
        dir,
        cfg: Some(cfg),
        opts,
        kind,
        error: None,
        plan: Some(plan),
        done: Some(done),
        judged: Some(judged),
        report_dir: plan.report.clone(),
    });
    print_summary(opts, done, judged, unwritten.as_deref());
    code
}

/// `unwritten` is the path of a report that could not be written.
fn print_summary(opts: Options, done: &Executed<'_>, judged: &Judgement, unwritten: Option<&Path>) {
    println!();
    println!("{}", judged.evaluated);
    println!("{}", judged.count);
    if opts.mode == Mode::DryRun {
        print_dry_run(done, judged);
    } else if judged.failed.is_empty() {
        print_pass(judged, unwritten);
    } else {
        println!(
            "fail: {} problem(s), {} command(s) run",
            judged.failed.len(),
            judged.commands
        );
        print_problems(judged);
    }
}

fn print_problems(judged: &Judgement) {
    for problem in &judged.failed {
        println!("  - {problem}");
    }
}

fn print_dry_run(done: &Executed<'_>, judged: &Judgement) {
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
    print_problems(judged);
}

/// Formats the `ok:` summary line so `passed + vacuous == ran_commands`.
pub(crate) fn ok_line(judged: &Judgement) -> String {
    if judged.vacuous == 0 {
        format!("ok: {} command(s) passed", judged.commands)
    } else {
        format!(
            "ok: {} command(s) passed, {} vacuous (counted for no law)",
            judged.commands.saturating_sub(judged.vacuous),
            judged.vacuous
        )
    }
}

/// No problem: `ok`, unless the report that says so could not be written.
fn print_pass(judged: &Judgement, unwritten: Option<&Path>) {
    if let Some(path) = unwritten {
        println!(
            "fail: {} could not be written; the gate has no verdict on record",
            path.display()
        );
    } else {
        println!("{}", ok_line(judged));
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

/// Write the report and return the exit code.
fn emit(parts: &Parts<'_>) -> ExitCode {
    write_report(parts).0
}

/// Write the report. A report that cannot be written is an environment
/// fault, unless the config fault is already the exit code; its path is
/// returned so the summary can say so.
fn write_report(parts: &Parts<'_>) -> (ExitCode, Option<PathBuf>) {
    let mut kind = parts.kind;
    let path = parts.report_dir.join(REPORT);
    let wrote = fs::create_dir_all(&parts.report_dir).and_then(|()| {
        let text =
            serde_json::to_string_pretty(&document(parts, kind)).map_err(std::io::Error::other)?;
        fs::write(&path, format!("{text}\n"))
    });
    let mut unwritten = None;
    if let Err(err) = wrote {
        eprintln!("error: write {}: {err}", path.display());
        if matches!(kind, Kind::Pass | Kind::Failed) {
            kind = Kind::Env;
        }
        unwritten = Some(path);
    }
    (ExitCode::from(kind.code()), unwritten)
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

/// A layer enabled above `--max-tier`: named, not judged.
fn above_range_row(parts: &Parts<'_>, id: &str, tier: u8) -> Map<String, Value> {
    let mut entry = row(id, tier, judge::above_range_verdict(), &[], 0.0, None);
    entry.insert(
        "detail".into(),
        json!(format!("above --max-tier {}", parts.opts.max_tier)),
    );
    entry
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
            above_range_row(parts, &layer.id, tier)
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

/// The numbers a layer's floors, or its survivors, were judged on.
fn judged_numbers(parts: &Parts<'_>, id: &str) -> Option<Value> {
    let judged = parts.judged?;
    match id {
        "coverage" => judged
            .measured
            .coverage
            .as_ref()
            .map(CoverageNumbers::to_json),
        "crap" => judged.measured.crap.as_ref().map(CrapNumbers::to_json),
        "mutation" => judged
            .survivors
            .as_ref()
            .map(|found| found.ledger.to_json(found.max)),
        _ => None,
    }
}

fn unit_row(parts: &Parts<'_>, id: &str, tier: u8) -> Map<String, Value> {
    let (Some(plan), Some(done)) = (parts.plan, parts.done) else {
        return row(id, tier, judge::unreached_verdict(), &[], 0.0, None);
    };
    let numbers = judged_numbers(parts, id);
    let verdict = judge::layer_verdict(done, parts.judged, id);
    let version = done.tool_version(id);
    let mut entry = row(
        id,
        tier,
        verdict,
        &plan.argv_of(id),
        done.seconds(id),
        version.as_deref(),
    );
    if let Some(numbers) = numbers {
        entry.insert("judged".into(), numbers);
    }
    if let Some(detail) = layer_detail(plan, done, id) {
        entry.insert("detail".into(), json!(detail));
    }
    if id == "mutation" {
        name_the_base(plan, done, &mut entry);
    }
    entry
}

/// The mutation row names the base the diff started from and where it came
/// from (`flag`, `key` or `origin/HEAD`), under `judged.base`. When the
/// layer stopped before `cargo mutants`, `detail` says why, in the words the
/// run printed. A dry run has no run, so a base that is missing or not a
/// commit is worded by [`steps::stop_note`] from the plan alone.
fn name_the_base(plan: &GatePlan<'_>, done: &Executed<'_>, entry: &mut Map<String, Value>) {
    let Some(What::Mutation(mutation)) = plan
        .units
        .iter()
        .find(|unit| unit.layer.id == "mutation")
        .map(|unit| &unit.what)
    else {
        return;
    };
    let base = match &mutation.base {
        Base::Found { source, name } => json!({ "ref": name, "source": source.report_name() }),
        Base::Unresolved { source, name } => {
            json!({ "ref": name, "source": source.report_name(), "resolved": false })
        }
        Base::Missing { tried, .. } => json!({ "ref": null, "source": null, "tried": tried }),
    };
    put_judged(entry, "base", base);
    // A run that stopped says why in its own words. A row with no such note
    // (a missing tool, a dry run) names the base's fault and no outcome.
    if let Some(detail) = run_note(done, "mutation").or_else(|| steps::base_finding(&mutation.base))
    {
        entry.insert("detail".into(), json!(detail));
    }
}

/// Why a layer has no plain pass or fail: no command is mapped for it, or
/// its run kept a note (the credit evidence of `property` and `acceptance`).
fn layer_detail(plan: &GatePlan<'_>, done: &Executed<'_>, id: &str) -> Option<String> {
    let unmapped = plan
        .units
        .iter()
        .any(|unit| unit.layer.id == id && matches!(unit.what, What::NotRun));
    if unmapped {
        return Some("enabled and no command is mapped".to_string());
    }
    run_note(done, id)
}

/// The note `layer`'s run kept: why it stopped, in the words the report and
/// the failure line both use.
fn run_note(done: &Executed<'_>, layer: &str) -> Option<String> {
    done.runs
        .iter()
        .find(|run| run.layer == layer)
        .and_then(|run| run.note.clone())
}

/// Set `key` in the row's `judged` object, keeping what else is there.
fn put_judged(entry: &mut Map<String, Value>, key: &str, value: Value) {
    let judged = entry
        .entry("judged".to_string())
        .or_insert_with(|| json!({}));
    if let Some(map) = judged.as_object_mut() {
        map.insert(key.to_string(), value);
    }
}

fn laws_row(parts: &Parts<'_>, tier: u8) -> Map<String, Value> {
    let argv = vec![vec![
        "bend".to_string(),
        parts.report_dir.join("cases.bend").display().to_string(),
    ]];
    let verdict = judge::laws_verdict(parts.judged);
    let Some(judged) = parts.judged.filter(|judged| judged.bend_ran) else {
        return row("laws", tier, verdict, &argv, 0.0, None);
    };
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
    use super::super::judge::Judgement;
    use super::super::tests::Scratch;
    use super::{clear, ok_line, put_judged, REPORT};
    use crate::floors;
    use serde_json::{json, Map};
    use std::fs;

    fn judged_with(commands: usize, vacuous: usize) -> Judgement {
        Judgement {
            failed: Vec::new(),
            evaluated: String::new(),
            count: String::new(),
            commands,
            vacuous,
            measured: floors::Judged::default(),
            laws_seconds: 0.0,
            bend_ran: true,
            laws_failed: false,
            environment: false,
            law_total: 0,
            survivors: None,
        }
    }

    #[test]
    fn ok_line_agrees_with_ran_commands_when_laws_ran() {
        assert_eq!(ok_line(&judged_with(9, 0)), "ok: 9 command(s) passed");
        assert_eq!(
            ok_line(&judged_with(9, 1)),
            "ok: 8 command(s) passed, 1 vacuous (counted for no law)"
        );
    }

    #[test]
    fn a_report_an_earlier_gate_left_is_cleared() {
        let scratch = Scratch::new("clear");
        fs::write(scratch.0.join(REPORT), "{}").expect("write");
        clear(&scratch.0);
        assert!(!scratch.0.join(REPORT).exists());
    }

    #[test]
    fn a_judged_key_is_added_beside_the_ones_already_there() {
        let mut row = Map::new();
        put_judged(&mut row, "base", json!({ "ref": "x" }));
        assert_eq!(row.get("judged"), Some(&json!({ "base": { "ref": "x" } })));
        put_judged(&mut row, "caught", json!(3));
        assert_eq!(
            row.get("judged"),
            Some(&json!({ "base": { "ref": "x" }, "caught": 3 }))
        );
    }
}
