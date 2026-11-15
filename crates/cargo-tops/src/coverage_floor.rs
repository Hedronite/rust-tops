//! The coverage floors of `gates.coverage` (§4, §7.3), judged on the
//! `cargo llvm-cov report --json --summary-only` export of the gate's
//! coverage run.
//!
//! Only production files of the crate count ([`crate::scope`]). The crate's
//! line and region totals are the sums over those files. Each of those
//! files must also meet `per_file_line_min_pct`, unless a live `coverage`
//! exemption names it. A floor set to `null` or left out is not judged. A
//! floor is met at exactly its value.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use serde_json::Value;

use crate::config::CoverageGate;
use crate::exemptions::Exemption;
use crate::scope::Scope;

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
    fn percent(self) -> Option<f64> {
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

/// Every floor `gate` misses on `files`, one line each. `exempt` holds the
/// live `coverage` exemptions. No production file at all is a miss: the
/// floors cannot be judged on a report that has none of the crate's files.
pub(crate) fn judge(
    files: &[FileCoverage],
    gate: &CoverageGate,
    exempt: &[Exemption],
) -> Vec<String> {
    if files.is_empty() {
        return vec![
            "coverage: no production file of this crate is in the coverage report, so its floors \
             cannot be judged"
                .to_string(),
        ];
    }
    let mut problems = crate_misses(files, gate);
    problems.extend(file_misses(files, gate.per_file_line_min_pct, exempt));
    problems
}

/// The crate's line and region totals against their floors.
fn crate_misses(files: &[FileCoverage], gate: &CoverageGate) -> Vec<String> {
    let total = |pick: fn(&FileCoverage) -> Count| {
        files.iter().map(pick).fold(Count::default(), Count::plus)
    };
    [
        ("line", "lines", total(|f| f.lines), gate.line_min_pct),
        (
            "region",
            "regions",
            total(|f| f.regions),
            gate.region_min_pct,
        ),
    ]
    .into_iter()
    .filter_map(|(what, unit, count, floor)| {
        below(count, floor).map(|pct| {
            format!(
                "coverage: {what} coverage {pct:.2}% ({} of {} {unit}) is below \
                 gates.coverage.{what}_min_pct {}",
                count.covered,
                count.count,
                floor.unwrap_or_default()
            )
        })
    })
    .collect()
}

/// Each file that is not exempt against the per-file line floor (§7.3).
fn file_misses(files: &[FileCoverage], floor: Option<f64>, exempt: &[Exemption]) -> Vec<String> {
    files
        .iter()
        .filter(|file| !exempt.iter().any(|e| e.covers(&file.rel, None)))
        .filter_map(|file| {
            below(file.lines, floor).map(|pct| {
                format!(
                    "coverage: {} line coverage {pct:.2}% ({} of {} lines) is below \
                     gates.coverage.per_file_line_min_pct {}",
                    file.rel,
                    file.lines.covered,
                    file.lines.count,
                    floor.unwrap_or_default()
                )
            })
        })
        .collect()
}

/// The percent when `count` is below `floor`. No floor, or nothing to
/// cover, is not below.
fn below(count: Count, floor: Option<f64>) -> Option<f64> {
    let floor = floor?;
    count.percent().filter(|pct| *pct < floor)
}

#[cfg(test)]
mod tests {
    use super::{judge, production_files, Count, FileCoverage};
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
            judge(&files, &gate(Some(80.0), Some(75.0), None), &[]),
            [] as [String; 0]
        );
        assert_eq!(
            judge(&files, &gate(Some(80.1), Some(75.1), None), &[]),
            [
                "coverage: line coverage 80.00% (16 of 20 lines) is below gates.coverage.line_min_pct 80.1",
                "coverage: region coverage 75.00% (15 of 20 regions) is below gates.coverage.region_min_pct 75.1",
            ]
        );
        assert_eq!(
            judge(&files, &gate(None, None, None), &[]),
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
            judge(&files, &gate(None, None, Some(90.0)), &exempt),
            ["coverage: src/b.rs line coverage 50.00% (5 of 10 lines) is below gates.coverage.per_file_line_min_pct 90"]
        );
        assert_eq!(judge(&files, &gate(None, None, Some(90.0)), &[]).len(), 2);
    }

    #[test]
    fn no_production_file_is_a_miss() {
        assert_eq!(
            judge(&[], &gate(None, None, None), &[]),
            ["coverage: no production file of this crate is in the coverage report, so its floors cannot be judged"]
        );
    }
}
