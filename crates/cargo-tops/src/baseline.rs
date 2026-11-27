//! Baseline v2 debt ledger (`rust-tops-baseline.json`) and `cargo tops baseline`
//! (PROTOCOL.md §16.1, §16.2).
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Enforce, TopsConfig};
use crate::coverage_floor::{self, CoverageLedger};
use crate::crap_ratchet::{self, Score, EPSILON};
use crate::exemptions;
use crate::laws;
use crate::schema;
use crate::scope::{crate_root, Scope};
use crate::steps;

pub(crate) const DEFAULT_BASELINE: &str = "rust-tops-baseline.json";
pub(crate) const BASELINE_SCHEMA_JSON: &str = include_str!("../templates/baseline.schema.json");
const REPORT_FILE: &str = "report.json";

/// Mode of `cargo tops baseline`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Init,
    Refresh,
    Check,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct Regression {
    pub kind: String,
    pub key: String,
    pub from: Option<f64>,
    pub to: f64,
    pub reason: String,
    pub owner: String,
    pub date: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct BaselineV2 {
    pub version: u64,
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub functions: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<CoverageLedger>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lints: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppressions: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exemptions: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regressions: Option<Vec<Regression>>,
}

/// Path to the v2 ledger for `cfg` under `dir`.
pub(crate) fn path(dir: &Path, cfg: &TopsConfig) -> PathBuf {
    dir.join(cfg.baseline.as_deref().unwrap_or(DEFAULT_BASELINE))
}

/// Executes `cargo tops baseline [--init | --check]`.
pub(crate) fn command(
    dir: &Path,
    cfg: &TopsConfig,
    mode: Mode,
    base: Option<&str>,
) -> Result<ExitCode> {
    laws::begin_gate();
    match mode {
        Mode::Init => run_init(dir, cfg),
        Mode::Refresh => run_refresh(dir, cfg),
        Mode::Check => run_check(dir, cfg, base),
    }
}

/// Loads and validates a v2 ledger.
pub(crate) fn read_v2(ledger: &Path, cfg: &TopsConfig) -> Result<BaselineV2, String> {
    let raw = fs::read_to_string(ledger)
        .map_err(|err| format!("baseline {}: {err}", ledger.display()))?;
    parse_and_validate(&raw, cfg).map_err(|err| format!("baseline {}: {err}", ledger.display()))
}

/// CRAP scores recorded in `v2.functions`.
pub(crate) fn scores_from(v2: &BaselineV2) -> Vec<Score> {
    v2.functions
        .iter()
        .filter_map(|(key, &crap)| {
            let (file, function) = key.split_once("::")?;
            Some(Score {
                file: file.to_string(),
                function: function.to_string(),
                crap,
            })
        })
        .collect()
}

/// Parses JSON, validates against `baseline.schema.json`, checks crate match,
/// forbids line numbers, and checks canonical one-entry-per-line sorted keys.
pub(crate) fn parse_and_validate(raw: &str, cfg: &TopsConfig) -> Result<BaselineV2, String> {
    let doc: Value = serde_json::from_str(raw).map_err(|err| format!("not valid JSON: {err}"))?;
    let schema_doc: Value = serde_json::from_str(BASELINE_SCHEMA_JSON)
        .map_err(|err| format!("bundled baseline.schema.json: {err}"))?;
    let errors = schema::validate(&schema_doc, &doc);
    if !errors.is_empty() {
        return Err(format!(
            "does not match schema/baseline.schema.json:\n  {}",
            errors.join("\n  ")
        ));
    }
    let v2: BaselineV2 =
        serde_json::from_value(doc).map_err(|err| format!("invalid baseline structure: {err}"))?;
    if v2.crate_name != cfg.crate_.name {
        return Err(format!(
            "crate {:?} does not match instance crate {:?}",
            v2.crate_name, cfg.crate_.name
        ));
    }
    for key in v2.functions.keys() {
        if has_line_number(key) {
            return Err(format!("key {key:?} contains a line number"));
        }
    }
    check_functions_layout(raw, &v2.functions)?;
    if let Some(coverage) = &v2.coverage {
        check_coverage_form(raw, coverage)?;
    }
    Ok(v2)
}

fn check_coverage_form(raw: &str, coverage: &CoverageLedger) -> Result<(), String> {
    if let Some(files) = &coverage.files {
        for key in files.keys() {
            if has_file_line_number(key) {
                return Err(format!("coverage.files key {key:?} contains a line number"));
            }
        }
        check_object_layout(raw, "files", files, Some("coverage"))?;
    }
    Ok(())
}

fn has_file_line_number(key: &str) -> bool {
    key.contains("#L") || has_colon_digits(key)
}

fn has_line_number(key: &str) -> bool {
    let Some((file, func)) = key.split_once("::") else {
        return true;
    };
    file.is_empty()
        || func.is_empty()
        || file.contains(':')
        || has_colon_digits(func)
        || key.contains("#L")
}

fn has_colon_digits(text: &str) -> bool {
    let bytes = text.as_bytes();
    text.match_indices(':').any(|(idx, _)| {
        let prev_colon = idx.checked_sub(1).and_then(|prev| bytes.get(prev)) == Some(&b':');
        let next_digit = bytes.get(idx + 1).is_some_and(u8::is_ascii_digit);
        !prev_colon && next_digit
    })
}

/// Checks that `functions` entries in `raw` appear one per line and in
/// strictly sorted key order.
fn check_functions_layout(raw: &str, expected: &BTreeMap<String, f64>) -> Result<(), String> {
    check_object_layout(raw, "functions", expected, None)
}

fn check_object_layout(
    raw: &str,
    field: &str,
    expected: &BTreeMap<String, f64>,
    under: Option<&str>,
) -> Result<(), String> {
    let body = object_body(raw, field, under).ok_or_else(|| match under {
        Some(parent) => format!("could not locate {parent}.{field} object"),
        None => format!("could not locate top-level {field} object"),
    })?;
    if !expected.is_empty() && !body.contains('\n') {
        return Err(format!("non-empty {field} must place one entry per line"));
    }
    let mut seen: Vec<String> = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim().trim_end_matches(',').trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = parse_named_entry_line(field, trimmed)?;
        seen.push(key);
    }
    let want: Vec<String> = expected.keys().cloned().collect();
    if seen != want {
        return Err(format!(
            "{field} keys must be sorted lexicographically, one entry per line"
        ));
    }
    Ok(())
}

fn object_body<'a>(raw: &'a str, field: &str, under: Option<&str>) -> Option<&'a str> {
    match under {
        Some(parent) => find_object_at_depth(find_top_object(raw, parent)?, field, 0),
        None => find_top_object(raw, field),
    }
}

fn parse_named_entry_line(field: &str, trimmed: &str) -> Result<String, String> {
    let Some((raw_key, raw_val)) = split_key_value(trimmed) else {
        return Err(format!(
            "{field} entry must be one `\"key\": number` per line: {trimmed}"
        ));
    };
    let key: String = serde_json::from_str(raw_key)
        .map_err(|err| format!("invalid {field} key {raw_key}: {err}"))?;
    let _: f64 = serde_json::from_str(raw_val.trim()).map_err(|_| {
        format!(
            "{field} entry must have one value per line (found extra content on line: {trimmed})"
        )
    })?;
    Ok(key)
}

#[derive(Default)]
struct StrScan {
    depth: usize,
    in_str: bool,
    escape: bool,
}

impl StrScan {
    fn step(&mut self, ch: char) -> bool {
        if self.in_str {
            self.step_in_str(ch);
            return false;
        }
        match ch {
            '"' => self.in_str = true,
            '{' => self.depth += 1,
            '}' => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
        true
    }

    fn step_in_str(&mut self, ch: char) {
        if self.escape {
            self.escape = false;
        } else {
            self.escape = ch == '\\';
            self.in_str = ch != '"';
        }
    }
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    if !line.starts_with('"') {
        return None;
    }
    let mut scan = StrScan {
        depth: 0,
        in_str: true,
        escape: false,
    };
    for (idx, ch) in line.char_indices().skip(1) {
        if !scan.step(ch) && !scan.in_str {
            let rest = line.get(idx + 1..)?.trim_start();
            return Some((line.get(..=idx)?, rest.strip_prefix(':')?));
        }
    }
    None
}

#[cfg(test)]
fn find_functions_body(raw: &str) -> Option<&str> {
    find_top_object(raw, "functions")
}

fn find_top_object<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    find_object_at_depth(raw, name, 1)
}

fn find_object_at_depth<'a>(raw: &'a str, name: &str, key_depth: usize) -> Option<&'a str> {
    let mut scan = StrScan::default();
    for (idx, ch) in raw.char_indices() {
        let was_depth = scan.depth;
        if scan.step(ch) && ch == '"' && was_depth == key_depth {
            if let Some(body) = match_object_at(raw, idx, name) {
                return Some(body);
            }
        }
    }
    None
}

fn match_object_at<'a>(raw: &'a str, idx: usize, name: &str) -> Option<&'a str> {
    let quoted = format!("\"{name}\"");
    let tail = raw.get(idx..)?.strip_prefix(quoted.as_str())?.trim_start();
    let rest = tail.strip_prefix(':')?.trim_start();
    let open = raw.len() - rest.len();
    extract_braced(raw, open)
}

fn extract_braced(raw: &str, open: usize) -> Option<&str> {
    let slice = raw.get(open..)?.strip_prefix('{')?;
    let mut scan = StrScan {
        depth: 1,
        ..StrScan::default()
    };
    for (rel, ch) in slice.char_indices() {
        if scan.step(ch) && scan.depth == 0 {
            return slice.get(..rel);
        }
    }
    None
}

fn aggregate_scores(crap_raw: &str, dir: &Path, cfg: &TopsConfig) -> Result<BTreeMap<String, f64>> {
    let globs = cfg.gates.coverage.exclude_globs.clone().unwrap_or_default();
    let scope = Scope::new(dir, &crate_root(dir, &cfg.crate_.name), &globs);
    aggregate_scores_in_scope(crap_raw, cfg, &scope)
}

fn aggregate_scores_in_scope(
    crap_raw: &str,
    cfg: &TopsConfig,
    scope: &Scope,
) -> Result<BTreeMap<String, f64>> {
    let scores = crap_ratchet::production_scores(crap_raw, scope).map_err(|err| anyhow!(err))?;
    let today = exemptions::today();
    let live: Vec<_> = exemptions::for_layer(cfg.exemptions.as_ref(), "crap")
        .into_iter()
        .filter(|record| record.live(&today))
        .collect();
    let mut map = BTreeMap::new();
    for score in scores {
        if live
            .iter()
            .any(|e| e.covers(&score.file, Some(&score.function)))
        {
            continue;
        }
        let key = format!("{}::{}", score.file, score.function);
        map.entry(key)
            .and_modify(|prev: &mut f64| *prev = prev.max(score.crap))
            .or_insert(score.crap);
    }
    Ok(map)
}

fn over_cap_only(scores: BTreeMap<String, f64>, cap: Option<f64>) -> BTreeMap<String, f64> {
    let Some(cap) = cap else {
        return BTreeMap::new();
    };
    scores
        .into_iter()
        .filter(|(_, score)| *score > cap)
        .collect()
}

fn write_v2(out: &Path, v2: &BaselineV2) -> Result<()> {
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(v2)?;
    fs::write(out, format!("{text}\n")).with_context(|| format!("write {}", out.display()))
}

fn read_report(report_dir: &Path) -> Result<Value> {
    let report_path = report_dir.join(REPORT_FILE);
    let raw = fs::read_to_string(&report_path).with_context(|| {
        format!(
            "read {}; run `cargo tops gate` first",
            report_path.display()
        )
    })?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", report_path.display()))
}

fn init_precheck(report: &Value) -> Result<(), String> {
    if report.get("dry_run").and_then(Value::as_bool) == Some(true) {
        return Err("last gate was a --dry-run; run `cargo tops gate` first".into());
    }
    if !matches!(report.get("exit_code").and_then(Value::as_i64), Some(0 | 1)) {
        return Err("last gate did not complete execution; run `cargo tops gate` first".into());
    }
    let layers = report
        .get("layers")
        .and_then(Value::as_array)
        .ok_or_else(|| "report.json has no layers".to_string())?;
    layers.iter().try_for_each(check_init_layer)
}

fn check_init_layer(row: &Value) -> Result<(), String> {
    let id = row.get("id").and_then(Value::as_str).unwrap_or("");
    let verdict = row.get("verdict").and_then(Value::as_str).unwrap_or("");
    match id {
        "unit" | "doc" | "integration" | "property"
            if ["fail", "not-run", "tool-missing", "tool-mismatch"].contains(&verdict) =>
        {
            Err(format!("layer {id} did not pass ({verdict}) in last gate"))
        }
        "crap" if invalid_init_measured(verdict, row) => Err(format!(
            "crap layer did not produce a valid report ({verdict})"
        )),
        "coverage" if invalid_init_measured(verdict, row) => Err(format!(
            "coverage layer did not produce a valid report ({verdict})"
        )),
        _ => Ok(()),
    }
}

fn invalid_init_measured(verdict: &str, row: &Value) -> bool {
    verdict != "pass" && (verdict != "fail" || row.get("judged").is_none())
}

fn read_last_crap_scores(
    dir: &Path,
    cfg: &TopsConfig,
    report_dir: &Path,
) -> Result<BTreeMap<String, f64>> {
    let source = report_dir.join(steps::CRAP);
    let raw = fs::read_to_string(&source).with_context(|| {
        format!(
            "read {}; run `cargo tops gate` with the crap layer first",
            source.display()
        )
    })?;
    if raw.trim().is_empty() {
        return Err(anyhow!("{} is empty", source.display()));
    }
    aggregate_scores(&raw, dir, cfg)
}

fn read_last_coverage_debt(
    dir: &Path,
    cfg: &TopsConfig,
    report_dir: &Path,
) -> Result<Option<CoverageLedger>> {
    if !coverage_floor::floors_set(&cfg.gates.coverage) {
        return Ok(None);
    }
    let (files, live) = last_coverage_files(dir, cfg, report_dir)?;
    if files.is_empty() {
        return Err(anyhow!(
            "{} has no production files; run `cargo tops gate` with the coverage layer first",
            report_dir.join(steps::SUMMARY).display()
        ));
    }
    Ok(coverage_floor::debt_snapshot(
        &files,
        &cfg.gates.coverage,
        &live,
    ))
}

fn last_coverage_files(
    dir: &Path,
    cfg: &TopsConfig,
    report_dir: &Path,
) -> Result<(
    Vec<coverage_floor::FileCoverage>,
    Vec<exemptions::Exemption>,
)> {
    let source = report_dir.join(steps::SUMMARY);
    let raw = fs::read_to_string(&source).with_context(|| {
        format!(
            "read {}; run `cargo tops gate` with the coverage layer first",
            source.display()
        )
    })?;
    if raw.trim().is_empty() {
        return Err(anyhow!("{} is empty", source.display()));
    }
    let globs = cfg.gates.coverage.exclude_globs.clone().unwrap_or_default();
    let scope = Scope::new(dir, &crate_root(dir, &cfg.crate_.name), &globs);
    let files = coverage_floor::production_files(&raw, &scope).map_err(|err| anyhow!(err))?;
    let today = exemptions::today();
    let live: Vec<_> = exemptions::for_layer(cfg.exemptions.as_ref(), "coverage")
        .into_iter()
        .filter(|record| record.live(&today))
        .collect();
    Ok((files, live))
}

fn artifact_report(dir: &Path) -> Result<(PathBuf, Value)> {
    let report_dir = std::path::absolute(laws::artifact_dir(dir))
        .with_context(|| format!("resolve the report dir of {}", dir.display()))?;
    let report = read_report(&report_dir)?;
    Ok((report_dir, report))
}

fn run_init(dir: &Path, cfg: &TopsConfig) -> Result<ExitCode> {
    if cfg.enforce == Some(Enforce::Advisory) {
        eprintln!("error: `cargo tops baseline --init` is refused under enforce: advisory");
        return Ok(ExitCode::from(1));
    }
    let out = path(dir, cfg);
    if out.exists() {
        eprintln!(
            "error: baseline {} already exists; run `cargo tops baseline` to refresh",
            out.display()
        );
        return Ok(ExitCode::from(1));
    }
    let (report_dir, report) = artifact_report(dir)?;
    if let Err(msg) = init_precheck(&report) {
        eprintln!("error: {msg}");
        return Ok(ExitCode::from(1));
    }
    let functions = over_cap_only(
        read_last_crap_scores(dir, cfg, &report_dir)?,
        cfg.gates.crap.cap(),
    );
    let coverage = read_last_coverage_debt(dir, cfg, &report_dir)?;
    let count = functions.len();
    let v2 = BaselineV2 {
        version: 2,
        crate_name: cfg.crate_.name.clone(),
        functions,
        coverage,
        lints: None,
        suppressions: None,
        exemptions: None,
        regressions: None,
    };
    write_v2(&out, &v2)?;
    println!("wrote {} ({count} over-cap functions)", out.display());
    Ok(ExitCode::SUCCESS)
}

fn gate_passed(report: &Value) -> bool {
    report.get("ok").and_then(Value::as_bool) == Some(true)
        && report.get("exit_code").and_then(Value::as_i64) == Some(0)
        && report.get("dry_run").and_then(Value::as_bool) != Some(true)
}

#[cfg(test)]
fn crap_layer_passed(report: &Value) -> bool {
    layer_passed(report, "crap")
}

fn layer_passed(report: &Value, id: &str) -> bool {
    report
        .get("layers")
        .and_then(Value::as_array)
        .is_some_and(|layers| {
            layers.iter().any(|layer| {
                layer.get("id").and_then(Value::as_str) == Some(id)
                    && layer.get("verdict").and_then(Value::as_str) == Some("pass")
            })
        })
}

enum RefreshInput {
    Ready(Box<BaselineV2>, BTreeMap<String, f64>, PathBuf),
    Stop(ExitCode),
}

fn load_refresh_inputs(dir: &Path, cfg: &TopsConfig, out: &Path) -> Result<RefreshInput> {
    if cfg.enforce == Some(Enforce::Advisory) {
        eprintln!("error: `cargo tops baseline` is refused under enforce: advisory");
        return Ok(RefreshInput::Stop(ExitCode::from(1)));
    }
    if !out.exists() {
        eprintln!(
            "error: missing baseline {}; run `cargo tops baseline --init`",
            out.display()
        );
        return Ok(RefreshInput::Stop(ExitCode::from(1)));
    }
    let (report_dir, report) = artifact_report(dir)?;
    if !gate_passed(&report) {
        eprintln!(
            "error: last gate did not pass; baseline refresh writes only after a passing gate"
        );
        return Ok(RefreshInput::Stop(ExitCode::from(1)));
    }
    if cfg.gates.crap.cap().is_some() && !layer_passed(&report, "crap") {
        eprintln!(
            "error: last gate did not pass the crap layer; baseline refresh writes only after a passing crap layer"
        );
        return Ok(RefreshInput::Stop(ExitCode::from(1)));
    }
    if coverage_floor::floors_set(&cfg.gates.coverage) && !layer_passed(&report, "coverage") {
        eprintln!(
            "error: last gate did not pass the coverage layer; baseline refresh writes only after a passing coverage layer"
        );
        return Ok(RefreshInput::Stop(ExitCode::from(1)));
    }
    let raw_old = fs::read_to_string(out).with_context(|| format!("read {}", out.display()))?;
    let existing = parse_and_validate(&raw_old, cfg)
        .map_err(|err| anyhow!("baseline {}: {err}", out.display()))?;
    let current = read_last_crap_scores(dir, cfg, &report_dir)?;
    Ok(RefreshInput::Ready(Box::new(existing), current, report_dir))
}

fn run_refresh(dir: &Path, cfg: &TopsConfig) -> Result<ExitCode> {
    let out = path(dir, cfg);
    match load_refresh_inputs(dir, cfg, &out)? {
        RefreshInput::Ready(existing, current, report_dir) => {
            write_refresh(dir, cfg, &out, *existing, &current, &report_dir)
        }
        RefreshInput::Stop(code) => Ok(code),
    }
}

fn write_refresh(
    dir: &Path,
    cfg: &TopsConfig,
    out: &Path,
    existing: BaselineV2,
    current: &BTreeMap<String, f64>,
    report_dir: &Path,
) -> Result<ExitCode> {
    let cap = cfg.gates.crap.cap();
    let rises = refresh_rises(&existing.functions, current, cap);
    let cov = coverage_refresh(dir, cfg, existing.coverage.as_ref(), report_dir)?;
    if !rises.is_empty() || !cov.problems.is_empty() {
        for msg in rises.iter().chain(cov.problems.iter()) {
            eprintln!("error: baseline refresh refused: {msg}");
        }
        return Ok(ExitCode::from(1));
    }
    let updated = next_functions(&existing.functions, current, cap);
    let count = updated.len();
    write_v2(
        out,
        &BaselineV2 {
            functions: updated,
            coverage: cov.next,
            ..existing
        },
    )?;
    println!("wrote {} ({count} over-cap functions)", out.display());
    Ok(ExitCode::SUCCESS)
}

struct CovRefresh {
    problems: Vec<String>,
    next: Option<CoverageLedger>,
}

fn coverage_refresh(
    dir: &Path,
    cfg: &TopsConfig,
    old: Option<&CoverageLedger>,
    report_dir: &Path,
) -> Result<CovRefresh> {
    if !coverage_floor::floors_set(&cfg.gates.coverage) {
        return Ok(CovRefresh {
            problems: Vec::new(),
            next: None,
        });
    }
    let (files, live) = last_coverage_files(dir, cfg, report_dir)?;
    Ok(CovRefresh {
        problems: coverage_floor::refresh_problems(old, &files, &cfg.gates.coverage, &live),
        next: coverage_floor::next_coverage(old, &files, &cfg.gates.coverage, &live),
    })
}

fn refresh_rises(
    old: &BTreeMap<String, f64>,
    current: &BTreeMap<String, f64>,
    cap: Option<f64>,
) -> Vec<String> {
    let mut rises = Vec::new();
    for (key, &score) in current {
        if let Some(&was) = old.get(key) {
            if score > was + EPSILON {
                rises.push(format!("{key} rose from {was:.2} to {score:.2}"));
            }
        } else if cap.is_some_and(|c| score > c) {
            rises.push(format!(
                "{key} is over cap ({score:.2}) and not in baseline"
            ));
        }
    }
    rises
}

fn next_functions(
    old: &BTreeMap<String, f64>,
    current: &BTreeMap<String, f64>,
    cap: Option<f64>,
) -> BTreeMap<String, f64> {
    let Some(cap) = cap else {
        return BTreeMap::new();
    };
    current
        .iter()
        .filter_map(|(key, &cur)| {
            (cur > cap)
                .then(|| old.get(key).map(|&was| (key.clone(), cur.min(was))))
                .flatten()
        })
        .collect()
}

fn validate_local_v2(cur_path: &Path, cfg: &TopsConfig) -> Result<Option<BaselineV2>> {
    let cur_raw = fs::read_to_string(cur_path)
        .map_err(|err| anyhow!("read {}: {err}", cur_path.display()))?;
    let cur = match parse_and_validate(&cur_raw, cfg) {
        Ok(v2) => v2,
        Err(err) => {
            eprintln!("error: baseline {}: {err}", cur_path.display());
            return Ok(None);
        }
    };
    let mut form_errors = check_cap_entries(&cur.functions, cfg.gates.crap.cap());
    if let Some(coverage) = &cur.coverage {
        form_errors.extend(coverage_floor::over_floor_problems(
            coverage,
            &cfg.gates.coverage,
        ));
    }
    if form_errors.is_empty() {
        return Ok(Some(cur));
    }
    for err in form_errors {
        eprintln!("error: {err}");
    }
    Ok(None)
}

fn run_check(dir: &Path, cfg: &TopsConfig, base: Option<&str>) -> Result<ExitCode> {
    let cur_path = path(dir, cfg);
    let Some(cur) = validate_local_v2(&cur_path, cfg)? else {
        return Ok(ExitCode::from(1));
    };
    let base_ref = base.unwrap_or("HEAD");
    let Some(old) = load_compared_ledger(dir, cfg, base_ref)? else {
        println!("first ledger (no compared baseline at {base_ref}): form valid");
        return Ok(ExitCode::SUCCESS);
    };
    let mut offenders = compare_functions(&cur, &old);
    offenders.extend(compare_coverage(&cur, &old));
    if offenders.is_empty() {
        println!(
            "ok: {} ({} entries checked against {base_ref})",
            cur_path.display(),
            cur.functions.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    for msg in offenders {
        eprintln!("error: baseline regression without record: {msg}");
    }
    Ok(ExitCode::from(1))
}

fn check_cap_entries(functions: &BTreeMap<String, f64>, cap: Option<f64>) -> Vec<String> {
    let mut errs = Vec::new();
    match cap {
        Some(cap) => {
            for (key, &score) in functions {
                if score <= cap {
                    errs.push(format!(
                        "{key} ({score:.2}) is at or under CRAP cap {cap}; remove it from functions"
                    ));
                }
            }
        }
        None if !functions.is_empty() => {
            errs.push("instance has no CRAP cap, so baseline functions must be empty".into());
        }
        None => {}
    }
    errs
}

fn load_compared_ledger(
    dir: &Path,
    cfg: &TopsConfig,
    base_ref: &str,
) -> Result<Option<BaselineV2>> {
    let top = git_stdout(dir, &["rev-parse", "--show-toplevel"])?;
    let commit = git_stdout(
        dir,
        &["rev-parse", "--verify", &format!("{base_ref}^{{commit}}")],
    )?;
    let abs_dir = dir
        .canonicalize()
        .with_context(|| format!("canonicalize {}", dir.display()))?;
    let abs_top = Path::new(&top)
        .canonicalize()
        .with_context(|| format!("canonicalize {top}"))?;
    let inst_rel = abs_dir
        .strip_prefix(&abs_top)
        .map_err(|_| anyhow!("{} is outside git root {top}", dir.display()))?;
    let yaml_git = git_rel(inst_rel, "rust-tops.yaml");
    let old_rel = if git_exists(dir, &commit, &yaml_git) {
        let yaml_raw = git_show(dir, &commit, &yaml_git)?;
        let old_cfg: TopsConfig = serde_saphyr::from_str_with_options(
            &yaml_raw,
            serde_saphyr::options! { strict_booleans: true },
        )
        .with_context(|| format!("parse {commit}:{yaml_git}"))?;
        old_cfg.baseline.unwrap_or_else(|| DEFAULT_BASELINE.into())
    } else {
        DEFAULT_BASELINE.into()
    };
    let old_git = git_rel(inst_rel, &old_rel);
    if !git_exists(dir, &commit, &old_git) {
        return Ok(None);
    }
    let old_raw = git_show(dir, &commit, &old_git)?;
    let old_v2 =
        parse_and_validate(&old_raw, cfg).map_err(|err| anyhow!("{commit}:{old_git}: {err}"))?;
    Ok(Some(old_v2))
}

fn compare_functions(cur: &BaselineV2, old: &BaselineV2) -> Vec<String> {
    let mut offenders = Vec::new();
    for (key, &cur_val) in &cur.functions {
        let old_val = old.functions.get(key).copied();
        let grew = old_val.is_none_or(|was| cur_val > was);
        if grew
            && !has_regression(
                cur.regressions.as_deref(),
                "functions",
                key,
                old_val,
                cur_val,
            )
        {
            let from = old_val.map_or_else(|| "absent".to_string(), |v| format!("{v:.2}"));
            offenders.push(format!("{key} ({from} -> {cur_val:.2})"));
        }
    }
    offenders
}

fn compare_coverage(cur: &BaselineV2, old: &BaselineV2) -> Vec<String> {
    coverage_floor::coverage_deltas(cur.coverage.as_ref(), old.coverage.as_ref())
        .into_iter()
        .filter(|delta| {
            !has_regression(
                cur.regressions.as_deref(),
                delta.kind,
                &delta.key,
                delta.from,
                delta.to,
            )
        })
        .map(|delta| {
            let from = delta
                .from
                .map_or_else(|| "absent".to_string(), |v| format!("{v:.2}"));
            format!("{} {} ({from} -> {:.2})", delta.kind, delta.key, delta.to)
        })
        .collect()
}

fn has_regression(
    records: Option<&[Regression]>,
    kind: &str,
    key: &str,
    from: Option<f64>,
    to: f64,
) -> bool {
    records.unwrap_or_default().iter().any(|rec| {
        rec.kind == kind && rec.key == key && same_opt(rec.from, from) && same_val(rec.to, to)
    })
}

fn same_opt(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => same_val(x, y),
        _ => false,
    }
}

fn same_val(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn git_stdout(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("spawn git {}", args.join(" ")))?;
    if !out.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_exists(dir: &Path, commit: &str, rel: &str) -> bool {
    Command::new("git")
        .args(["cat-file", "-e", &format!("{commit}:{rel}")])
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn git_show(dir: &Path, commit: &str, rel: &str) -> Result<String> {
    let out = Command::new("git")
        .args(["show", &format!("{commit}:{rel}")])
        .current_dir(dir)
        .output()
        .with_context(|| format!("git show {commit}:{rel}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "git show {commit}:{rel} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout).map_err(|err| anyhow!("{commit}:{rel} not UTF-8: {err}"))
}

fn git_rel(inst_rel: &Path, rel: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for comp in inst_rel.join(rel).components() {
        match comp {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::ParentDir => {
                parts.pop();
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::{
        aggregate_scores_in_scope, check_cap_entries, compare_functions, has_line_number,
        next_functions, over_cap_only, parse_and_validate, refresh_rises, BASELINE_SCHEMA_JSON,
    };
    use crate::config::{CoverageGate, CrapGate, Gates, MutationGate, Protocol, TopsConfig};
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::path::Path;

    fn cfg_for(name: &str) -> TopsConfig {
        TopsConfig {
            protocol: Protocol {
                id: "rust-tops".into(),
                version: "1.5.0".into(),
            },
            enforce: None,
            advisory: None,
            baseline: None,
            crate_: crate::config::Crate {
                name: name.into(),
                class: "library-core".into(),
            },
            goals: json!({}),
            layers: Vec::new(),
            gates: Gates {
                coverage: CoverageGate {
                    line_min_pct: Some(95.0),
                    region_min_pct: Some(90.0),
                    per_file_line_min_pct: Some(90.0),
                    exclude_globs: None,
                },
                crap: CrapGate {
                    threshold_agent: Some(6.0),
                    threshold_production: Some(6.0),
                    fail_on: None,
                    baseline: None,
                },
                mutation: MutationGate {
                    unexplained_survivors_max: 0,
                },
                base_ref: None,
                property: None,
                acceptance: None,
            },
            density: json!({}),
            performance: None,
            agent: None,
            exemptions: None,
        }
    }

    #[test]
    fn vendored_baseline_schema_matches_workspace() {
        let workspace =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/baseline.schema.json");
        let raw = std::fs::read_to_string(workspace).expect("workspace baseline.schema.json");
        assert_eq!(BASELINE_SCHEMA_JSON, raw);
        let doc: Value = serde_json::from_str(BASELINE_SCHEMA_JSON).expect("json");
        assert_eq!(doc.get("type").and_then(Value::as_str), Some("object"));
    }

    #[test]
    fn validates_form_line_numbers_sorting_and_single_line() {
        let cfg = cfg_for("demo");
        let good = "{\n  \"version\": 2,\n  \"crate\": \"demo\",\n  \"functions\": {\n    \"src/a.rs::alpha\": 10.0,\n    \"src/b.rs::Type::method\": 12.5\n  }\n}\n";
        assert!(parse_and_validate(good, &cfg).is_ok());

        let wrong_crate = good.replace("\"demo\"", "\"other\"");
        assert!(parse_and_validate(&wrong_crate, &cfg).is_err());

        let with_line = "{\n  \"version\": 2,\n  \"crate\": \"demo\",\n  \"functions\": {\n    \"src/a.rs:12::alpha\": 10.0\n  }\n}\n";
        assert!(parse_and_validate(with_line, &cfg).is_err());
        assert!(has_line_number("src/a.rs::alpha:42"));
        assert!(!has_line_number("src/a.rs::<impl Foo for Bar>::baz"));

        let unsorted = "{\n  \"version\": 2,\n  \"crate\": \"demo\",\n  \"functions\": {\n    \"src/b.rs::z\": 10.0,\n    \"src/a.rs::a\": 10.0\n  }\n}\n";
        assert!(parse_and_validate(unsorted, &cfg).is_err());

        let two_on_line = "{\"version\": 2, \"crate\": \"demo\", \"functions\": {\"src/a.rs::a\": 7.0, \"src/b.rs::b\": 8.0}}\n";
        assert!(parse_and_validate(two_on_line, &cfg).is_err());
    }

    #[test]
    fn aggregates_highest_score_filters_exempt_and_under_cap() {
        let base = std::env::temp_dir().join(format!("cargo-tops-v2-unit-{}", std::process::id()));
        let krate = base.join("crates/demo");
        std::fs::create_dir_all(krate.join("src")).expect("mkdir");
        std::fs::write(krate.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("manifest");
        let mut cfg = cfg_for("demo");
        cfg.exemptions = Some(json!([{
            "path": "src/main.rs",
            "symbol": "main",
            "layer": "crap",
            "reason": "cli dispatch table",
            "owner": "cli",
            "expires": "2099-01-01"
        }]));
        let report = json!({"entries": [
            {"file": "./crates/demo/src/a.rs", "function": "dup", "crap": 9.0},
            {"file": "./crates/demo/src/a.rs", "function": "dup", "crap": 14.5},
            {"file": "./crates/demo/src/a.rs", "function": "ok", "crap": 6.0},
            {"file": "./crates/demo/src/main.rs", "function": "main", "crap": 29.0},
            {"file": "./crates/demo/src/tests/t.rs", "function": "t", "crap": 50.0}
        ]})
        .to_string();
        let scope = crate::scope::Scope::new(&base, &krate, &[]);
        let agg = aggregate_scores_in_scope(&report, &cfg, &scope).expect("aggregate");
        let over = over_cap_only(agg, Some(6.0));
        assert_eq!(over.len(), 1);
        assert_eq!(over.get("src/a.rs::dup"), Some(&14.5));
        assert!(over_cap_only(over, None).is_empty());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn cap_check_and_refresh_helpers_reject_under_cap_and_rises() {
        let mut map = BTreeMap::new();
        map.insert("src/a.rs::at".into(), 6.0);
        assert_eq!(check_cap_entries(&map, Some(6.0)).len(), 1);
        assert_eq!(check_cap_entries(&map, None).len(), 1);
        assert!(check_cap_entries(&BTreeMap::new(), None).is_empty());

        let old_m = BTreeMap::from([("src/a.rs::f".into(), 10.0)]);
        let cur_m = BTreeMap::from([("src/a.rs::f".into(), 10.5)]);
        assert_eq!(refresh_rises(&old_m, &cur_m, Some(6.0)).len(), 1);
        let next_cur = BTreeMap::from([("src/a.rs::f".into(), 8.0)]);
        assert_eq!(
            next_functions(&old_m, &next_cur, Some(6.0)).get("src/a.rs::f"),
            Some(&8.0)
        );
    }

    #[test]
    fn compare_functions_requires_matching_regression_record() {
        let old = super::BaselineV2 {
            version: 2,
            crate_name: "demo".into(),
            functions: BTreeMap::from([("src/a.rs::f".into(), 10.0)]),
            coverage: None,
            lints: None,
            suppressions: None,
            exemptions: None,
            regressions: None,
        };
        let mut cur = super::BaselineV2 {
            functions: BTreeMap::from([("src/a.rs::f".into(), 10.5)]),
            ..old.clone()
        };
        assert_eq!(compare_functions(&cur, &old).len(), 1);
        cur.regressions = Some(vec![super::Regression {
            kind: "functions".into(),
            key: "src/a.rs::f".into(),
            from: Some(10.0),
            to: 10.5,
            reason: "documented".into(),
            owner: "team".into(),
            date: "2026-11-25".into(),
        }]);
        assert!(compare_functions(&cur, &old).is_empty());
    }

    #[test]
    fn layout_scanner_handles_escapes_malformed_lines_and_bad_schema() {
        let cfg = cfg_for("demo");
        assert!(parse_and_validate("not json", &cfg).is_err());
        assert!(
            parse_and_validate(r#"{"version":1,"crate":"demo","functions":{}}"#, &cfg).is_err()
        );
        let esc = "{\n  \"version\": 2,\n  \"crate\": \"demo\",\n  \"lints\": {\"k\": \"{\\\"}\"},\n  \"functions\": {\n    \"src/a.rs::a\": 8.0\n  }\n}\n";
        assert!(parse_and_validate(esc, &cfg).is_ok());
        let bad_entry =
            "{\n  \"version\": 2,\n  \"crate\": \"demo\",\n  \"functions\": {\n    123\n  }\n}\n";
        assert!(parse_and_validate(bad_entry, &cfg).is_err());
        let inline = "{\"version\": 2, \"crate\": \"demo\", \"functions\": {\"src/a.rs::a\": 8.0}}";
        assert!(parse_and_validate(inline, &cfg).is_err());
        assert!(super::split_key_value("noquote").is_none());
        assert!(super::split_key_value("\"k\\\"\" 1").is_none());
        assert!(has_line_number("no-sep"));
        assert!(super::find_functions_body("{}").is_none());
        assert!(super::extract_braced("[]", 0).is_none());
        assert!(super::extract_braced("{unclosed", 0).is_none());
    }

    #[test]
    fn init_precheck_and_git_rel_cover_edge_branches() {
        assert!(super::init_precheck(&json!({"dry_run": true})).is_err());
        assert!(super::init_precheck(&json!({"dry_run": false, "exit_code": 2})).is_err());
        assert!(super::init_precheck(&json!({
            "dry_run": false, "exit_code": 1,
            "layers": [{"id": "unit", "verdict": "fail"}]
        }))
        .is_err());
        assert!(super::init_precheck(&json!({
            "dry_run": false, "exit_code": 1,
            "layers": [{"id": "crap", "verdict": "fail"}]
        }))
        .is_err());
        assert!(super::init_precheck(&json!({
            "dry_run": false, "exit_code": 1,
            "layers": [{"id": "unit", "verdict": "pass"}, {"id": "crap", "verdict": "fail", "judged": {}}]
        }))
        .is_ok());
        assert!(super::init_precheck(&json!({
            "dry_run": false, "exit_code": 1,
            "layers": [
                {"id": "unit", "verdict": "pass"},
                {"id": "coverage", "verdict": "fail", "judged": {"files": 1}},
                {"id": "crap", "verdict": "fail", "judged": {}}
            ]
        }))
        .is_ok());
        assert!(super::init_precheck(&json!({
            "dry_run": false, "exit_code": 1,
            "layers": [{"id": "coverage", "verdict": "fail"}]
        }))
        .is_err());
        assert_eq!(
            super::git_rel(Path::new("crates/a"), "./../b/file.json"),
            "crates/b/file.json"
        );
        assert!(!super::same_opt(Some(1.0), None));
    }

    #[test]
    fn line_number_and_scanner_guards_kill_mutants() {
        for bad in [
            "::foo",
            "a.rs::",
            "a.rs:12::foo",
            "a.rs::foo:12",
            "a.rs::foo#L12",
        ] {
            assert!(has_line_number(bad), "{bad}");
        }
        assert!(super::has_colon_digits(":5") && super::has_colon_digits("f:5"));
        assert!(!super::has_colon_digits("Type::5"));
        let nested_fake = "{\n  \"version\": 2,\n  \"crate\": \"demo\",\n  \"lints\": {\n    \"note\": \"fake \\\"functions\\\": {\\\"src/x.rs::bad\\\": 9.0} {\",\n    \"functions\": {\"src/fake.rs::f\": 9.0}\n  },\n  \"functions\": {\n    \"src/a.rs::a\": 8.0\n  }\n}\n";
        assert!(parse_and_validate(nested_fake, &cfg_for("demo")).is_ok());
        assert!(super::gate_passed(
            &json!({"ok": true, "exit_code": 0, "dry_run": false})
        ));
        assert!(super::crap_layer_passed(
            &json!({"layers": [{"id": "crap", "verdict": "pass"}]})
        ));
        assert!(!super::crap_layer_passed(
            &json!({"layers": [{"id": "crap", "verdict": "fail"}]})
        ));
        assert!(!super::crap_layer_passed(
            &json!({"layers": [{"id": "unit", "verdict": "pass"}]})
        ));
        for bad in [
            json!({"ok": false, "exit_code": 0, "dry_run": false}),
            json!({"ok": true, "exit_code": 1, "dry_run": false}),
            json!({"ok": true, "exit_code": 0, "dry_run": true}),
        ] {
            assert!(!super::gate_passed(&bad));
        }
    }

    #[test]
    fn refresh_and_same_val_boundaries_kill_mutants() {
        let old_m = BTreeMap::from([("src/a.rs::f".into(), 10.0)]);
        for (k, v, want) in [
            ("src/a.rs::f", 10.005, 0),
            ("src/a.rs::f", 10.01, 0),
            ("src/a.rs::f", 10.02, 1),
            ("src/a.rs::at", 6.0, 0),
            ("src/a.rs::under", 5.0, 0),
            ("src/a.rs::new", 6.5, 1),
        ] {
            let cur = BTreeMap::from([(k.into(), v)]);
            assert_eq!(
                refresh_rises(&old_m, &cur, Some(6.0)).len(),
                want,
                "{k}={v}"
            );
        }
        let drop = BTreeMap::from([("src/a.rs::f".into(), 6.0)]);
        assert!(next_functions(&old_m, &drop, Some(6.0)).is_empty());
        assert!(!super::same_val(0.0, 1e-9));
    }

    #[test]
    fn kit_ledger_omits_coverage_and_stays_valid() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rust-tops-baseline.json");
        let raw = std::fs::read_to_string(path).expect("kit ledger");
        let v2 = parse_and_validate(&raw, &cfg_for("cargo-tops")).expect("valid kit ledger");
        assert!(v2.coverage.is_none());
        assert!(!raw.contains("\"coverage\""));
    }

    #[test]
    fn coverage_files_layout_and_line_numbers_are_rejected() {
        let cfg = cfg_for("demo");
        let good = "{\n  \"version\": 2,\n  \"crate\": \"demo\",\n  \"functions\": {},\n  \"coverage\": {\n    \"crate_line\": 80.0,\n    \"files\": {\n      \"src/a.rs\": 70.0,\n      \"src/z.rs\": 60.0\n    }\n  }\n}\n";
        assert!(parse_and_validate(good, &cfg).is_ok());

        let line_no = good.replace("src/a.rs", "src/a.rs:12");
        assert!(parse_and_validate(&line_no, &cfg).is_err());
        let hash_l = good.replace("src/a.rs", "src/a.rs#L12");
        assert!(parse_and_validate(&hash_l, &cfg).is_err());

        let unsorted = "{\n  \"version\": 2,\n  \"crate\": \"demo\",\n  \"functions\": {},\n  \"coverage\": {\n    \"files\": {\n      \"src/z.rs\": 60.0,\n      \"src/a.rs\": 70.0\n    }\n  }\n}\n";
        assert!(parse_and_validate(unsorted, &cfg).is_err());

        let inline = "{\"version\": 2, \"crate\": \"demo\", \"functions\": {}, \"coverage\": {\"files\": {\"src/a.rs\": 70.0, \"src/z.rs\": 60.0}}}\n";
        assert!(parse_and_validate(inline, &cfg).is_err());
        assert!(super::has_file_line_number("src/a.rs:9"));
        assert!(!super::has_file_line_number("src/a.rs"));
    }

    fn v2_with(coverage: super::CoverageLedger) -> super::BaselineV2 {
        super::BaselineV2 {
            version: 2,
            crate_name: "demo".into(),
            functions: BTreeMap::new(),
            coverage: Some(coverage),
            lints: None,
            suppressions: None,
            exemptions: None,
            regressions: None,
        }
    }

    fn rec(kind: &str, key: &str, from: Option<f64>, to: f64) -> super::Regression {
        super::Regression {
            kind: kind.into(),
            key: key.into(),
            from,
            to,
            reason: "r".into(),
            owner: "o".into(),
            date: "2026-11-27".into(),
        }
    }

    #[test]
    fn compare_coverage_names_drops_without_records() {
        let old = v2_with(super::CoverageLedger {
            crate_line: Some(80.0),
            crate_region: None,
            files: Some(BTreeMap::from([("src/a.rs".into(), 70.0)])),
        });
        let cur = v2_with(super::CoverageLedger {
            crate_line: Some(70.0),
            crate_region: Some(80.0),
            files: Some(BTreeMap::from([
                ("src/a.rs".into(), 60.0),
                ("src/b.rs".into(), 50.0),
            ])),
        });
        assert_eq!(super::compare_coverage(&cur, &old).len(), 4);
    }

    #[test]
    fn compare_coverage_accepts_matching_regression_records() {
        let old = v2_with(super::CoverageLedger {
            crate_line: Some(80.0),
            crate_region: None,
            files: Some(BTreeMap::from([("src/a.rs".into(), 70.0)])),
        });
        let mut cur = v2_with(super::CoverageLedger {
            crate_line: Some(70.0),
            crate_region: Some(80.0),
            files: Some(BTreeMap::from([
                ("src/a.rs".into(), 60.0),
                ("src/b.rs".into(), 50.0),
            ])),
        });
        cur.regressions = Some(vec![
            rec("coverage", "crate_line", Some(80.0), 70.0),
            rec("coverage", "crate_region", None, 80.0),
            rec("coverage.files", "src/a.rs", Some(70.0), 60.0),
            rec("coverage.files", "src/b.rs", None, 50.0),
        ]);
        assert!(super::compare_coverage(&cur, &old).is_empty());
    }
}
