//! The survivors ledger: what `cargo mutants` left in `mutants.out/outcomes.json`.
//!
//! [`parse`] reads the file in the shape cargo-mutants [`PINNED`] writes and
//! refuses any other: a version it was not written for, a field it cannot
//! read, an outcome it does not know, or totals that disagree with the
//! outcomes. [`explain`] sorts the missed mutants into those a live
//! `layer: mutation` exemption covers (the rule of [`Exemption::covers`])
//! and those nothing covers. The gate passes the layer when the unexplained
//! count is at most `gates.mutation.unexplained_survivors_max`. Timeouts and
//! unviable mutants are counted and reported; they are not survivors.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::exemptions::Exemption;

/// Where cargo-mutants writes the run's outcomes, under the directory it ran in.
const OUTCOMES: &str = "mutants.out/outcomes.json";

/// The cargo-mutants release whose `outcomes.json` this reads.
pub(crate) const PINNED: &str = "27.1.0";

/// One mutant the suite did not catch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Missed {
    /// The file, as cargo-mutants names it: relative to the workspace root.
    pub file: String,
    /// The function the mutant is in, `module::name` for a nested one.
    pub function: Option<String>,
    pub line: u64,
    /// cargo-mutants' own name for the mutant, such as
    /// `src/lib.rs:8:5: replace double -> i32 with 0`.
    pub name: String,
}

/// What a run found, by kind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Outcomes {
    pub caught: usize,
    pub missed: Vec<Missed>,
    pub timeouts: usize,
    pub unviable: usize,
}

/// The missed mutants of a run, sorted by the records that cover them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Ledger {
    pub caught: usize,
    pub explained: Vec<Missed>,
    pub unexplained: Vec<Missed>,
    pub timeouts: usize,
    pub unviable: usize,
}

/// Remove the `outcomes.json` an earlier run left, so one found after
/// `cargo mutants` is the one it just wrote.
pub(crate) fn clear(dir: &Path) -> Result<()> {
    let path = dir.join(OUTCOMES);
    match fs::remove_file(&path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            Err(err).with_context(|| format!("remove {}", path.display()))
        }
        _ => Ok(()),
    }
}

/// The outcomes `cargo mutants` just wrote under `dir`. Call [`clear`] before
/// the run: a file that is there afterwards is this run's.
pub(crate) fn read(dir: &Path) -> Result<Outcomes, String> {
    let text = fs::read_to_string(dir.join(OUTCOMES))
        .map_err(|err| format!("no {OUTCOMES} after cargo mutants ran: {err}"))?;
    parse(&text)
}

fn field<'v>(value: &'v Value, key: &str) -> Result<&'v Value, String> {
    value.get(key).ok_or_else(|| {
        format!("outcomes.json has no `{key}` where cargo-mutants {PINNED} writes one")
    })
}

fn count(value: &Value, key: &str) -> Result<usize, String> {
    field(value, key)?
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| format!("outcomes.json `{key}` is not a count"))
}

/// The missed mutant a `Mutant` scenario describes.
fn missed(scenario: &Value) -> Result<Missed, String> {
    let mutant = field(scenario, "Mutant")?;
    let function = match field(mutant, "function")? {
        Value::Null => None,
        named => Some(
            field(named, "function_name")?
                .as_str()
                .ok_or("outcomes.json `function_name` is not a string")?
                .to_string(),
        ),
    };
    let line = field(field(field(mutant, "span")?, "start")?, "line")?
        .as_u64()
        .ok_or("outcomes.json span `line` is not a number")?;
    let text = |key: &str| -> Result<String, String> {
        Ok(field(mutant, key)?
            .as_str()
            .ok_or_else(|| format!("outcomes.json `{key}` is not a string"))?
            .to_string())
    };
    Ok(Missed {
        file: text("file")?,
        function,
        line,
        name: text("name")?,
    })
}

/// What one outcome says happened to its mutant.
#[derive(Clone, Copy)]
enum Kind {
    Caught,
    Missed,
    Timeout,
    Unviable,
}

/// The scenario and the summary of one outcome.
fn parts(outcome: &Value) -> Result<(&Value, &str), String> {
    let scenario = field(outcome, "scenario")?;
    let summary = field(outcome, "summary")?
        .as_str()
        .ok_or("outcomes.json `summary` is not a string")?;
    Ok((scenario, summary))
}

/// The baseline run is not a mutant: it must have succeeded.
fn baseline(summary: &str) -> Result<(), String> {
    match summary {
        "Success" => Ok(()),
        other => Err(format!("the baseline run ended as `{other}`")),
    }
}

fn kind(summary: &str) -> Result<Kind, String> {
    match summary {
        "CaughtMutant" => Ok(Kind::Caught),
        "MissedMutant" => Ok(Kind::Missed),
        "Timeout" => Ok(Kind::Timeout),
        "Unviable" => Ok(Kind::Unviable),
        other => Err(format!(
            "outcomes.json has an outcome `{other}` this gate does not know"
        )),
    }
}

impl Outcomes {
    fn add(&mut self, kind: Kind, scenario: &Value) -> Result<(), String> {
        match kind {
            Kind::Caught => self.caught += 1,
            Kind::Missed => self.missed.push(missed(scenario)?),
            Kind::Timeout => self.timeouts += 1,
            Kind::Unviable => self.unviable += 1,
        }
        Ok(())
    }
}

/// Add one outcome to `found`.
fn take(found: &mut Outcomes, outcome: &Value) -> Result<(), String> {
    let (scenario, summary) = parts(outcome)?;
    if scenario.as_str() == Some("Baseline") {
        return baseline(summary);
    }
    found.add(kind(summary)?, scenario)
}

/// Read `outcomes.json`. Any shape but the pinned one is an `Err` saying
/// what was not as expected, never a pass.
pub(crate) fn parse(text: &str) -> Result<Outcomes, String> {
    let doc: Value =
        serde_json::from_str(text).map_err(|err| format!("outcomes.json is not JSON: {err}"))?;
    let version = field(&doc, "cargo_mutants_version")?
        .as_str()
        .unwrap_or_default();
    if version != PINNED {
        return Err(format!(
            "outcomes.json is from cargo-mutants {version:?}; this gate reads {PINNED}"
        ));
    }
    let list = field(&doc, "outcomes")?
        .as_array()
        .ok_or("outcomes.json `outcomes` is not a list")?;
    let mut found = Outcomes::default();
    for outcome in list {
        take(&mut found, outcome)?;
    }
    let totals = [
        ("caught", found.caught),
        ("missed", found.missed.len()),
        ("timeout", found.timeouts),
        ("unviable", found.unviable),
    ];
    for (key, seen) in totals {
        let said = count(&doc, key)?;
        if said != seen {
            return Err(format!("outcomes.json says {said} {key} but lists {seen}"));
        }
    }
    Ok(found)
}

/// `file` relative to the crate root, given the crate's path under the
/// workspace root (`""` for a crate at the root).
fn relative<'f>(file: &'f str, crate_prefix: &str) -> &'f str {
    let prefix = crate_prefix.trim_matches('/');
    if prefix.is_empty() {
        return file;
    }
    file.strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(file)
}

/// Sort the missed mutants. `live` are the layer's records that have not
/// expired; `crate_prefix` is the crate's directory under the workspace root,
/// which the records' globs are relative to.
pub(crate) fn explain(found: &Outcomes, live: &[Exemption], crate_prefix: &str) -> Ledger {
    let (explained, unexplained) = found.missed.iter().cloned().partition(|mutant: &Missed| {
        let rel = relative(&mutant.file, crate_prefix);
        live.iter()
            .any(|record| record.covers(rel, mutant.function.as_deref()))
    });
    Ledger {
        caught: found.caught,
        explained,
        unexplained,
        timeouts: found.timeouts,
        unviable: found.unviable,
    }
}

impl Ledger {
    /// Whether the layer passes: at most `max` survivors nothing explains.
    pub(crate) fn passes(&self, max: i64) -> bool {
        i64::try_from(self.unexplained.len()).is_ok_and(|n| n <= max)
    }

    /// The problem that names each unexplained survivor, or `None` when the
    /// layer passes. Explained survivors are never named.
    pub(crate) fn problem(&self, max: i64) -> Option<String> {
        if self.passes(max) {
            return None;
        }
        let named: Vec<String> = self
            .unexplained
            .iter()
            .map(|mutant| format!("{}:{} {}", mutant.file, mutant.line, mutant.name))
            .collect();
        Some(format!(
            "mutation: {} unexplained survivor(s), above unexplained_survivors_max {max}: {}",
            named.len(),
            named.join("; ")
        ))
    }

    /// The numbers the mutation row's `judged` carries.
    pub(crate) fn to_json(&self, max: i64) -> Value {
        json!({
            "caught": self.caught,
            "missed": self.explained.len() + self.unexplained.len(),
            "explained": self.explained.len(),
            "unexplained": self.unexplained.len(),
            "timeouts": self.timeouts,
            "unviable": self.unviable,
            "unexplained_survivors_max": max,
        })
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "the tests edit and read JSON they built from the recorded fixture"
)]
mod tests {
    use super::{clear, explain, parse, read, relative, Outcomes, PINNED};
    use crate::exemptions::Exemption;
    use serde_json::Value;
    use std::fs;

    /// A real run of cargo-mutants 27.1.0 over a small crate: 11 caught, 10
    /// missed (5 in `double`, 5 in `inner::triple`), 1 timeout, 1 unviable.
    type Edit = fn(&mut Value);

    const FIXTURE: &str = include_str!("../tests/fixtures/mutants-27.1.0/outcomes.json");

    fn record(path: &str, symbol: Option<&str>) -> Exemption {
        Exemption {
            path: path.to_string(),
            symbol: symbol.map(str::to_string),
            expires: "2099-01-01".to_string(),
        }
    }

    fn edited(change: impl FnOnce(&mut Value)) -> String {
        let mut doc: Value = serde_json::from_str(FIXTURE).expect("fixture is JSON");
        change(&mut doc);
        doc.to_string()
    }

    #[test]
    fn the_recorded_run_has_the_counts_cargo_mutants_reported() {
        let found = parse(FIXTURE).expect("the pinned shape parses");
        assert_eq!(found.caught, 11);
        assert_eq!(found.missed.len(), 10);
        assert_eq!(found.timeouts, 1);
        assert_eq!(found.unviable, 1);
        let first = found.missed.first().expect("a survivor");
        assert_eq!(first.file, "src/lib.rs");
        assert_eq!(first.function.as_deref(), Some("double"));
        assert_eq!(first.line, 8);
        assert_eq!(first.name, "src/lib.rs:8:5: replace double -> i32 with 0");
        let nested = found
            .missed
            .iter()
            .filter(|m| m.function.as_deref() == Some("inner::triple"))
            .count();
        assert_eq!(nested, 5);
    }

    #[test]
    fn another_release_is_refused_naming_both() {
        let text = edited(|doc| doc["cargo_mutants_version"] = "26.0.0".into());
        let err = parse(&text).expect_err("not the pinned release");
        assert!(err.contains("26.0.0") && err.contains(PINNED), "{err}");
        let text = edited(|doc| {
            doc.as_object_mut()
                .expect("object")
                .remove("cargo_mutants_version");
        });
        assert!(parse(&text).is_err());
    }

    #[test]
    fn an_unknown_shape_is_an_error_and_never_a_pass() {
        assert!(parse("").is_err());
        assert!(parse("[]").is_err());
        assert!(parse("{}").is_err());
        let cases: [(&str, Edit); 7] = [
            ("outcomes is not a list", |d| d["outcomes"] = "x".into()),
            ("an unknown summary", |d| {
                d["outcomes"][1]["summary"] = "Skipped".into()
            }),
            ("no scenario", |d| {
                d["outcomes"][1]
                    .as_object_mut()
                    .expect("object")
                    .remove("scenario");
            }),
            ("a baseline that failed", |d| {
                d["outcomes"][0]["summary"] = "Failure".into()
            }),
            ("no span", |d| {
                d["outcomes"][6]["scenario"]["Mutant"]
                    .as_object_mut()
                    .expect("object")
                    .remove("span");
            }),
            ("a missed total that disagrees", |d| d["missed"] = 3.into()),
            ("no caught total", |d| {
                d.as_object_mut().expect("object").remove("caught");
            }),
        ];
        for (what, change) in cases {
            let err = parse(&edited(change)).expect_err(what);
            assert!(!err.is_empty(), "{what}");
        }
    }

    #[test]
    fn a_missed_mutant_outside_a_function_has_no_function_name() {
        let text = edited(|doc| {
            let at = doc["outcomes"]
                .as_array()
                .expect("list")
                .iter()
                .position(|o| o["summary"] == "MissedMutant")
                .expect("a survivor");
            doc["outcomes"][at]["scenario"]["Mutant"]["function"] = Value::Null;
        });
        let found = parse(&text).expect("a null function is allowed");
        assert_eq!(found.missed.first().and_then(|m| m.function.clone()), None);
    }

    #[test]
    fn a_record_for_one_function_explains_only_its_survivors() {
        let found = parse(FIXTURE).expect("parses");
        let ledger = explain(&found, &[record("src/**", Some("double"))], "");
        assert_eq!(ledger.explained.len(), 5);
        assert_eq!(ledger.unexplained.len(), 5);
        assert!(ledger
            .unexplained
            .iter()
            .all(|m| m.function.as_deref() == Some("inner::triple")));
        assert!(!ledger.passes(0));
        assert!(ledger.passes(5));
        let problem = ledger.problem(0).expect("fails");
        assert!(
            problem.contains("5 unexplained") && problem.contains("inner::triple"),
            "{problem}"
        );
        assert!(!problem.contains(" double "), "{problem}");
    }

    #[test]
    fn a_symbol_matches_the_last_path_segment_and_a_symbol_free_record_covers_its_glob() {
        let found = parse(FIXTURE).expect("parses");
        let nested = explain(&found, &[record("src/lib.rs", Some("triple"))], "");
        assert_eq!(nested.explained.len(), 5);
        let both = [
            record("src/lib.rs", Some("double")),
            record("src/lib.rs", Some("inner::triple")),
        ];
        assert!(explain(&found, &both, "").unexplained.is_empty());
        let all = explain(&found, &[record("src/**", None)], "");
        assert_eq!(all.explained.len(), 10);
        assert!(all.passes(0) && all.problem(0).is_none());
        let elsewhere = explain(&found, &[record("other/**", None)], "");
        assert_eq!(elsewhere.unexplained.len(), 10);
    }

    #[test]
    fn globs_are_relative_to_the_crate_not_the_workspace() {
        let text = edited(|doc| {
            for outcome in doc["outcomes"].as_array_mut().expect("list") {
                if let Some(file) = outcome["scenario"]["Mutant"]["file"].as_str() {
                    let moved = format!("crates/sample/{file}");
                    outcome["scenario"]["Mutant"]["file"] = moved.into();
                }
            }
        });
        let found = parse(&text).expect("parses");
        // The record is relative to the crate: it names `src/lib.rs`, and the
        // workspace-relative file is `crates/sample/src/lib.rs`.
        let inside = [record("src/lib.rs", None)];
        assert_eq!(
            explain(&found, &inside, "crates/sample").explained.len(),
            10
        );
        assert_eq!(
            explain(&found, &inside, "crates/sample/").explained.len(),
            10
        );
        // A record that spells the workspace path matches only when no
        // prefix is stripped.
        let spelled = [record("crates/sample/src/lib.rs", None)];
        assert_eq!(explain(&found, &spelled, "").explained.len(), 10);
        assert_eq!(
            explain(&found, &spelled, "crates/sample").explained.len(),
            0
        );
        // A record for another crate's file explains nothing.
        let other = [record("crates/other/src/lib.rs", None)];
        assert_eq!(explain(&found, &other, "crates/sample").explained.len(), 0);
    }

    #[test]
    fn the_prefix_is_stripped_whole_segments_only() {
        assert_eq!(relative("crates/a/src/x.rs", "crates/a"), "src/x.rs");
        assert_eq!(
            relative("crates/ab/src/x.rs", "crates/a"),
            "crates/ab/src/x.rs"
        );
        assert_eq!(relative("src/x.rs", ""), "src/x.rs");
        assert_eq!(relative("src/x.rs", "/"), "src/x.rs");
    }

    #[test]
    fn the_judged_numbers_count_every_kind_and_the_max() {
        let found = parse(FIXTURE).expect("parses");
        let ledger = explain(&found, &[record("src/lib.rs", Some("double"))], "");
        let judged = ledger.to_json(2);
        assert_eq!(judged["caught"], 11);
        assert_eq!(judged["missed"], 10);
        assert_eq!(judged["explained"], 5);
        assert_eq!(judged["unexplained"], 5);
        assert_eq!(judged["timeouts"], 1);
        assert_eq!(judged["unviable"], 1);
        assert_eq!(judged["unexplained_survivors_max"], 2);
    }

    #[test]
    fn timeouts_and_unviable_mutants_are_not_survivors() {
        let only = Outcomes {
            caught: 0,
            missed: Vec::new(),
            timeouts: 4,
            unviable: 9,
        };
        let ledger = explain(&only, &[], "");
        assert!(ledger.passes(0) && ledger.problem(0).is_none());
        assert_eq!(ledger.timeouts, 4);
        assert_eq!(ledger.unviable, 9);
    }

    #[test]
    fn a_negative_max_is_never_met() {
        let ledger = explain(&Outcomes::default(), &[], "");
        assert!(!ledger.passes(-1));
    }

    /// The QA H4 shape, recorded in a workspace: `qa_h4_survivor_miss` has no
    /// test, so its two mutants survive, and `file` is relative to the
    /// workspace root, not to the crate.
    const PLANT: &str =
        include_str!("../tests/fixtures/mutants-27.1.0/plant-workspace/outcomes.json");

    #[test]
    fn the_h4_plant_has_two_survivors_in_a_crate_under_the_workspace() {
        let found = parse(PLANT).expect("parses");
        assert_eq!((found.caught, found.timeouts, found.unviable), (5, 0, 1));
        assert_eq!(found.missed.len(), 2);
        assert!(found
            .missed
            .iter()
            .all(|m| m.file == "crates/plant/src/qa_h4_plant.rs"
                && m.function.as_deref() == Some("qa_h4_survivor_miss")));
        let by_symbol = [record("src/qa_h4_plant.rs", Some("qa_h4_survivor_miss"))];
        assert_eq!(
            explain(&found, &by_symbol, "crates/plant").explained.len(),
            2
        );
        assert_eq!(explain(&found, &by_symbol, "").explained.len(), 2);
        let other = [record("src/qa_h4_plant.rs", Some("another"))];
        assert_eq!(explain(&found, &other, "crates/plant").unexplained.len(), 2);
        let wrong_file = [record("src/lib.rs", Some("qa_h4_survivor_miss"))];
        assert_eq!(
            explain(&found, &wrong_file, "crates/plant")
                .unexplained
                .len(),
            2
        );
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cargo-tops-mutation-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn an_outcomes_file_from_an_earlier_run_is_removed_and_none_is_nothing() {
        let dir = scratch("clear");
        assert!(clear(&dir).is_ok());
        fs::create_dir_all(dir.join("mutants.out")).expect("mkdir");
        let file = dir.join("mutants.out/outcomes.json");
        fs::write(&file, FIXTURE).expect("write");
        assert!(clear(&dir).is_ok());
        assert!(!file.exists());
        // A directory where the file should be cannot be removed as a file.
        fs::create_dir_all(file.join("inside")).expect("mkdir");
        let err = clear(&dir).expect_err("a directory is not a report");
        assert!(format!("{err:#}").contains("remove"), "{err:#}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn what_the_run_wrote_is_read_and_a_missing_file_is_an_error() {
        let dir = scratch("read");
        let err = read(&dir).expect_err("no file");
        assert!(err.contains("mutants.out/outcomes.json"), "{err}");
        fs::create_dir_all(dir.join("mutants.out")).expect("mkdir");
        fs::write(dir.join("mutants.out/outcomes.json"), "not json").expect("write");
        assert!(read(&dir).expect_err("not json").contains("not JSON"));
        fs::write(dir.join("mutants.out/outcomes.json"), FIXTURE).expect("write");
        assert_eq!(read(&dir).expect("parses").missed.len(), 10);
        fs::remove_dir_all(&dir).ok();
    }
}
