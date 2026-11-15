//! The CRAP threshold on new code and the ratchet on existing code (§2,
//! §16), judged on the gate's `cargo crap --format json` report.
//!
//! A production function is new when the baseline has no function of that
//! name in that file; with no baseline every function is new. A new
//! function fails above the threshold. A function the baseline has fails
//! when an edit takes it above the threshold, or, when it was already above
//! it, when its score rises more than [`EPSILON`] over its baseline score.
//! So changed code is held to the threshold, and legacy code may only move
//! down. A live `crap` exemption takes a function out of both rules.
//!
//! The baseline is a `cargo crap --format json` report (§16), written by
//! `cargo tops crap-baseline` from the last gate's report.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::collections::HashMap;

use serde_json::Value;

use crate::exemptions::Exemption;
use crate::scope::Scope;

/// A rise at or under this is no rise: `cargo crap`'s own default epsilon.
pub(crate) const EPSILON: f64 = 0.01;

/// One production function's score.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Score {
    /// Relative to the crate root, `/`-separated.
    pub file: String,
    pub function: String,
    pub crap: f64,
}

/// The production functions of `scope` in a `cargo crap` JSON report.
pub(crate) fn production_scores(report: &str, scope: &Scope) -> Result<Vec<Score>, String> {
    let doc: Value =
        serde_json::from_str(report).map_err(|err| format!("the report is not JSON: {err}"))?;
    let entries = doc
        .get("entries")
        .and_then(Value::as_array)
        .ok_or("the report has no entries")?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            Some(Score {
                file: scope.production(entry.get("file")?.as_str()?)?,
                function: entry.get("function")?.as_str()?.to_string(),
                crap: entry.get("crap")?.as_f64()?,
            })
        })
        .collect())
}

/// A function's baseline score: the highest of that name in that file.
fn baseline_index(baseline: &[Score]) -> HashMap<(&str, &str), f64> {
    let mut index: HashMap<(&str, &str), f64> = HashMap::new();
    for score in baseline {
        let slot = index
            .entry((score.file.as_str(), score.function.as_str()))
            .or_insert(score.crap);
        *slot = slot.max(score.crap);
    }
    index
}

/// Every function in `current` that breaks the threshold or the ratchet,
/// one line each. `exempt` holds the live `crap` exemptions.
pub(crate) fn judge(
    current: &[Score],
    baseline: Option<&[Score]>,
    threshold: f64,
    exempt: &[Exemption],
) -> Vec<String> {
    let index = baseline.map(baseline_index).unwrap_or_default();
    current
        .iter()
        .filter(|score| {
            !exempt
                .iter()
                .any(|e| e.covers(&score.file, Some(&score.function)))
        })
        .filter_map(|score| {
            let was = index
                .get(&(score.file.as_str(), score.function.as_str()))
                .copied();
            (score.crap > limit(threshold, was)).then(|| problem(score, was, threshold))
        })
        .collect()
}

/// The highest score a function may have: the threshold, or a baseline
/// score already over it.
fn limit(threshold: f64, was: Option<f64>) -> f64 {
    was.filter(|baseline| *baseline > threshold)
        .map_or(threshold, |baseline| baseline + EPSILON)
}

fn problem(score: &Score, was: Option<f64>, threshold: f64) -> String {
    let head = format!(
        "crap: {} {} CRAP {:.2}",
        score.file, score.function, score.crap
    );
    match was {
        None => format!("{head} is above the threshold {threshold} (new function)"),
        Some(baseline) if baseline <= threshold => {
            format!("{head} rose above the threshold {threshold} (baseline {baseline:.2})")
        }
        Some(baseline) => format!(
            "{head} rose above its baseline {baseline:.2}; it was over the threshold \
             {threshold} already, and the ratchet allows no rise"
        ),
    }
}

/// The report with its entries sorted by file, function and line, so a
/// tracked baseline diffs by function. Fails when it is not a report.
pub(crate) fn baseline_json(report: &str) -> Result<(String, usize), String> {
    let mut doc: Value =
        serde_json::from_str(report).map_err(|err| format!("the report is not JSON: {err}"))?;
    let entries = doc
        .get_mut("entries")
        .and_then(Value::as_array_mut)
        .ok_or("the report has no entries")?;
    entries.sort_by_key(|entry| {
        (
            entry
                .get("file")
                .and_then(Value::as_str)
                .map(str::to_string),
            entry
                .get("function")
                .and_then(Value::as_str)
                .map(str::to_string),
            entry.get("line").and_then(Value::as_u64),
        )
    });
    let count = entries.len();
    let text = serde_json::to_string_pretty(&doc).map_err(|err| err.to_string())?;
    Ok((format!("{text}\n"), count))
}

#[cfg(test)]
mod tests {
    use super::{baseline_json, judge, production_scores, Score, EPSILON};
    use crate::exemptions::Exemption;
    use crate::scope::Scope;
    use std::path::Path;

    fn score(file: &str, function: &str, crap: f64) -> Score {
        Score {
            file: file.to_string(),
            function: function.to_string(),
            crap,
        }
    }

    #[test]
    fn the_report_keeps_only_production_functions_of_the_crate() {
        let dir = Path::new("/w");
        let scope = Scope::new(dir, &dir.join("crates/tool"), &[]);
        let report = r#"{"entries":[
            {"file":"./crates/tool/src/lib.rs","function":"parse","crap":7.5},
            {"file":"./crates/tool/src/tests/a.rs","function":"helper","crap":42.0},
            {"file":"./crates/other/src/lib.rs","function":"parse","crap":9.0},
            {"file":"./crates/tool/src/lib.rs","function":"nameless"},
            {"file":"./crates/tool/src/lib.rs","crap":1.0}
        ]}"#;
        assert_eq!(
            production_scores(report, &scope),
            Ok(vec![score("src/lib.rs", "parse", 7.5)])
        );
        assert!(
            production_scores("{}", &scope).is_err_and(|err| err == "the report has no entries")
        );
        assert!(production_scores("stub", &scope)
            .is_err_and(|err| err.starts_with("the report is not JSON")));
    }

    #[test]
    fn with_no_baseline_every_function_is_new_and_held_to_the_threshold() {
        let current = [score("src/a.rs", "at", 6.0), score("src/a.rs", "over", 6.5)];
        assert_eq!(
            judge(&current, None, 6.0, &[]),
            ["crap: src/a.rs over CRAP 6.50 is above the threshold 6 (new function)"]
        );
    }

    #[test]
    fn a_function_the_baseline_lacks_is_new() {
        let baseline = [score("src/a.rs", "old", 30.0)];
        let current = [
            score("src/a.rs", "old", 30.0),
            score("src/b.rs", "old", 7.0),
            score("src/a.rs", "fresh", 8.0),
        ];
        assert_eq!(
            judge(&current, Some(&baseline), 6.0, &[]),
            [
                "crap: src/b.rs old CRAP 7.00 is above the threshold 6 (new function)",
                "crap: src/a.rs fresh CRAP 8.00 is above the threshold 6 (new function)",
            ]
        );
    }

    #[test]
    fn a_function_under_the_threshold_may_not_rise_above_it() {
        let baseline = [
            score("src/a.rs", "f", 5.0),
            score("src/a.rs", "g", 5.0),
            score("src/a.rs", "at", 6.0),
        ];
        let current = [
            score("src/a.rs", "f", 6.0),
            score("src/a.rs", "g", 6.2),
            score("src/a.rs", "at", 6.008),
        ];
        assert_eq!(
            judge(&current, Some(&baseline), 6.0, &[]),
            [
                "crap: src/a.rs g CRAP 6.20 rose above the threshold 6 (baseline 5.00)",
                "crap: src/a.rs at CRAP 6.01 rose above the threshold 6 (baseline 6.00)",
            ]
        );
    }

    #[test]
    fn a_function_over_the_threshold_may_only_move_down() {
        let baseline = [
            score("src/a.rs", "same", 32.0),
            score("src/a.rs", "noise", 32.0),
            score("src/a.rs", "worse", 32.0),
            score("src/a.rs", "better", 32.0),
            score("src/a.rs", "twice", 9.0),
            score("src/a.rs", "twice", 12.0),
        ];
        let current = [
            score("src/a.rs", "same", 32.0),
            score("src/a.rs", "noise", 32.0 + EPSILON),
            score("src/a.rs", "worse", 32.02),
            score("src/a.rs", "better", 20.0),
            score("src/a.rs", "twice", 12.0),
        ];
        assert_eq!(
            judge(&current, Some(&baseline), 6.0, &[]),
            ["crap: src/a.rs worse CRAP 32.02 rose above its baseline 32.00; it was over the threshold 6 already, and the ratchet allows no rise"]
        );
    }

    #[test]
    fn a_live_exemption_takes_a_function_out_of_both_rules() {
        let exempt = [Exemption {
            path: "src/main.rs".to_string(),
            symbol: Some("main".to_string()),
            expires: "2030-01-01".to_string(),
        }];
        let current = [
            score("src/main.rs", "main", 29.0),
            score("src/main.rs", "other", 9.0),
        ];
        assert_eq!(
            judge(&current, None, 6.0, &exempt),
            ["crap: src/main.rs other CRAP 9.00 is above the threshold 6 (new function)"]
        );
    }

    #[test]
    fn a_baseline_is_the_report_sorted_by_function() {
        let report = r#"{"version":"0.9.0","entries":[
            {"file":"./src/b.rs","function":"z","line":1,"crap":9.0},
            {"file":"./src/a.rs","function":"y","line":9,"crap":2.0},
            {"file":"./src/a.rs","function":"y","line":3,"crap":1.0}
        ]}"#;
        let (text, count) = baseline_json(report).expect("a report");
        assert_eq!(count, 3);
        let doc: serde_json::Value = serde_json::from_str(&text).expect("json");
        let lines: Vec<u64> = doc
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .expect("entries")
            .iter()
            .filter_map(|e| e.get("line").and_then(serde_json::Value::as_u64))
            .collect();
        assert_eq!(lines, [3, 9, 1]);
        assert_eq!(
            doc.get("version").and_then(serde_json::Value::as_str),
            Some("0.9.0")
        );
        assert!(text.ends_with("}\n"), "{text}");
        assert!(baseline_json("[]").is_err_and(|err| err == "the report has no entries"));
        assert!(baseline_json("x").is_err_and(|err| err.starts_with("the report is not JSON")));
    }
}
