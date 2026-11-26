//! Turn what ran into verdicts: the floors, the CRAP ratchet, the laws, the
//! command count and the list of problems.
use std::fs;
use std::path::Path;
use std::time::Instant;

use std::fmt::Write as _;

use super::execute::{Executed, Prepared};
use super::plan::GatePlan;
use super::{Fault, Mode};
use crate::config::{self, TopsConfig};
use crate::facts::{self, Outcome, Run};
use crate::mutation::Ledger;
use crate::scope::crate_root;
use crate::{config_path, exemptions, floors, laws, mutation, profiles, steps};

/// What the judging found.
pub(crate) struct Judgement {
    /// One line per problem. Any problem fails the gate.
    pub(crate) failed: Vec<String>,
    /// The `laws evaluated:` line.
    pub(crate) evaluated: String,
    /// The `count:` line.
    pub(crate) count: String,
    /// Commands that ran: a missing tool ran none, `bend` is one.
    pub(crate) commands: usize,
    pub(crate) vacuous: usize,
    /// The numbers the coverage floors and the CRAP ratchet judged.
    pub(crate) measured: floors::Judged,
    pub(crate) laws_seconds: f64,
    pub(crate) bend_ran: bool,
    pub(crate) laws_failed: bool,
    /// A command's tool was missing: the verdict rests on the environment.
    pub(crate) environment: bool,
    /// The laws in the pack, checked or not.
    pub(crate) law_total: usize,
    /// The survivors `cargo mutants` left, sorted by the records that cover
    /// them, when it finished.
    pub(crate) survivors: Option<Survivors>,
}

impl Judgement {
    /// Whether a floor, or the survivor maximum, of layer `id` was missed.
    pub(crate) fn missed(&self, id: &str) -> bool {
        match id {
            "coverage" => self.measured.coverage_problems > 0,
            "crap" => self.measured.crap_problems > 0,
            "mutation" => self
                .survivors
                .as_ref()
                .is_some_and(|found| !found.ledger.passes(found.max)),
            _ => false,
        }
    }
}

/// The mutation ledger and the maximum it was judged against.
pub(crate) struct Survivors {
    pub(crate) ledger: Ledger,
    pub(crate) max: i64,
}

/// What `judge` reads.
struct Inputs<'p, 'a> {
    dir: &'p Path,
    cfg: &'p TopsConfig,
    plan: &'p GatePlan<'a>,
    done: &'p Executed<'a>,
}

/// What the laws layer did.
struct LawsOutcome {
    evaluated: String,
    ran: bool,
    failed: bool,
    seconds: f64,
}

/// The laws in the adopter's pack, or in the one this release ships.
fn law_total(dir: &Path) -> usize {
    fs::read_to_string(dir.join(laws::PACK))
        .map(|pack| facts::pack_law_count(&pack))
        .unwrap_or_else(|_| facts::pack_law_count(profiles::LAWS_BEND))
}

/// Judge `done`. A dry run judges nothing: it is never a pass.
pub(crate) fn judge(
    dir: &Path,
    cfg: &TopsConfig,
    plan: &GatePlan<'_>,
    done: &Executed<'_>,
    prepared: Prepared,
    mode: Mode,
) -> Result<Judgement, Fault> {
    let total = law_total(dir);
    if mode == Mode::DryRun {
        return Ok(dry_judgement(cfg, plan, done, total));
    }
    let inputs = Inputs {
        dir,
        cfg,
        plan,
        done,
    };
    let mut failed = run_problems(done);
    let survivors = judge_survivors(&inputs, &mut failed);
    // The coverage floors and the CRAP ratchet, on the reports just written.
    let measured = floors::judge(dir, cfg, &done.runs, &plan.report);
    failed.extend(measured.problems.iter().cloned());
    add_expired(cfg, &mut failed);
    if let Some(problem) = cfg.expired_advisory(&exemptions::today()) {
        failed.push(problem);
    }
    let laws = judge_laws(&inputs, prepared, total, &mut failed)?;
    Ok(finish(&inputs, failed, measured, laws, total, survivors))
}

/// One problem for every exemption record that has expired, or whose date
/// is not a day, whatever its layer and whether or not the layer ran (§19).
pub(crate) fn expired_problems(cfg: &TopsConfig) -> Vec<String> {
    let today = exemptions::today();
    let records = cfg.exemptions.as_ref();
    let mut layers: Vec<&str> = Vec::new();
    for layer in records
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("layer")?.as_str())
    {
        if !layers.contains(&layer) {
            layers.push(layer);
        }
    }
    layers
        .into_iter()
        .flat_map(|layer| {
            exemptions::for_layer(records, layer)
                .into_iter()
                .filter(|record| !record.live(&today))
                .map(|record| record.expired_problem(layer))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Add each expired record to `failed`. A record a floor already named is
/// not named twice.
fn add_expired(cfg: &TopsConfig, failed: &mut Vec<String>) {
    for problem in expired_problems(cfg) {
        if !failed.contains(&problem) {
            failed.push(problem);
        }
    }
}

/// The crate's directory under the workspace root, which is what the
/// `file` of a mutant is relative to. Empty for a crate at the root.
fn crate_prefix(dir: &Path, name: &str) -> String {
    let Some(root) = laws::workspace_root(dir) else {
        return String::new();
    };
    crate_root(dir, name)
        .strip_prefix(&root)
        .map(|rest| rest.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The survivors of this run's `cargo mutants`, matched to the live
/// `layer: mutation` records, and the problem when more than
/// `unexplained_survivors_max` are left. An outcomes file that could not be
/// read is already the layer's failure.
fn judge_survivors(inputs: &Inputs<'_, '_>, failed: &mut Vec<String>) -> Option<Survivors> {
    let Some(Ok(found)) = &inputs.done.found else {
        return None;
    };
    let cfg = inputs.cfg;
    let today = exemptions::today();
    let mut live = exemptions::for_layer(cfg.exemptions.as_ref(), "mutation");
    live.retain(|record| record.live(&today));
    let prefix = crate_prefix(inputs.dir, &cfg.crate_.name);
    let max = cfg.gates.mutation.unexplained_survivors_max;
    let ledger = mutation::explain(found, &live, &prefix);
    failed.extend(ledger.problem(max));
    Some(Survivors { ledger, max })
}

/// Nothing ran, so the count is short for every layer: a dry run lists the
/// commands and is never a pass.
fn dry_judgement(
    cfg: &TopsConfig,
    plan: &GatePlan<'_>,
    done: &Executed<'_>,
    total: usize,
) -> Judgement {
    let (count, _) = command_count(0, &plan.enabled, &[], false, plan.max_tier, &done.shared);
    let mut failed = done.not_run.clone();
    failed.push(count.clone());
    add_expired(cfg, &mut failed);
    if let Some(problem) = cfg.expired_advisory(&exemptions::today()) {
        failed.push(problem);
    }
    Judgement {
        failed,
        evaluated: format!("laws evaluated: 0/{total} (dry run: bend did not run)"),
        count,
        commands: 0,
        vacuous: 0,
        measured: floors::Judged::default(),
        laws_seconds: 0.0,
        bend_ran: false,
        laws_failed: false,
        environment: false,
        law_total: total,
        survivors: None,
    }
}

/// A layer that never ran, and every run that did not pass.
fn run_problems(done: &Executed<'_>) -> Vec<String> {
    let mut failed: Vec<String> = done.not_run.clone();
    failed.extend(done.runs.iter().filter_map(|run| {
        match done.shared.iter().find(|(layer, _)| *layer == run.layer) {
            Some((_, from)) if run.note.is_none() => run.outcome.fails_the_gate().then(|| {
                format!(
                    "{}: no command of its own; the {from} run it took is {}",
                    run.layer, run.outcome
                )
            }),
            _ => run_failure(run),
        }
    }));
    failed
}

/// The count line, the short-count problem and the totals.
fn finish(
    inputs: &Inputs<'_, '_>,
    mut failed: Vec<String>,
    measured: floors::Judged,
    laws: LawsOutcome,
    law_total: usize,
    survivors: Option<Survivors>,
) -> Judgement {
    let (plan, done) = (inputs.plan, inputs.done);
    // The `count:` line, `ok:` line and closing `fail:` line say the same
    // number: commands that ran.
    let commands = ran_commands(&done.runs, laws.ran, &done.shared);
    let vacuous = done
        .runs
        .iter()
        .filter(|run| run.outcome == Outcome::Vacuous)
        .count();
    let (count, short) = command_count(
        commands,
        &plan.enabled,
        &done.runs,
        laws.ran,
        plan.max_tier,
        &done.shared,
    );
    if short {
        failed.push(count.clone());
    }
    Judgement {
        failed,
        evaluated: laws.evaluated,
        count,
        commands,
        vacuous,
        measured,
        laws_seconds: laws.seconds,
        bend_ran: laws.ran,
        laws_failed: laws.failed,
        environment: done.runs.iter().any(|run| run.outcome.lacks_tool()),
        law_total,
        survivors,
    }
}

/// The verdict of a layer's runs: a failure outranks a missing tool, which
/// outranks a tool at another version than its pin, which outranks a vacuous
/// run.
fn runs_verdict(runs: &[&Run]) -> &'static str {
    let any = |outcome: Outcome| runs.iter().any(|run| run.outcome == outcome);
    if any(Outcome::Failed) {
        "fail"
    } else if any(Outcome::ToolMissing) {
        "tool-missing"
    } else if any(Outcome::ToolMismatch) {
        "tool-mismatch"
    } else if any(Outcome::Vacuous) {
        "vacuous"
    } else {
        "pass"
    }
}

/// The verdict of a layer enabled above `--max-tier`: it is not judged.
pub(crate) fn above_range_verdict() -> &'static str {
    "not-evaluated"
}

/// The verdict of a row the run never reached (a fault before it ran).
pub(crate) fn unreached_verdict() -> &'static str {
    "not-run"
}

/// The verdict of the laws layer: `not-run` when `bend` did not run.
pub(crate) fn laws_verdict(judged: Option<&Judgement>) -> &'static str {
    match judged.filter(|judged| judged.bend_ran) {
        None => "not-run",
        Some(judged) if judged.laws_failed => "fail",
        Some(_) => "pass",
    }
}

/// The row verdict of layer `id`: `not-run` when nothing ran for it,
/// otherwise its worst run. A run that passed still fails the row when its
/// floors, or its survivors, did not.
pub(crate) fn layer_verdict(
    done: &Executed<'_>,
    judged: Option<&Judgement>,
    id: &str,
) -> &'static str {
    let runs: Vec<&Run> = done.runs.iter().filter(|run| run.layer == id).collect();
    if runs.is_empty() {
        return "not-run";
    }
    match runs_verdict(&runs) {
        "pass" if judged.is_some_and(|judged| judged.missed(id)) => "fail",
        verdict => verdict,
    }
}

/// A report from an earlier gate must not be counted for this one.
fn clear_cases(cases: &Path) -> Result<(), Fault> {
    match fs::remove_file(cases) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(Fault::env(
            anyhow::Error::new(err).context(format!("remove {}", cases.display())),
        )),
        _ => Ok(()),
    }
}

/// What the fact readers of the laws layer see.
fn laws_ctx<'i>(inputs: &Inputs<'i, '_>) -> facts::Ctx<'i> {
    let Inputs {
        dir,
        cfg,
        plan,
        done,
    } = *inputs;
    facts::Ctx {
        dir,
        cfg,
        yaml: fs::read_to_string(config_path(dir))
            .ok()
            .and_then(|raw| facts::parse_yaml(&raw).ok())
            .unwrap_or_default(),
        max_tier: plan.max_tier,
        runs: &done.runs,
        artifacts: &plan.report,
        started: done.started,
        workflows: facts::load_workflows(dir),
    }
}

/// The laws layer: `bend` on the reports the other layers wrote.
fn judge_laws(
    inputs: &Inputs<'_, '_>,
    prepared: Prepared,
    total: usize,
    failed: &mut Vec<String>,
) -> Result<LawsOutcome, Fault> {
    let Some(judge) = prepared.judge else {
        return Ok(LawsOutcome {
            evaluated: format!("laws evaluated: 0/{total} (laws layer off)"),
            ran: false,
            failed: false,
            seconds: 0.0,
        });
    };
    let cases = inputs.plan.report.join("cases.json");
    clear_cases(&cases)?;
    let ctx = laws_ctx(inputs);
    let clock = Instant::now();
    let problems = laws::run(&judge, &ctx).map_err(Fault::env)?;
    let seconds = clock.elapsed().as_secs_f64();
    let outcome_failed = !problems.is_empty();
    failed.extend(problems);
    // `cases.json` was removed above, so one here means bend ran.
    let report = laws_evaluated(&cases);
    let (checked, of) = report.unwrap_or((0, total));
    Ok(LawsOutcome {
        evaluated: format!("laws evaluated: {checked}/{of}"),
        ran: report.is_some(),
        failed: outcome_failed,
        seconds,
    })
}

/// Commands that ran: each run whose tool was found, and `bend` once when it
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "already in this crate when the strict table was turned on"
)]
/// judged the laws layer. A layer that took another layer's run ran none.
pub(crate) fn ran_commands(runs: &[Run], bend_ran: bool, shared: &[(&str, &str)]) -> usize {
    runs.iter()
        .filter(|run| !run.outcome.lacks_tool() && run.ran())
        .filter(|run| !shared.iter().any(|(layer, _)| *layer == run.layer))
        .count()
        + usize::from(bend_ran)
}

/// The `count:` line, and whether it is short: an enabled layer in range
/// ran no command (NotRun, a missing tool, or a dry run). Every run belongs
/// to one enabled layer, so fewer commands than layers is always short. A
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "already in this crate when the strict table was turned on"
)]
/// short count fails the gate.
pub(crate) fn command_count(
    ran: usize,
    enabled: &[&config::Layer],
    runs: &[Run],
    bend_ran: bool,
    max_tier: u8,
    shared: &[(&str, &str)],
) -> (String, bool) {
    let none: Vec<&str> = enabled
        .iter()
        .map(|layer| layer.id.as_str())
        .filter(|id| match *id {
            "laws" => !bend_ran,
            _ => !runs
                .iter()
                .any(|run| run.layer == *id && !run.outcome.lacks_tool() && run.ran()),
        })
        .collect();
    let mut line = format!(
        "count: {ran} command(s) ran; {} of {} enabled layer(s) at tier <= {max_tier} ran one",
        enabled.len() - none.len(),
        enabled.len()
    );
    // A layer that took another's run is counted when that run happened,
    // with the run named.
    for from in ["coverage", "unit"] {
        let took: Vec<&str> = shared
            .iter()
            .filter(|(_, source)| *source == from)
            .map(|(layer, _)| *layer)
            .collect();
        if !took.is_empty() {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "already in this crate when the strict table was turned on"
            )]
            let _ = write!(line, "; {} took the {from} run", took.join(", "));
        }
    }
    if none.is_empty() {
        (line, false)
    } else {
        (format!("{line}; none ran for: {}", none.join(", ")), true)
    }
}

/// `(checked, total)` from the `cases.json` the laws layer just wrote. A law
/// is evaluated when `bend` got a case for it.
pub(crate) fn laws_evaluated(cases: &Path) -> Option<(usize, usize)> {
    let raw = fs::read_to_string(cases).ok()?;
    let report: serde_json::Value = serde_json::from_str(&raw).ok()?;
    #[expect(
        clippy::indexing_slicing,
        reason = "already in this crate when the strict table was turned on"
    )]
    let laws = report["laws"].as_array()?;
    let checked = laws.iter().filter(|law| law["status"] == "checked").count();
    Some((checked, laws.len()))
}

pub(crate) fn run_failure(run: &Run) -> Option<String> {
    match run.outcome {
        Outcome::Passed | Outcome::Vacuous => None,
        Outcome::Failed | Outcome::ToolMismatch | Outcome::ToolMissing => {
            Some(format!("{}: {}", run.layer, failure_words(run)))
        }
    }
}

/// What failed, in the words the run kept (a note), or for a command whose
/// tool is not on `PATH` the way that is said, or the step that failed.
fn failure_words(run: &Run) -> String {
    match (&run.note, run.outcome) {
        (Some(note), _) => note.clone(),
        (None, Outcome::ToolMissing) => steps::MissingTool {
            binary: steps::tool_binary(std::slice::from_ref(&run.step))
                .unwrap_or_else(|| run.step.clone()),
        }
        .to_string(),
        (None, _) => format!("`{}` failed", run.step),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::Scratch;
    use super::{clear_cases, crate_prefix, laws_verdict, Judgement};
    use crate::floors;
    use std::fs;

    #[test]
    fn a_cases_file_from_an_earlier_gate_is_removed() {
        let scratch = Scratch::new("cases-old");
        let cases = scratch.0.join("cases.json");
        fs::write(&cases, "{}").expect("write");
        assert!(clear_cases(&cases).is_ok());
        assert!(!cases.exists());
    }

    #[test]
    fn no_cases_file_is_nothing_to_clear() {
        let scratch = Scratch::new("cases-none");
        assert!(clear_cases(&scratch.0.join("cases.json")).is_ok());
    }

    #[test]
    fn a_cases_path_that_cannot_be_removed_is_an_environment_fault() {
        let scratch = Scratch::new("cases-stuck");
        let cases = scratch.0.join("cases.json");
        fs::create_dir_all(cases.join("inside")).expect("mkdir");
        let Err(fault) = clear_cases(&cases) else {
            panic!("a directory is not a report");
        };
        assert!(!fault.config && !fault.announced);
        assert!(
            format!("{:#}", fault.error).contains("remove"),
            "{:#}",
            fault.error
        );
    }

    #[test]
    fn a_crate_under_the_workspace_root_is_named_by_its_directory() {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace");
        assert_eq!(crate_prefix(&workspace, "cargo-tops"), "crates/cargo-tops");
        // No such package, or no cargo project here: the gate directory.
        assert_eq!(crate_prefix(&workspace, "no-such-package"), "");
        let empty = Scratch::new("prefix-none");
        assert_eq!(crate_prefix(&empty.0, "cargo-tops"), "");
    }

    /// A judgement of a run in which the laws layer did not run.
    fn quiet() -> Judgement {
        Judgement {
            failed: Vec::new(),
            evaluated: String::new(),
            count: String::new(),
            commands: 0,
            vacuous: 0,
            measured: floors::Judged::default(),
            laws_seconds: 0.0,
            bend_ran: false,
            laws_failed: false,
            environment: false,
            law_total: 0,
            survivors: None,
        }
    }

    #[test]
    fn the_laws_row_is_not_run_unless_bend_ran_and_then_pass_or_fail() {
        assert_eq!(laws_verdict(None), "not-run");
        assert_eq!(laws_verdict(Some(&quiet())), "not-run");
        let failed_without_bend = Judgement {
            laws_failed: true,
            ..quiet()
        };
        assert_eq!(laws_verdict(Some(&failed_without_bend)), "not-run");
        let passed = Judgement {
            bend_ran: true,
            ..quiet()
        };
        assert_eq!(laws_verdict(Some(&passed)), "pass");
        let failed = Judgement {
            bend_ran: true,
            laws_failed: true,
            ..quiet()
        };
        assert_eq!(laws_verdict(Some(&failed)), "fail");
    }
}
