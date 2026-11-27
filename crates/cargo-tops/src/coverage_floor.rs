//! The coverage floors of `gates.coverage` (§4, §7.3, §2.3), judged on the
//! `cargo llvm-cov report --json --summary-only` export of the gate's
//! coverage run.
//!
//! Only production files of the crate count ([`crate::scope`]). The crate's
//! line and region totals are the sums over those files. Each of those
//! files must also meet `per_file_line_min_pct`, unless a live `coverage`
//! exemption names it. A floor set to `null` or left out is not judged. A
//! floor is met at exactly its value. Under `enforce: ratchet` a listed
//! below-floor entry passes at `current >= baseline - EPSILON`.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{CoverageGate, TopsConfig};
use crate::crap_ratchet::EPSILON;
use crate::exemptions::Exemption;
use crate::scope::Scope;

/// Empty coverage debt: every below-floor value is unlisted.
pub(crate) static EMPTY: CoverageLedger = CoverageLedger {
    crate_line: None,
    crate_region: None,
    files: None,
};

/// Below-floor coverage recorded in Baseline v2 (`coverage`).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct CoverageLedger {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crate_line: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crate_region: Option<f64>,
    #[serde(default, skip_serializing_if = "files_blank")]
    pub files: Option<BTreeMap<String, f64>>,
}

fn files_blank(files: &Option<BTreeMap<String, f64>>) -> bool {
    files.as_ref().is_none_or(BTreeMap::is_empty)
}

impl CoverageLedger {
    /// True when there is no crate or per-file debt to write.
    pub(crate) fn is_empty(&self) -> bool {
        self.crate_line.is_none() && self.crate_region.is_none() && files_blank(&self.files)
    }

    fn file(&self, rel: &str) -> Option<f64> {
        self.files.as_ref()?.get(rel).copied()
    }
}

/// Whether any coverage floor is set on `gate`.
pub(crate) fn floors_set(gate: &CoverageGate) -> bool {
    [
        gate.line_min_pct,
        gate.region_min_pct,
        gate.per_file_line_min_pct,
    ]
    .iter()
    .any(Option::is_some)
}

/// Whether the coverage and CRAP commands (or report rows) actually ran.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LayerRan {
    pub coverage: bool,
    pub crap: bool,
}

/// Whether the coverage and CRAP layers of this gate actually judge a ledger.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct JudgedLayers {
    pub coverage: bool,
    pub crap: bool,
}

impl JudgedLayers {
    /// `ran` is a passed coverage/crap command (gate) or a pass/fail row (report).
    pub(crate) fn of(cfg: &TopsConfig, ran: LayerRan) -> Self {
        Self {
            coverage: ran.coverage && floors_set(&cfg.gates.coverage),
            crap: ran.crap && cfg.gates.crap.cap().is_some(),
        }
    }
}

/// Covered and total units (lines or regions).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Count {
    pub covered: u64,
    pub count: u64,
}

impl Count {
    fn plus(self, other: Self) -> Self {
        Self {
            covered: self.covered + other.covered,
            count: self.count + other.count,
        }
    }

    /// Percent covered; `None` when there is nothing to cover.
    pub(crate) fn percent(self) -> Option<f64> {
        (self.count != 0).then(|| self.covered as f64 * 100.0 / self.count as f64)
    }
}

/// One production file of the crate in the coverage export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileCoverage {
    /// Relative to the crate root, `/`-separated.
    pub rel: String,
    pub lines: Count,
    pub regions: Count,
}

/// The production files of `scope` in an `llvm.coverage.json.export`.
pub(crate) fn production_files(export: &str, scope: &Scope) -> Result<Vec<FileCoverage>, String> {
    let doc: Value =
        serde_json::from_str(export).map_err(|err| format!("the summary is not JSON: {err}"))?;
    let files = doc
        .pointer("/data/0/files")
        .and_then(Value::as_array)
        .ok_or("the summary has no data[0].files")?;
    Ok(files
        .iter()
        .filter_map(|file| {
            let rel = scope.production(file.get("filename")?.as_str()?)?;
            Some(FileCoverage {
                rel,
                lines: count(file, "lines"),
                regions: count(file, "regions"),
            })
        })
        .collect())
}

fn count(file: &Value, kind: &str) -> Count {
    let field = |name: &str| {
        file.pointer(&format!("/summary/{kind}/{name}"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    Count {
        covered: field("covered"),
        count: field("count"),
    }
}

/// The crate's line and region percent over `files`; `None` for a total
/// with nothing to cover.
pub(crate) fn percents(files: &[FileCoverage]) -> (Option<f64>, Option<f64>) {
    let total = |pick: fn(&FileCoverage) -> Count| {
        files.iter().map(pick).fold(Count::default(), Count::plus)
    };
    (total(|f| f.lines).percent(), total(|f| f.regions).percent())
}

/// Every floor `gate` misses on `files`, one line each. `exempt` holds the
/// live `coverage` exemptions. `ratchet` is `Some` under `enforce: ratchet`
/// (and advisory with a ledger): listed entries pass at `current >=
/// baseline - EPSILON`; unlisted below-floor files fail against the floor.
/// `None` is the full floor (hard, omitted, advisory with no ledger).
/// No production file at all is a miss: the floors cannot be judged on a
/// report that has none of the crate's files.
pub(crate) fn judge(
    files: &[FileCoverage],
    gate: &CoverageGate,
    exempt: &[Exemption],
    ratchet: Option<&CoverageLedger>,
) -> Vec<String> {
    if files.is_empty() {
        return vec![
            "coverage: no production file of this crate is in the coverage report, so its floors \
             cannot be judged"
                .to_string(),
        ];
    }
    let mut problems = crate_misses(files, gate, ratchet);
    problems.extend(file_misses(
        files,
        gate.per_file_line_min_pct,
        exempt,
        ratchet,
    ));
    problems
}

/// The crate's line and region totals against their floors.
fn crate_misses(
    files: &[FileCoverage],
    gate: &CoverageGate,
    ratchet: Option<&CoverageLedger>,
) -> Vec<String> {
    let total = |pick: fn(&FileCoverage) -> Count| {
        files.iter().map(pick).fold(Count::default(), Count::plus)
    };
    [
        (
            "line",
            "lines",
            total(|f| f.lines),
            gate.line_min_pct,
            ratchet.and_then(|l| l.crate_line),
        ),
        (
            "region",
            "regions",
            total(|f| f.regions),
            gate.region_min_pct,
            ratchet.and_then(|l| l.crate_region),
        ),
    ]
    .into_iter()
    .filter_map(|(what, unit, count, floor, was)| {
        crate_problem(what, unit, count, floor, bind(ratchet, was))
    })
    .collect()
}

fn bind(ratchet: Option<&CoverageLedger>, was: Option<f64>) -> Bind {
    match ratchet {
        Some(_) => Bind::Ratchet { was },
        None => Bind::Floor,
    }
}

#[derive(Clone, Copy)]
enum Bind {
    Floor,
    Ratchet { was: Option<f64> },
}

fn crate_problem(
    what: &str,
    unit: &str,
    count: Count,
    floor: Option<f64>,
    bind: Bind,
) -> Option<String> {
    let floor = floor?;
    let pct = below_pct(count, floor)?;
    match miss_kind(pct, floor, bind) {
        Miss::Ok => None,
        Miss::Floor => Some(format!(
            "coverage: {what} coverage {pct:.2}% ({} of {} {unit}) is below \
             gates.coverage.{what}_min_pct {floor}",
            count.covered, count.count
        )),
        Miss::Drop(baseline) => Some(format!(
            "coverage: {what} coverage {pct:.2}% ({} of {} {unit}) dropped below \
             baseline {baseline:.2} (floor {floor})",
            count.covered, count.count
        )),
    }
}

/// Each file that is not exempt against the per-file line floor (§7.3).
fn file_misses(
    files: &[FileCoverage],
    floor: Option<f64>,
    exempt: &[Exemption],
    ratchet: Option<&CoverageLedger>,
) -> Vec<String> {
    files
        .iter()
        .filter(|file| !exempt.iter().any(|e| e.covers(&file.rel, None)))
        .filter_map(|file| file_problem(file, floor, ratchet))
        .collect()
}

fn file_problem(
    file: &FileCoverage,
    floor: Option<f64>,
    ratchet: Option<&CoverageLedger>,
) -> Option<String> {
    let floor = floor?;
    let pct = below_pct(file.lines, floor)?;
    let was = ratchet.and_then(|l| l.file(&file.rel));
    match miss_kind(pct, floor, bind(ratchet, was)) {
        Miss::Ok => None,
        Miss::Floor => Some(format!(
            "coverage: {} line coverage {pct:.2}% ({} of {} lines) is below \
             gates.coverage.per_file_line_min_pct {floor}",
            file.rel, file.lines.covered, file.lines.count
        )),
        Miss::Drop(baseline) => Some(format!(
            "coverage: {} line coverage {pct:.2}% ({} of {} lines) dropped below \
             baseline {baseline:.2} (floor {floor})",
            file.rel, file.lines.covered, file.lines.count
        )),
    }
}

enum Miss {
    Ok,
    Floor,
    Drop(f64),
}

fn miss_kind(pct: f64, floor: f64, bind: Bind) -> Miss {
    if pct >= floor {
        Miss::Ok
    } else {
        ratchet_miss(pct, bind)
    }
}

fn ratchet_miss(pct: f64, bind: Bind) -> Miss {
    match bind {
        Bind::Floor | Bind::Ratchet { was: None } => Miss::Floor,
        Bind::Ratchet { was: Some(was) } if !dropped(pct, was) => Miss::Ok,
        Bind::Ratchet { was: Some(was) } => Miss::Drop(was),
    }
}

/// True when `cur` is more than [`EPSILON`] below `was`.
fn dropped(cur: f64, was: f64) -> bool {
    cur < was - EPSILON
}

/// The percent when `count` is below `floor`. No floor, or nothing to
/// cover, is not below.
fn below_pct(count: Count, floor: f64) -> Option<f64> {
    count.percent().filter(|pct| *pct < floor)
}

/// Below-floor coverage to record on `--init`. Omits `coverage` when every
/// floor is met. Exempt files still count toward crate totals.
pub(crate) fn debt_snapshot(
    files: &[FileCoverage],
    gate: &CoverageGate,
    exempt: &[Exemption],
) -> Option<CoverageLedger> {
    let (line, region) = percents(files);
    let ledger = CoverageLedger {
        crate_line: metric_debt(line, gate.line_min_pct),
        crate_region: metric_debt(region, gate.region_min_pct),
        files: file_debt(files, gate.per_file_line_min_pct, exempt),
    };
    (!ledger.is_empty()).then_some(ledger)
}

fn metric_debt(pct: Option<f64>, floor: Option<f64>) -> Option<f64> {
    pct.zip(floor)
        .and_then(|(pct, floor)| (pct < floor).then_some(pct))
}

fn file_debt(
    files: &[FileCoverage],
    floor: Option<f64>,
    exempt: &[Exemption],
) -> Option<BTreeMap<String, f64>> {
    let floor = floor?;
    let map: BTreeMap<String, f64> = files
        .iter()
        .filter(|file| !exempt.iter().any(|e| e.covers(&file.rel, None)))
        .filter_map(|file| below_pct(file.lines, floor).map(|pct| (file.rel.clone(), pct)))
        .collect();
    (!map.is_empty()).then_some(map)
}

/// Refresh refusals: a drop below `baseline - EPSILON`, or a new below-floor
/// crate/file entry that was not already in the ledger.
pub(crate) fn refresh_problems(
    old: Option<&CoverageLedger>,
    files: &[FileCoverage],
    gate: &CoverageGate,
    exempt: &[Exemption],
) -> Vec<String> {
    let (line, region) = percents(files);
    let mut problems = Vec::new();
    problems.extend(metric_refresh_problems(
        "crate_line",
        line,
        old.and_then(|o| o.crate_line),
        gate.line_min_pct,
    ));
    problems.extend(metric_refresh_problems(
        "crate_region",
        region,
        old.and_then(|o| o.crate_region),
        gate.region_min_pct,
    ));
    problems.extend(file_refresh_problems(old, files, gate, exempt));
    problems
}

fn metric_refresh_problems(
    key: &str,
    cur: Option<f64>,
    was: Option<f64>,
    floor: Option<f64>,
) -> Option<String> {
    let floor = floor?;
    let cur = cur?;
    match was {
        Some(was) if dropped(cur, was) => {
            Some(format!("coverage.{key} dropped from {was:.2} to {cur:.2}"))
        }
        None if cur < floor => Some(format!(
            "coverage.{key} is below floor ({cur:.2}) and not in baseline"
        )),
        _ => None,
    }
}

fn file_refresh_problems(
    old: Option<&CoverageLedger>,
    files: &[FileCoverage],
    gate: &CoverageGate,
    exempt: &[Exemption],
) -> Vec<String> {
    let Some(floor) = gate.per_file_line_min_pct else {
        return Vec::new();
    };
    files
        .iter()
        .filter(|file| !exempt.iter().any(|e| e.covers(&file.rel, None)))
        .filter_map(|file| {
            let pct = below_pct(file.lines, floor)?;
            match old.and_then(|o| o.file(&file.rel)) {
                Some(was) if dropped(pct, was) => Some(format!(
                    "coverage.files {} dropped from {was:.2} to {pct:.2}",
                    file.rel
                )),
                None => Some(format!(
                    "coverage.files {} is below floor ({pct:.2}) and not in baseline",
                    file.rel
                )),
                _ => None,
            }
        })
        .collect()
}

/// Next coverage object after a passing refresh: raise toward the floor,
/// drop entries that now meet it / are exempt / are gone. `None` when empty.
pub(crate) fn next_coverage(
    old: Option<&CoverageLedger>,
    files: &[FileCoverage],
    gate: &CoverageGate,
    exempt: &[Exemption],
) -> Option<CoverageLedger> {
    let (line, region) = percents(files);
    let ledger = CoverageLedger {
        crate_line: next_metric(line, old.and_then(|o| o.crate_line), gate.line_min_pct),
        crate_region: next_metric(
            region,
            old.and_then(|o| o.crate_region),
            gate.region_min_pct,
        ),
        files: next_files(old, files, gate.per_file_line_min_pct, exempt),
    };
    (!ledger.is_empty()).then_some(ledger)
}

fn next_metric(cur: Option<f64>, was: Option<f64>, floor: Option<f64>) -> Option<f64> {
    let floor = floor?;
    let cur = cur?;
    let raised = was.map_or(cur, |was| cur.max(was));
    (raised < floor).then_some(raised)
}

fn next_files(
    old: Option<&CoverageLedger>,
    files: &[FileCoverage],
    floor: Option<f64>,
    exempt: &[Exemption],
) -> Option<BTreeMap<String, f64>> {
    let floor = floor?;
    let map: BTreeMap<String, f64> = files
        .iter()
        .filter(|file| !exempt.iter().any(|e| e.covers(&file.rel, None)))
        .filter_map(|file| {
            let pct = file.lines.percent()?;
            let was = old.and_then(|o| o.file(&file.rel))?;
            let raised = pct.max(was);
            (raised < floor).then(|| (file.rel.clone(), raised))
        })
        .collect();
    (!map.is_empty()).then_some(map)
}

/// `--check` refusals: a coverage entry at or above its floor, or present
/// when that floor is unset.
pub(crate) fn over_floor_problems(ledger: &CoverageLedger, gate: &CoverageGate) -> Vec<String> {
    let mut problems = Vec::new();
    problems.extend(over_metric(
        "crate_line",
        ledger.crate_line,
        gate.line_min_pct,
        "line_min_pct",
    ));
    problems.extend(over_metric(
        "crate_region",
        ledger.crate_region,
        gate.region_min_pct,
        "region_min_pct",
    ));
    if let Some(files) = &ledger.files {
        problems.extend(over_files(files, gate.per_file_line_min_pct));
    }
    problems
}

fn over_metric(
    key: &str,
    value: Option<f64>,
    floor: Option<f64>,
    floor_key: &str,
) -> Option<String> {
    let value = value?;
    match floor {
        None => Some(format!(
            "coverage.{key} is set but gates.coverage.{floor_key} is unset"
        )),
        Some(floor) if value >= floor => Some(format!(
            "coverage.{key} ({value:.2}) is at or above {floor_key} {floor}; remove it from coverage"
        )),
        Some(_) => None,
    }
}

fn over_files(files: &BTreeMap<String, f64>, floor: Option<f64>) -> Vec<String> {
    match floor {
        None => vec![
            "coverage.files is set but gates.coverage.per_file_line_min_pct is unset".into(),
        ],
        Some(floor) => files
            .iter()
            .filter(|(_, &value)| value >= floor)
            .map(|(key, value)| {
                format!(
                    "coverage.files {key} ({value:.2}) is at or above per_file_line_min_pct {floor}; \
                     remove it from coverage"
                )
            })
            .collect(),
    }
}

/// Coverage values that fell or appeared versus the compared ledger.
pub(crate) fn coverage_deltas(
    cur: Option<&CoverageLedger>,
    old: Option<&CoverageLedger>,
) -> Vec<CoverageDelta> {
    let mut out = Vec::new();
    out.extend(metric_delta(
        "crate_line",
        cur.and_then(|c| c.crate_line),
        old.and_then(|o| o.crate_line),
    ));
    out.extend(metric_delta(
        "crate_region",
        cur.and_then(|c| c.crate_region),
        old.and_then(|o| o.crate_region),
    ));
    out.extend(file_deltas(
        cur.and_then(|c| c.files.as_ref()),
        old.and_then(|o| o.files.as_ref()),
    ));
    out
}

/// One lowered or newly added coverage debt for `--check`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CoverageDelta {
    pub kind: &'static str,
    pub key: String,
    pub from: Option<f64>,
    pub to: f64,
}

fn metric_delta(key: &'static str, cur: Option<f64>, old: Option<f64>) -> Option<CoverageDelta> {
    let to = cur?;
    let grew = old.is_none_or(|was| to < was);
    grew.then(|| CoverageDelta {
        kind: "coverage",
        key: key.to_string(),
        from: old,
        to,
    })
}

fn file_deltas(
    cur: Option<&BTreeMap<String, f64>>,
    old: Option<&BTreeMap<String, f64>>,
) -> Vec<CoverageDelta> {
    let Some(cur) = cur else {
        return Vec::new();
    };
    cur.iter()
        .filter_map(|(key, &to)| {
            let from = old.and_then(|m| m.get(key)).copied();
            let grew = from.is_none_or(|was| to < was);
            grew.then(|| CoverageDelta {
                kind: "coverage.files",
                key: key.clone(),
                from,
                to,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{judge, percents, production_files, Count, FileCoverage};
    use crate::config::CoverageGate;
    use crate::exemptions::Exemption;
    use crate::scope::Scope;
    use std::path::Path;

    fn gate(line: Option<f64>, region: Option<f64>, per_file: Option<f64>) -> CoverageGate {
        CoverageGate {
            line_min_pct: line,
            region_min_pct: region,
            per_file_line_min_pct: per_file,
            exclude_globs: None,
        }
    }

    fn file(rel: &str, lines: (u64, u64), regions: (u64, u64)) -> FileCoverage {
        FileCoverage {
            rel: rel.to_string(),
            lines: Count {
                covered: lines.0,
                count: lines.1,
            },
            regions: Count {
                covered: regions.0,
                count: regions.1,
            },
        }
    }

    #[test]
    fn the_export_keeps_only_production_files_of_the_crate() {
        let dir = Path::new("/w");
        let scope = Scope::new(dir, &dir.join("crates/tool"), &[]);
        let export = r#"{"data":[{"files":[
            {"filename":"/w/crates/tool/src/lib.rs","summary":{"lines":{"count":10,"covered":9},"regions":{"count":4,"covered":3}}},
            {"filename":"/w/crates/tool/src/tests/a.rs","summary":{"lines":{"count":5,"covered":0},"regions":{"count":1,"covered":0}}},
            {"filename":"/w/crates/other/src/lib.rs","summary":{"lines":{"count":5,"covered":0},"regions":{"count":1,"covered":0}}},
            {"filename":"crates/tool/src/main.rs","summary":{}},
            {"summary":{}}
        ]}]}"#;
        assert_eq!(
            production_files(export, &scope),
            Ok(vec![
                file("src/lib.rs", (9, 10), (3, 4)),
                file("src/main.rs", (0, 0), (0, 0)),
            ])
        );
        assert!(production_files("{}", &scope)
            .is_err_and(|err| err == "the summary has no data[0].files"));
        assert!(production_files("stub", &scope)
            .is_err_and(|err| err.starts_with("the summary is not JSON")));
    }

    #[test]
    fn a_crate_total_below_its_floor_fails_and_one_at_the_floor_passes() {
        let files = [
            file("src/a.rs", (8, 10), (6, 10)),
            file("src/b.rs", (8, 10), (9, 10)),
        ];
        assert_eq!(
            judge(&files, &gate(Some(80.0), Some(75.0), None), &[], None),
            [] as [String; 0]
        );
        assert_eq!(
            judge(&files, &gate(Some(80.1), Some(75.1), None), &[], None),
            [
                "coverage: line coverage 80.00% (16 of 20 lines) is below gates.coverage.line_min_pct 80.1",
                "coverage: region coverage 75.00% (15 of 20 regions) is below gates.coverage.region_min_pct 75.1",
            ]
        );
        assert_eq!(
            judge(&files, &gate(None, None, None), &[], None),
            [] as [String; 0]
        );
    }

    #[test]
    fn every_file_below_the_per_file_floor_is_reported_unless_exempt() {
        let files = [
            file("src/a.rs", (9, 10), (1, 1)),
            file("src/b.rs", (5, 10), (1, 1)),
            file("src/gen/c.rs", (1, 10), (1, 1)),
            file("src/empty.rs", (0, 0), (0, 0)),
        ];
        let exempt = [Exemption {
            path: "src/gen/**".to_string(),
            symbol: None,
            expires: "2030-01-01".to_string(),
        }];
        assert_eq!(
            judge(&files, &gate(None, None, Some(90.0)), &exempt, None),
            ["coverage: src/b.rs line coverage 50.00% (5 of 10 lines) is below gates.coverage.per_file_line_min_pct 90"]
        );
        assert_eq!(
            judge(&files, &gate(None, None, Some(90.0)), &[], None).len(),
            2
        );
    }

    #[test]
    fn no_production_file_is_a_miss() {
        assert_eq!(
            judge(&[], &gate(None, None, None), &[], None),
            ["coverage: no production file of this crate is in the coverage report, so its floors cannot be judged"]
        );
    }

    #[test]
    fn percents_are_the_line_and_region_percent_of_the_whole_crate() {
        let files = [file("a.rs", (1, 4), (2, 4)), file("b.rs", (3, 4), (0, 4))];
        assert_eq!(percents(&files), (Some(50.0), Some(25.0)));
        assert_eq!(percents(&[]), (None, None));
    }

    fn ledger(
        line: Option<f64>,
        region: Option<f64>,
        files: &[(&str, f64)],
    ) -> super::CoverageLedger {
        super::CoverageLedger {
            crate_line: line,
            crate_region: region,
            files: (!files.is_empty())
                .then(|| files.iter().map(|(k, v)| ((*k).to_string(), *v)).collect()),
        }
    }

    #[test]
    fn ratchet_listed_crate_passes_within_epsilon_and_fails_a_drop() {
        let files = [file("src/a.rs", (70, 100), (70, 100))];
        let debt = ledger(Some(70.0), Some(70.0), &[]);
        let g = gate(Some(95.0), Some(90.0), None);
        let noise = [file("src/a.rs", (6999, 10000), (6999, 10000))];
        assert!(judge(&noise, &g, &[], Some(&debt)).is_empty());
        let drop = [file("src/a.rs", (69, 100), (69, 100))];
        let problems = judge(&drop, &g, &[], Some(&debt));
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            problems
                .iter()
                .all(|p| p.contains("dropped below baseline 70.00")),
            "{problems:?}"
        );
        let unlisted = judge(&files, &g, &[], Some(&super::EMPTY));
        assert!(
            unlisted.iter().any(|p| p.contains("line_min_pct 95")),
            "{unlisted:?}"
        );
        assert!(judge(&files, &g, &[], None)
            .iter()
            .any(|p| p.contains("line_min_pct 95")));
    }

    #[test]
    fn ratchet_unlisted_file_fails_the_floor_listed_file_ratchets() {
        let files = [
            file("src/a.rs", (70, 100), (100, 100)),
            file("src/b.rs", (70, 100), (100, 100)),
        ];
        let debt = ledger(None, None, &[("src/a.rs", 70.0)]);
        let g = gate(None, None, Some(90.0));
        let problems = judge(&files, &g, &[], Some(&debt));
        assert_eq!(problems.len(), 1, "{problems:?}");
        let line = problems.first().map(String::as_str).unwrap_or("");
        assert!(
            line.contains("src/b.rs") && line.contains("per_file_line_min_pct 90"),
            "{problems:?}"
        );
        let drop = [file("src/a.rs", (69, 100), (100, 100))];
        assert!(judge(&drop, &g, &[], Some(&debt))
            .iter()
            .any(|p| p.contains("dropped below baseline 70.00")));
    }

    #[test]
    fn live_exemption_still_excuses_the_per_file_floor_under_ratchet() {
        let files = [
            file("src/a.rs", (70, 100), (100, 100)),
            file("src/gen/x.rs", (10, 100), (100, 100)),
        ];
        let exempt = [Exemption {
            path: "src/gen/**".to_string(),
            symbol: None,
            expires: "2030-01-01".to_string(),
        }];
        let debt = ledger(None, None, &[("src/a.rs", 70.0)]);
        assert!(judge(&files, &gate(None, None, Some(90.0)), &exempt, Some(&debt)).is_empty());
    }

    #[test]
    fn debt_snapshot_records_below_floor_values_only() {
        use super::{debt_snapshot, floors_set};
        let files = [
            file("src/a.rs", (70, 100), (80, 100)),
            file("src/b.rs", (95, 100), (95, 100)),
        ];
        let g = gate(Some(95.0), Some(90.0), Some(90.0));
        assert!(floors_set(&g));
        assert!(!floors_set(&gate(None, None, None)));
        let snap = debt_snapshot(&files, &g, &[]).expect("debt");
        assert_eq!(snap.crate_line, Some(82.5));
        assert_eq!(snap.crate_region, Some(87.5));
        assert_eq!(
            snap.files.as_ref().and_then(|m| m.get("src/a.rs")),
            Some(&70.0)
        );
        assert!(debt_snapshot(&[file("src/a.rs", (95, 100), (90, 100))], &g, &[]).is_none());
    }

    #[test]
    fn refresh_raises_toward_the_floor_and_refuses_drops() {
        use super::{debt_snapshot, next_coverage, refresh_problems};
        let g = gate(Some(95.0), Some(90.0), Some(90.0));
        let snap = debt_snapshot(
            &[
                file("src/a.rs", (70, 100), (80, 100)),
                file("src/b.rs", (95, 100), (95, 100)),
            ],
            &g,
            &[],
        )
        .expect("debt");
        let improved = [
            file("src/a.rs", (80, 100), (88, 100)),
            file("src/b.rs", (95, 100), (95, 100)),
        ];
        assert!(refresh_problems(Some(&snap), &improved, &g, &[]).is_empty());
        let next = next_coverage(Some(&snap), &improved, &g, &[]).expect("next");
        assert_eq!(next.crate_line, Some(87.5));
        assert!(next.crate_region.is_none());
        assert_eq!(
            next.files.as_ref().and_then(|m| m.get("src/a.rs")),
            Some(&80.0)
        );
        assert!(!refresh_problems(
            Some(&snap),
            &[file("src/a.rs", (60, 100), (80, 100))],
            &g,
            &[]
        )
        .is_empty());
        assert!(refresh_problems(
            Some(&snap),
            &[file("src/new.rs", (50, 100), (50, 100))],
            &g,
            &[]
        )
        .iter()
        .any(|p| p.contains("src/new.rs")));
    }

    #[test]
    fn dropped_is_strictly_more_than_epsilon_below_baseline() {
        assert!(super::dropped(69.0, 70.0));
        assert!(!super::dropped(70.0, 70.0));
        assert!(!super::dropped(69.99, 70.0));
        assert!(super::dropped(69.98, 70.0));
        assert!(!super::dropped(80.0, 70.0));
    }

    #[test]
    fn crate_line_refresh_drop_is_independent_of_files() {
        use super::{metric_refresh_problems, refresh_problems};
        let g = gate(Some(95.0), None, None);
        let old = ledger(Some(80.0), None, &[]);
        assert!(
            metric_refresh_problems("crate_line", Some(70.0), Some(80.0), Some(95.0)).is_some()
        );
        assert!(
            metric_refresh_problems("crate_line", Some(80.0), Some(80.0), Some(95.0)).is_none()
        );
        assert!(metric_refresh_problems("crate_line", Some(80.0), None, Some(95.0)).is_some());
        assert!(metric_refresh_problems("crate_line", Some(95.0), None, Some(95.0)).is_none());
        let files = [file("src/a.rs", (70, 100), (100, 100))];
        assert!(refresh_problems(Some(&old), &files, &g, &[])
            .iter()
            .any(|p| p.contains("crate_line")));
    }

    #[test]
    fn file_refresh_drop_is_independent_of_crate_totals() {
        use super::refresh_problems;
        let g = gate(None, None, Some(90.0));
        let old = ledger(None, None, &[("src/a.rs", 80.0)]);
        let drop = [file("src/a.rs", (70, 100), (100, 100))];
        assert!(refresh_problems(Some(&old), &drop, &g, &[])
            .iter()
            .any(|p| p.contains("src/a.rs") && p.contains("dropped")));
        let same = [file("src/a.rs", (80, 100), (100, 100))];
        assert!(refresh_problems(Some(&old), &same, &g, &[]).is_empty());
        let fresh = [file("src/new.rs", (50, 100), (100, 100))];
        assert!(refresh_problems(Some(&old), &fresh, &g, &[])
            .iter()
            .any(|p| p.contains("src/new.rs")));
    }

    #[test]
    fn check_helpers_reject_over_floor_and_count_deltas() {
        use super::{coverage_deltas, debt_snapshot, over_floor_problems, CoverageLedger};
        let g = gate(Some(95.0), Some(90.0), Some(90.0));
        let snap = debt_snapshot(
            &[
                file("src/a.rs", (70, 100), (80, 100)),
                file("src/b.rs", (95, 100), (95, 100)),
            ],
            &g,
            &[],
        )
        .expect("debt");
        let at_floor = CoverageLedger {
            crate_line: Some(95.0),
            ..CoverageLedger::default()
        };
        assert_eq!(over_floor_problems(&at_floor, &g).len(), 1);
        assert_eq!(over_floor_problems(&snap, &gate(None, None, None)).len(), 3);
        let old = ledger(Some(80.0), None, &[("src/a.rs", 70.0)]);
        let cur = ledger(
            Some(70.0),
            Some(80.0),
            &[("src/a.rs", 60.0), ("src/b.rs", 50.0)],
        );
        assert_eq!(coverage_deltas(Some(&cur), Some(&old)).len(), 4);
    }
}
