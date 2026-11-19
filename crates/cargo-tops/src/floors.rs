//! The coverage floors and the CRAP ratchet, judged on the reports this
//! gate's coverage and crap layers wrote (§4, §7.3, §16).
//!
//! `gate` calls [`judge`] after every layer has run. A layer that did not
//! pass wrote nothing to judge and has already failed the gate, so only a
//! passed coverage run and a passed crap run are read. Every miss is one
//! problem line, and any problem fails the gate. `cargo tops crap-baseline`
//! calls [`record_baseline`].
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::config::{CoverageGate, TopsConfig};
use crate::coverage_floor;
use crate::crap_ratchet::{self, Score};
use crate::exemptions::{self, Exemption};
use crate::facts::{Outcome, Run};
use crate::laws;
use crate::scope::{crate_root, Scope};
use crate::steps;

/// The default baseline, in the Cargo target directory (§16).
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
            "line_pct": self.line_pct,
            "region_pct": self.region_pct,
            "line_floor": self.line_floor,
            "region_floor": self.region_floor,
            "per_file_line_floor": self.per_file_line_floor,
            "files": self.files,
        })
    }
}

/// The CRAP numbers a gate judged.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CrapNumbers {
    pub(crate) threshold: f64,
    /// The highest production score; `None` with no functions.
    pub(crate) max: Option<f64>,
    pub(crate) functions: usize,
    /// Functions over the threshold or the ratchet, exemptions aside.
    pub(crate) over: usize,
    pub(crate) baseline: bool,
}

impl CrapNumbers {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "threshold": self.threshold,
            "max": self.max,
            "functions": self.functions,
            "over": self.over,
            "baseline": self.baseline,
        })
    }
}

/// What [`judge`] found: every problem, and the numbers behind them.
#[derive(Debug, Default)]
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
    let passed = |layer: &str| {
        runs.iter()
            .any(|run| run.layer == layer && run.outcome == Outcome::Passed)
    };
    let (coverage, crap) = (passed("coverage"), passed("crap"));
    if !coverage && !crap {
        return Judged::default();
    }
    let globs = cfg.gates.coverage.exclude_globs.clone().unwrap_or_default();
    let scope = Scope::new(dir, &crate_root(dir, &cfg.crate_.name), &globs);
    let today = exemptions::today();
    let mut judged = Judged::default();
    if coverage {
        let (found, numbers) = judge_coverage(cfg, &scope, report, &today);
        judged.coverage_problems = found.len();
        judged.problems.extend(found);
        judged.coverage = Some(numbers);
    }
    if crap {
        let (found, numbers) = judge_crap(dir, cfg, &scope, report, &today);
        judged.crap_problems = found.len();
        judged.problems.extend(found);
        judged.crap = numbers;
    }
    judged
}

fn judge_coverage(
    cfg: &TopsConfig,
    scope: &Scope,
    report: &Path,
    today: &str,
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
    if !floors_set(gate) {
        println!("    no floor is set, so there is nothing to judge");
        return (Vec::new(), numbers);
    }
    let (live, mut problems) = split(cfg, "coverage", today);
    let summary = report.join(steps::SUMMARY);
    match read(&summary).and_then(|raw| coverage_floor::production_files(&raw, scope)) {
        Ok(files) => {
            (numbers.line_pct, numbers.region_pct) = coverage_floor::percents(&files);
            numbers.files = files.len();
            problems.extend(coverage_floor::judge(&files, gate, &live));
        }
        Err(err) => problems.push(format!("coverage: {}: {err}", summary.display())),
    }
    tally(&problems);
    (problems, numbers)
}

fn floors_set(gate: &CoverageGate) -> bool {
    [
        gate.line_min_pct,
        gate.region_min_pct,
        gate.per_file_line_min_pct,
    ]
    .iter()
    .any(Option::is_some)
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
) -> (Vec<String>, Option<CrapNumbers>) {
    let gate = &cfg.gates.crap;
    let Some(threshold) = gate.threshold_agent.or(gate.threshold_production) else {
        println!(
            "\n==> [crap] no threshold_agent or threshold_production: not judged here \
             (a workspace root keeps its packages' floors)"
        );
        return (Vec::new(), None);
    };
    let baseline = baseline_path(dir, cfg, report);
    println!(
        "\n==> [crap] threshold {threshold} on new functions in {}; ratchet against {}",
        scope.root().display(),
        baseline.display()
    );
    let (live, mut problems) = split(cfg, "crap", today);
    let crap = report.join(steps::CRAP);
    let scores = read(&crap)
        .and_then(|raw| crap_ratchet::production_scores(&raw, scope))
        .map_err(|err| format!("{}: {err}", crap.display()));
    let mut numbers = CrapNumbers {
        threshold,
        ..CrapNumbers::default()
    };
    match scores.and_then(|current| Ok((current, read_baseline(&baseline, scope)?))) {
        Ok((current, before)) => {
            announce(before.as_deref());
            let over = crap_ratchet::judge(&current, before.as_deref(), threshold, &live);
            numbers.max = current.iter().map(|score| score.crap).reduce(f64::max);
            numbers.functions = current.len();
            numbers.over = over.len();
            numbers.baseline = before.is_some();
            problems.extend(over);
        }
        Err(err) => problems.push(format!("crap: {err}")),
    }
    tally(&problems);
    (problems, Some(numbers))
}

/// Without a baseline every function is judged as new; say how to make one.
fn announce(baseline: Option<&[Score]>) {
    if baseline.is_none() {
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

/// The baseline's production scores. A missing file is `None`; one that
/// cannot be read or is not a report is an error, never "no baseline".
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

/// `cargo tops crap-baseline`: the last gate's CRAP report, sorted, written
/// to the instance's baseline path.
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
