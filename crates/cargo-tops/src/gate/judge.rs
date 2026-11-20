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
use crate::{config_path, floors, laws, profiles, steps};

/// What the judging found.
pub(crate) struct Judgement {
    /// One line per problem. Any problem fails the gate.
    pub(crate) failed: Vec<String>,
    /// The `laws evaluated:` line.
    pub(crate) evaluated: String,
    /// The `count:` line.
    pub(crate) count: String,
    /// Commands that ran, shared layers excluded.
    pub(crate) ran: usize,
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
        return Ok(dry_judgement(plan, done, total));
    }
    let inputs = Inputs {
        dir,
        cfg,
        plan,
        done,
    };
    let mut failed = run_problems(done);
    // The coverage floors and the CRAP ratchet, on the reports just written.
    let measured = floors::judge(dir, cfg, &done.runs, &plan.report);
    failed.extend(measured.problems.iter().cloned());
    let laws = judge_laws(&inputs, prepared, total, &mut failed)?;
    Ok(finish(&inputs, failed, measured, laws, total))
}

/// Nothing ran, so the count is short for every layer: a dry run lists the
/// commands and is never a pass.
fn dry_judgement(plan: &GatePlan<'_>, done: &Executed<'_>, total: usize) -> Judgement {
    let (count, _) = command_count(0, &plan.enabled, &[], false, plan.max_tier, &done.shared);
    let mut failed = done.not_run.clone();
    failed.push(count.clone());
    Judgement {
        failed,
        evaluated: format!("laws evaluated: 0/{total} (dry run: bend did not run)"),
        count,
        ran: 0,
        vacuous: 0,
        measured: floors::Judged::default(),
        laws_seconds: 0.0,
        bend_ran: false,
        laws_failed: false,
        environment: false,
        law_total: total,
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
) -> Judgement {
    let (plan, done) = (inputs.plan, inputs.done);
    // A layer that took another's run ran no command of its own.
    let ran = done.runs.len() - done.shared.len();
    let vacuous = done
        .runs
        .iter()
        .filter(|run| run.outcome == Outcome::Vacuous)
        .count();
    let (count, short) = command_count(
        ran_commands(&done.runs, laws.ran, &done.shared),
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
        ran,
        vacuous,
        measured,
        laws_seconds: laws.seconds,
        bend_ran: laws.ran,
        laws_failed: laws.failed,
        environment: done
            .runs
            .iter()
            .any(|run| run.outcome == Outcome::ToolMissing),
        law_total,
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
        .filter(|run| run.outcome != Outcome::ToolMissing && run.ran())
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
                .any(|run| run.layer == *id && run.outcome != Outcome::ToolMissing && run.ran()),
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
        Outcome::Failed => Some(match &run.note {
            Some(note) => format!("{}: {note}", run.layer),
            None => format!("{}: `{}` failed", run.layer, run.step),
        }),
        Outcome::ToolMissing => {
            let missing = steps::MissingTool {
                binary: steps::tool_binary(std::slice::from_ref(&run.step))
                    .unwrap_or_else(|| run.step.clone()),
            };
            Some(format!("{}: {missing}", run.layer))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::clear_cases;
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cargo-tops-judge-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn a_cases_file_from_an_earlier_gate_is_removed() {
        let dir = scratch("cases-old");
        let cases = dir.join("cases.json");
        fs::write(&cases, "{}").expect("write");
        assert!(clear_cases(&cases).is_ok());
        assert!(!cases.exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_cases_file_is_nothing_to_clear() {
        let dir = scratch("cases-none");
        assert!(clear_cases(&dir.join("cases.json")).is_ok());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_cases_path_that_cannot_be_removed_is_an_environment_fault() {
        let dir = scratch("cases-stuck");
        let cases = dir.join("cases.json");
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
        fs::remove_dir_all(&dir).ok();
    }
}
