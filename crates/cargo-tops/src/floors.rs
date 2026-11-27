//! The coverage floors and the CRAP threshold/ratchet, judged on the reports
//! this gate's coverage and crap layers wrote (§2.3, §4, §7.3, §16).
//!
//! `gate` calls [`judge`] after every layer has run. A layer that did not
//! pass wrote nothing to judge and has already failed the gate, so only a
//! passed coverage run and a passed crap run are read. Every miss is one
//! problem line, and any problem fails the gate.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::baseline::{self, BaselineV2};
use crate::config::{Enforce, TopsConfig};
use crate::coverage_floor::{self, CoverageLedger};
use crate::crap_ratchet::{self, Score};
use crate::exemptions::{self, Exemption};
use crate::facts::{Outcome, Run};
use crate::laws;
use crate::scope::{crate_root, Scope};
use crate::steps;

/// The default legacy v1 baseline, in the Cargo target directory (§16).
pub(crate) const BASELINE: &str = "crap-baseline.json";

/// The coverage numbers a gate judged: the floors and what the report held.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CoverageNumbers {
    pub(crate) line_pct: Option<f64>,
    pub(crate) region_pct: Option<f64>,
    pub(crate) line_floor: Option<f64>,
    pub(crate) region_floor: Option<f64>,
    pub(crate) per_file_line_floor: Option<f64>,
    /// Production files of the crate in the report.
    pub(crate) files: usize,
}

impl CoverageNumbers {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "files": self.files,
            "line_pct": self.line_pct,
            "region_pct": self.region_pct,
            "line_floor": self.line_floor,
            "region_floor": self.region_floor,
            "per_file_line_floor": self.per_file_line_floor,
        })
    }
}

/// The CRAP numbers a gate judged: the threshold and what the report held.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CrapNumbers {
    pub(crate) threshold: f64,
    pub(crate) max: Option<f64>,
    /// Production functions of the crate in the report.
    pub(crate) functions: usize,
    /// Functions that failed the threshold or the ratchet.
    pub(crate) over: usize,
    pub(crate) baseline: bool,
}

impl CrapNumbers {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "threshold": self.threshold,
            "functions": self.functions,
            "max": self.max,
            "over": self.over,
            "baseline": self.baseline,
        })
    }
}

/// Everything [`judge`] found: the problems and the numbers that failed or
/// passed them.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Judged {
    pub(crate) problems: Vec<String>,
    pub(crate) coverage: Option<CoverageNumbers>,
    pub(crate) coverage_problems: usize,
    pub(crate) crap: Option<CrapNumbers>,
    pub(crate) crap_problems: usize,
}

/// Every coverage floor and CRAP rule this gate's reports miss. `report`
/// is the gate's artifact directory.
pub(crate) fn judge(dir: &Path, cfg: &TopsConfig, runs: &[Run], report: &Path) -> Judged {
    let coverage = layer_passed(runs, "coverage");
    let crap = layer_passed(runs, "crap");
    if !coverage && !crap {
        return Judged::default();
    }
    let globs = cfg.gates.coverage.exclude_globs.clone().unwrap_or_default();
    let scope = Scope::new(dir, &crate_root(dir, &cfg.crate_.name), &globs);
    let today = exemptions::today();
    let ledger = load_v2(dir, cfg);
    let mut judged = Judged::default();
    attach_ledger_faults(&mut judged, dir, cfg, &ledger, runs);
    if coverage {
        let (found, numbers) = judge_coverage(cfg, &scope, report, &today, &ledger);
        judged.coverage_problems += found.len();
        judged.problems.extend(found);
        judged.coverage = Some(numbers);
    }
    if crap {
        let (found, numbers) = judge_crap(dir, cfg, &scope, report, &today, &ledger);
        judged.crap_problems += found.len();
        judged.problems.extend(found);
        judged.crap = numbers;
    }
    judged
}

fn attach_ledger_faults(
    judged: &mut Judged,
    dir: &Path,
    cfg: &TopsConfig,
    ledger: &LedgerLoad,
    runs: &[Run],
) {
    let coverage = layer_passed(runs, "coverage");
    let crap = layer_passed(runs, "crap");
    match ledger {
        LedgerLoad::Invalid(err) => {
            if coverage {
                judged.problems.push(format!("coverage: {err}"));
                judged.coverage_problems += 1;
            }
            if crap {
                judged.problems.push(format!("crap: {err}"));
                judged.crap_problems += 1;
            }
        }
        LedgerLoad::Absent if cfg.enforce == Some(Enforce::Ratchet) => {
            let msg = format!(
                "missing baseline {}; run `cargo tops baseline --init` (PROTOCOL.md §2.3, §16.2)",
                baseline::path(dir, cfg).display()
            );
            if coverage && coverage_floor::floors_set(&cfg.gates.coverage) {
                judged.problems.push(format!("coverage: {msg}"));
                judged.coverage_problems += 1;
            }
            if crap && cfg.gates.crap.cap().is_some() {
                judged.problems.push(format!("crap: {msg}"));
                judged.crap_problems += 1;
            }
        }
        LedgerLoad::Off | LedgerLoad::Present(_) | LedgerLoad::Absent => {}
    }
}

fn layer_passed(runs: &[Run], layer: &str) -> bool {
    runs.iter()
        .any(|run| run.layer == layer && run.outcome == Outcome::Passed)
}

fn judge_coverage(
    cfg: &TopsConfig,
    scope: &Scope,
    report: &Path,
    today: &str,
    ledger: &LedgerLoad,
) -> (Vec<String>, CoverageNumbers) {
    let gate = &cfg.gates.coverage;
    let mut numbers = CoverageNumbers {
        line_floor: gate.line_min_pct,
        region_floor: gate.region_min_pct,
        per_file_line_floor: gate.per_file_line_min_pct,
        ..CoverageNumbers::default()
    };
    println!(
        "\n==> [coverage] floors for {}: line {}, region {}, per-file line {}",
        scope.root().display(),
        floor(gate.line_min_pct),
        floor(gate.region_min_pct),
        floor(gate.per_file_line_min_pct)
    );
    if !coverage_floor::floors_set(gate) {
        println!("    no floor is set, so there is nothing to judge");
        return (Vec::new(), numbers);
    }
    let (live, mut problems) = split(cfg, "coverage", today);
    let summary = report.join(steps::SUMMARY);
    match read(&summary).and_then(|raw| coverage_floor::production_files(&raw, scope)) {
        Ok(files) => {
            (numbers.line_pct, numbers.region_pct) = coverage_floor::percents(&files);
            numbers.files = files.len();
            if ledger.ready(cfg) {
                problems.extend(coverage_floor::judge(
                    &files,
                    gate,
                    &live,
                    coverage_ratchet(cfg, ledger),
                ));
            }
        }
        Err(err) => problems.push(format!("coverage: {}: {err}", summary.display())),
    }
    tally(&problems);
    (problems, numbers)
}

fn coverage_ratchet<'a>(cfg: &TopsConfig, ledger: &'a LedgerLoad) -> Option<&'a CoverageLedger> {
    match (cfg.enforce, ledger) {
        (Some(Enforce::Ratchet | Enforce::Advisory), LedgerLoad::Present(v2)) => {
            Some(v2.coverage.as_ref().unwrap_or(&coverage_floor::EMPTY))
        }
        _ => None,
    }
}

fn floor(pct: Option<f64>) -> String {
    pct.map_or_else(|| "not set".to_string(), |pct| format!(">= {pct}%"))
}

fn judge_crap(
    dir: &Path,
    cfg: &TopsConfig,
    scope: &Scope,
    report: &Path,
    today: &str,
    ledger: &LedgerLoad,
) -> (Vec<String>, Option<CrapNumbers>) {
    let Some(threshold) = cfg.gates.crap.cap() else {
        println!(
            "\n==> [crap] no threshold_agent or threshold_production: not judged here \
             (a workspace root keeps its packages' floors)"
        );
        return (Vec::new(), None);
    };
    announce_crap_header(dir, cfg, scope, report, threshold);
    let (live, mut problems) = split(cfg, "crap", today);
    let crap = report.join(steps::CRAP);
    let scores = read(&crap)
        .and_then(|raw| crap_ratchet::production_scores(&raw, scope))
        .map_err(|err| format!("{}: {err}", crap.display()));
    let mut numbers = CrapNumbers {
        threshold,
        ..CrapNumbers::default()
    };
    match scores {
        Ok(current) => {
            numbers.max = current.iter().map(|score| score.crap).reduce(f64::max);
            numbers.functions = current.len();
            if ledger.ready(cfg) {
                match crap_baseline(dir, cfg, scope, report, ledger) {
                    Ok(before) => {
                        announce(cfg.enforce, before.as_deref());
                        let over =
                            crap_ratchet::judge(&current, before.as_deref(), threshold, &live);
                        numbers.over = over.len();
                        numbers.baseline = before.is_some();
                        problems.extend(over);
                    }
                    Err(err) => problems.push(format!("crap: {err}")),
                }
            }
        }
        Err(err) => problems.push(format!("crap: {err}")),
    }
    tally(&problems);
    (problems, Some(numbers))
}

fn announce_crap_header(
    dir: &Path,
    cfg: &TopsConfig,
    scope: &Scope,
    report: &Path,
    threshold: f64,
) {
    match cfg.enforce {
        Some(Enforce::Hard) => println!(
            "\n==> [crap] threshold {threshold} in {} (enforce: hard; baseline disabled)",
            scope.root().display()
        ),
        Some(Enforce::Ratchet | Enforce::Advisory) => println!(
            "\n==> [crap] threshold {threshold} on new functions in {}; ratchet against {}",
            scope.root().display(),
            baseline::path(dir, cfg).display()
        ),
        None => println!(
            "\n==> [crap] threshold {threshold} on new functions in {}; ratchet against {}",
            scope.root().display(),
            baseline_path(dir, cfg, report).display()
        ),
    }
}

enum LedgerLoad {
    Off,
    Present(Box<BaselineV2>),
    Absent,
    Invalid(String),
}

impl LedgerLoad {
    /// Floor and cap judging run when the ledger is usable for this mode.
    fn ready(&self, cfg: &TopsConfig) -> bool {
        match self {
            Self::Off | Self::Present(_) => true,
            Self::Absent => cfg.enforce == Some(Enforce::Advisory),
            Self::Invalid(_) => false,
        }
    }
}

fn load_v2(dir: &Path, cfg: &TopsConfig) -> LedgerLoad {
    match cfg.enforce {
        Some(Enforce::Hard) | None => LedgerLoad::Off,
        Some(Enforce::Ratchet | Enforce::Advisory) => read_v2_mode(dir, cfg),
    }
}

fn read_v2_mode(dir: &Path, cfg: &TopsConfig) -> LedgerLoad {
    let path = baseline::path(dir, cfg);
    if !path.exists() {
        return LedgerLoad::Absent;
    }
    match baseline::read_v2(&path, cfg) {
        Ok(v2) => LedgerLoad::Present(Box::new(v2)),
        Err(err) => LedgerLoad::Invalid(err),
    }
}

fn crap_baseline(
    dir: &Path,
    cfg: &TopsConfig,
    scope: &Scope,
    report: &Path,
    ledger: &LedgerLoad,
) -> Result<Option<Vec<Score>>, String> {
    match (cfg.enforce, ledger) {
        (None, _) => read_baseline(&baseline_path(dir, cfg, report), scope),
        (Some(Enforce::Hard), _) | (_, LedgerLoad::Off | LedgerLoad::Invalid(_)) => Ok(None),
        (Some(Enforce::Ratchet | Enforce::Advisory), LedgerLoad::Present(v2)) => {
            Ok(Some(baseline::scores_from(v2.as_ref())))
        }
        (Some(Enforce::Ratchet | Enforce::Advisory), LedgerLoad::Absent) => Ok(None),
    }
}

/// Without a baseline under legacy 1.4.0 mode, say how to make one.
fn announce(enforce: Option<Enforce>, baseline: Option<&[Score]>) {
    if enforce.is_none() && baseline.is_none() {
        println!(
            "    no baseline: every function is judged as new. After a gate run, \
             `cargo tops crap-baseline` records one (PROTOCOL.md §16)"
        );
    }
}

/// `gates.crap.baseline` under `dir`, or [`BASELINE`] in the Cargo target
/// directory, the parent of the artifact directory `report`.
pub(crate) fn baseline_path(dir: &Path, cfg: &TopsConfig, report: &Path) -> PathBuf {
    cfg.gates.crap.baseline.as_deref().map_or_else(
        || report.parent().unwrap_or(report).join(BASELINE),
        |rel| dir.join(rel),
    )
}

/// The legacy v1 baseline's production scores. A missing file is `None`; one
/// that cannot be read or is not a report is an error, never "no baseline".
fn read_baseline(path: &Path, scope: &Scope) -> Result<Option<Vec<Score>>, String> {
    match fs::read_to_string(path) {
        Ok(raw) => crap_ratchet::production_scores(&raw, scope)
            .map(Some)
            .map_err(|err| format!("baseline {}: {err}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("baseline {}: {err}", path.display())),
    }
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| err.to_string())
}

/// The live `layer` exemptions, and one problem per record that expired.
fn split(cfg: &TopsConfig, layer: &str, today: &str) -> (Vec<Exemption>, Vec<String>) {
    let (live, expired): (Vec<Exemption>, Vec<Exemption>) =
        exemptions::for_layer(cfg.exemptions.as_ref(), layer)
            .into_iter()
            .partition(|record| record.live(today));
    let problems = expired
        .iter()
        .map(|record| record.expired_problem(layer))
        .collect();
    (live, problems)
}

fn tally(problems: &[String]) {
    if problems.is_empty() {
        println!("    ok");
    } else {
        println!("    {} problem(s), listed at the end", problems.len());
    }
}

/// `cargo tops crap-baseline` (legacy v1 compatibility alias): the last gate's
/// CRAP report, sorted, written to the legacy baseline path.
pub(crate) fn record_baseline(dir: &Path, cfg: &TopsConfig) -> Result<()> {
    laws::begin_gate();
    let report = std::path::absolute(laws::artifact_dir(dir))
        .with_context(|| format!("resolve the report dir of {}", dir.display()))?;
    let source = report.join(steps::CRAP);
    let raw = fs::read_to_string(&source).with_context(|| {
        format!(
            "read {}; run `cargo tops gate` with the crap layer first",
            source.display()
        )
    })?;
    let (text, count) =
        crap_ratchet::baseline_json(&raw).map_err(|err| anyhow!("{}: {err}", source.display()))?;
    let out = baseline_path(dir, cfg, &report);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&out, text).with_context(|| format!("write {}", out.display()))?;
    println!(
        "wrote {} ({count} functions from {})",
        out.display(),
        source.display()
    );
    Ok(())
}
