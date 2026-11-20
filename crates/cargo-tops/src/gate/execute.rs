//! Run a plan: each command in order, timed. A dry run lists the commands
//! and runs none.
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{ChildStderr, Command, ExitStatus, Stdio};
use std::time::{Instant, SystemTime};

use anyhow::{Context, Result};

use super::plan::{GatePlan, Unit, What};
use super::{Fault, Mode};
use crate::config::Layer;
use crate::evidence;
use crate::facts::{Outcome, Run};
use crate::lock::{self, Line, Verdict};
use crate::mutation::{self, Outcomes};
use crate::steps::{self, Base, Kind, MutationPlan, Step};
use crate::{laws, TOOL_LOCK};

/// What the laws layer needs before any work starts.
pub(crate) struct Prepared {
    /// The pinned Bend, when the laws layer is on and this is not a dry run.
    pub(crate) judge: Option<laws::Judge>,
}

/// When a layer's commands began, as wall time and as a stopwatch.
struct Clock {
    began: SystemTime,
    watch: Instant,
}

impl Clock {
    fn start() -> Self {
        Self {
            began: SystemTime::now(),
            watch: Instant::now(),
        }
    }
}

/// Wall time and tool version of one layer's commands.
pub(crate) struct Timing {
    pub(crate) layer: String,
    /// When its commands began: a report older than this is not theirs.
    pub(crate) began: SystemTime,
    pub(crate) seconds: f64,
    pub(crate) tool_version: Option<String>,
}

/// What the commands did.
pub(crate) struct Executed<'a> {
    pub(crate) started: SystemTime,
    pub(crate) runs: Vec<Run>,
    pub(crate) timing: Vec<Timing>,
    /// Layers that add no command, each with the layer whose run it takes.
    pub(crate) shared: Vec<(&'a str, &'static str)>,
    /// Enabled, in range, and no plan: a skip is never a pass (H-37).
    pub(crate) not_run: Vec<String>,
    /// Commands named, whether or not they ran.
    pub(crate) listed: usize,
    /// What `cargo mutants` left in this run's `outcomes.json`: `None` when it
    /// did not finish, `Err` when the file is not the shape [`mutation`] reads.
    pub(crate) found: Option<Result<Outcomes, String>>,
}

/// The pinned Bend is fetched first: a missing Bend fails before any work.
pub(crate) fn prepare(dir: &Path, plan: &GatePlan<'_>, mode: Mode) -> Result<Prepared, Fault> {
    if !plan.laws_on || mode == Mode::DryRun {
        return Ok(Prepared { judge: None });
    }
    match laws::ensure_bend(dir) {
        Ok(judge) => Ok(Prepared { judge: Some(judge) }),
        Err(msg) => {
            println!("\nfail: {msg}");
            Err(Fault::announced(msg))
        }
    }
}

/// `cargo` subcommands that have no `--version` of their own.
const CARGO_BUILTINS: [&str; 6] = ["test", "build", "check", "doc", "run", "bench"];

/// The first line `<tool> --version` prints, or `None` when the tool does
/// not answer. A `cargo` subcommand asks itself; a built-in asks `cargo`.
fn version_of(
    dir: &Path,
    argv: &[String],
    cache: &mut HashMap<String, Option<String>>,
) -> Option<String> {
    let (program, rest) = argv.split_first()?;
    ask_cached(
        dir,
        &version_ask(program, rest.first().map(String::as_str)),
        cache,
    )
}

/// The first line `asks` prints, or `None` when it cannot run or fails. Each
/// distinct argv is run once per gate, so a pinned tool the gate checked and
/// the version the report names come from the same answer.
fn ask_cached(
    dir: &Path,
    asks: &[&str],
    cache: &mut HashMap<String, Option<String>>,
) -> Option<String> {
    cache
        .entry(asks.join(" "))
        .or_insert_with(|| {
            let (head, tail) = asks.split_first()?;
            let out = Command::new(head)
                .args(tail)
                .current_dir(dir)
                .output()
                .ok()?;
            out.status.success().then(|| {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .map(str::to_string)
            })?
        })
        .clone()
}

/// The argv that asks `program` for its version, given its first argument.
fn version_ask<'a>(program: &'a str, first: Option<&'a str>) -> Vec<&'a str> {
    match (program, first) {
        ("cargo", Some(sub)) if !sub.starts_with('-') && !CARGO_BUILTINS.contains(&sub) => {
            vec!["cargo", sub, "--version"]
        }
        ("cargo", _) => vec!["cargo", "--version"],
        (other, _) => vec![other, "--version"],
    }
}

/// The first tool step of `chain`: the command whose version names the layer.
fn first_tool(chain: &[Step]) -> Option<&[String]> {
    chain
        .iter()
        .find(|step| step.kind == Kind::Tool)
        .map(|step| step.argv.as_slice())
}

/// Execute `plan`. Every command prints its `==>` line first, then runs.
pub(crate) fn run<'a>(dir: &Path, plan: &GatePlan<'a>, mode: Mode) -> Result<Executed<'a>, Fault> {
    let mut runner = Runner {
        dir,
        plan,
        live: mode == Mode::Run,
        done: Executed {
            started: SystemTime::now(),
            runs: Vec::new(),
            timing: Vec::new(),
            shared: Vec::new(),
            not_run: Vec::new(),
            listed: 0,
            found: None,
        },
        versions: HashMap::new(),
        asked: HashMap::new(),
    };
    for unit in &plan.units {
        runner.unit(unit)?;
    }
    if runner.live {
        runner.take_shared();
    } else {
        runner.list_laws();
    }
    Ok(runner.done)
}

/// The state of one execution.
struct Runner<'p, 'a> {
    dir: &'p Path,
    plan: &'p GatePlan<'a>,
    /// `false` for a dry run: commands are listed, never run.
    live: bool,
    done: Executed<'a>,
    versions: HashMap<String, Option<String>>,
    /// What each pinned tool answered, asked once per gate.
    asked: HashMap<&'static str, Line>,
}

impl<'a> Runner<'_, 'a> {
    fn unit(&mut self, unit: &Unit<'a>) -> Result<(), Fault> {
        let layer = unit.layer;
        match &unit.what {
            What::Shared(from) => self.shared(layer, from),
            What::NotRun => self.not_run(layer),
            What::Mutation(mutation) => {
                if !self.pinned_tools_stop(layer) {
                    self.mutation(layer, mutation)?;
                }
            }
            What::Chains(chains) => {
                if !self.pinned_tools_stop(layer) {
                    for chain in chains {
                        self.chain(layer, chain)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Asks each pinned tool `layer` needs for its version, before any of the
    /// layer's commands ([`Runner::ask_pins`]). `true` when the layer stopped
    /// on one. A dry run only names the pins.
    fn pinned_tools_stop(&mut self, layer: &Layer) -> bool {
        let pins = self.plan.pins_of(&layer.id);
        if pins.is_empty() {
            return false;
        }
        let named: Vec<String> = pins
            .iter()
            .map(|(row, pin)| format!("{} {pin}", row.key))
            .collect();
        println!(
            "\n==> [{}] {TOOL_LOCK} pins, asked first: {}",
            layer.id,
            named.join(", ")
        );
        if !self.live {
            return false;
        }
        let clock = Clock::start();
        let Some(run) = self.ask_pins(layer) else {
            return false;
        };
        self.done.record(run, &clock, None);
        true
    }

    /// The run that stands for `layer` when a pinned tool it needs is at
    /// another version ([`Outcome::ToolMismatch`], the note naming tool, pin
    /// and found) or will not run ([`Outcome::ToolMissing`]); `None` when
    /// every tool answers with its pin. Each tool is asked once per gate.
    fn ask_pins(&mut self, layer: &Layer) -> Option<Run> {
        let dir = self.dir;
        let mut mismatched = Vec::new();
        let mut missing: Option<String> = None;
        for (row, pin) in self.plan.pins_of(&layer.id) {
            let line = self.asked.entry(row.key).or_insert_with(|| {
                lock::check_row(row, pin, &mut |argv| {
                    ask_cached(dir, argv, &mut self.versions)
                })
            });
            match &line.verdict {
                Verdict::Mismatch { .. } => mismatched.push((row.argv.join(" "), line.text())),
                Verdict::Missing { asked } => {
                    missing.get_or_insert_with(|| asked.clone());
                }
                Verdict::Ok { .. } | Verdict::Skipped { .. } => {}
            }
        }
        if let Some((argv, _)) = mismatched.first() {
            let notes: Vec<&str> = mismatched.iter().map(|(_, text)| text.as_str()).collect();
            return Some(mismatch_run(&layer.id, argv, notes.join("; ")));
        }
        missing.map(|asked| missing_run(&layer.id, asked))
    }

    fn shared(&mut self, layer: &'a Layer, from: &'static str) {
        println!(
            "\n==> [{}] no command of its own: it takes the {from} run's outcome",
            layer.id
        );
        self.done.shared.push((&layer.id, from));
    }

    fn not_run(&mut self, layer: &Layer) {
        let tier = crate::tiers::tier_for(layer);
        println!(
            "\nNotRun [{}]: enabled at tier {tier} (<= --max-tier {}) and no command is mapped",
            layer.id, self.plan.max_tier
        );
        self.done.not_run.push(format!(
            "{}: NotRun (enabled at tier {tier}, no command mapped; disable it or lower --max-tier)",
            layer.id
        ));
    }

    fn mutation(&mut self, layer: &Layer, mutation: &MutationPlan) -> Result<(), Fault> {
        println!(
            "\n==> [{}] {} && {}",
            layer.id,
            mutation.diff.display(),
            mutation.mutants.display()
        );
        self.done.listed += 1;
        if self.live {
            let clock = Clock::start();
            let Mutated { run, found } =
                run_mutation(self.dir, &layer.id, mutation).map_err(Fault::env)?;
            self.done.found = found;
            // The tool is asked for its version only when it ran: a vacuous
            // or failed diff runs no `cargo mutants`.
            let version = (run.outcome != Outcome::ToolMissing
                && run.cmd.contains(&mutation.mutants.display()))
            .then(|| version_of(self.dir, &mutation.mutants.argv, &mut self.versions))
            .flatten();
            self.done.record(run, &clock, version);
        }
        Ok(())
    }

    fn chain(&mut self, layer: &Layer, chain: &[Step]) -> Result<(), Fault> {
        println!(
            "\n==> [{}] {}",
            layer.id,
            chain
                .iter()
                .map(Step::display)
                .collect::<Vec<_>>()
                .join(" && ")
        );
        self.done.listed += 1;
        if !self.live {
            return Ok(());
        }
        let clock = Clock::start();
        let run = run_chain(self.dir, &layer.id, chain).map_err(Fault::env)?;
        let version = first_tool(chain)
            // A tool that is not there has no version to ask for, and crap's
            // is named only when the lock pinned it.
            .filter(|_| run.outcome != Outcome::ToolMissing)
            .filter(|_| layer.id != "crap" || !self.plan.pins_of("crap").is_empty())
            .and_then(|argv| version_of(self.dir, argv, &mut self.versions));
        self.done.record(run, &clock, version);
        Ok(())
    }

    /// A dry run names what the laws layer would run.
    fn list_laws(&self) {
        if self.plan.laws_on {
            println!(
                "\n==> [laws] sh {} (only if Bend from {} is not installed)",
                laws::FETCH,
                laws::LOCK
            );
            println!(
                "\n==> [laws] bend {}/cases.bend",
                laws::report_display(self.dir)
            );
        }
    }

    /// The run `layer` takes from `from`. `property` and `acceptance` pass
    /// only when that run also wrote a JUnit report holding a passed test
    /// they select by name; the report is read, never produced (HN-P3).
    fn credited(&self, layer: &str, from: &str) -> Run {
        let mut run = self.plan.tests.take(layer, from, &self.done.runs);
        let Some(filter) = self.plan.filters.for_layer(layer) else {
            return run;
        };
        if run.outcome != Outcome::Passed {
            return run;
        }
        let root = laws::workspace_root(self.dir).unwrap_or_else(|| self.dir.to_path_buf());
        // A report older than the credited run's own start is not its report.
        let since = self
            .done
            .timing
            .iter()
            .find(|timing| timing.layer == from)
            .map_or(self.done.started, |timing| timing.began);
        if let Err(why) = evidence::credit(layer, &root, filter, since) {
            run.outcome = Outcome::Failed;
            run.note = Some(why);
        }
        run
    }

    /// A layer that adds no command records its source's run.
    fn take_shared(&mut self) {
        for (layer, from) in self.done.shared.clone() {
            let run = self.credited(layer, from);
            match &run.note {
                Some(note) => {
                    println!("[{layer}] took the {from} run, but it is no credit: {note}");
                }
                None => println!("[{layer}] took the {from} run's outcome: {}", run.outcome),
            }
            self.done.runs.push(run);
            self.done.timing.push(Timing {
                layer: layer.to_string(),
                began: SystemTime::now(),
                seconds: 0.0,
                tool_version: None,
            });
        }
    }
}

impl Executed<'_> {
    /// Keep `run` with the time since `clock` and the tool's version.
    fn record(&mut self, run: Run, clock: &Clock, tool_version: Option<String>) {
        self.timing.push(Timing {
            layer: run.layer.clone(),
            began: clock.began,
            seconds: clock.watch.elapsed().as_secs_f64(),
            tool_version,
        });
        self.runs.push(run);
    }

    /// Seconds spent on `layer`, over all its commands.
    pub(crate) fn seconds(&self, layer: &str) -> f64 {
        self.timing
            .iter()
            .filter(|timing| timing.layer == layer)
            .map(|timing| timing.seconds)
            .sum::<f64>()
            // An empty float sum is -0.0; a layer that never ran took 0.0.
            + 0.0
    }

    /// The tool versions seen for `layer`, once each, joined.
    pub(crate) fn tool_version(&self, layer: &str) -> Option<String> {
        let mut seen: Vec<&str> = Vec::new();
        for version in self
            .timing
            .iter()
            .filter(|timing| timing.layer == layer)
            .filter_map(|timing| timing.tool_version.as_deref())
        {
            if !seen.contains(&version) {
                seen.push(version);
            }
        }
        (!seen.is_empty()).then(|| seen.join("; "))
    }
}

/// The steps that ran, joined, and the outcome of the last one.
pub(crate) fn finish(layer: &str, ran: &[String], outcome: Outcome) -> Run {
    Run {
        layer: layer.to_string(),
        cmd: ran.join(" && "),
        step: ran.last().cloned().unwrap_or_default(),
        outcome,
        note: None,
    }
}

pub(crate) fn run_chain(dir: &Path, layer: &str, chain: &[Step]) -> Result<Run> {
    let mut ran = Vec::new();
    let mut outcome = Outcome::Passed;
    for step in chain {
        ran.push(step.display());
        outcome = run_step(dir, step)?;
        if !steps::chain_continues(outcome) {
            break;
        }
    }
    Ok(finish(layer, &ran, outcome))
}

/// A layer that stopped because a pinned tool prints another version than its
/// pin. The same `note` is the stdout line, the layer's detail and its problem.
fn mismatch_run(layer: &str, argv: &str, note: String) -> Run {
    println!("tool-mismatch [{layer}]: {note}");
    Run {
        layer: layer.to_string(),
        cmd: String::new(),
        step: argv.to_string(),
        outcome: Outcome::ToolMismatch,
        note: Some(note),
    }
}

/// A layer that stopped because a pinned tool will not run: the same outcome
/// and words as a command whose tool is not on `PATH`.
fn missing_run(layer: &str, asked: String) -> Run {
    let words: Vec<String> = asked.split_whitespace().map(str::to_string).collect();
    let binary = steps::tool_binary(&words).unwrap_or_else(|| asked.clone());
    println!("{}", steps::MissingTool { binary });
    Run {
        layer: layer.to_string(),
        cmd: String::new(),
        step: asked,
        outcome: Outcome::ToolMissing,
        note: None,
    }
}

/// A mutation layer's run, and what its `outcomes.json` held.
pub(crate) struct Mutated {
    pub(crate) run: Run,
    pub(crate) found: Option<Result<Outcomes, String>>,
}

impl Mutated {
    fn without_outcomes(run: Run) -> Self {
        Self { run, found: None }
    }
}

/// `cargo-mutants` is looked up before the diff, so an empty diff cannot
/// hide a missing tool. A base that was named and is not a commit fails the
/// layer before any command runs. A failed diff is `Failed`; nothing waives
/// it. An empty one is `Vacuous` and `cargo mutants` does not run, except
/// that no base at all fails under CI. [`steps::stop_note`] words every one
/// of these stops. When `cargo mutants` runs, an `outcomes.json` from an
/// earlier run is removed first, and exit 0, 2 (missed) and 3 (timeouts)
/// are a finished run whose ledger the judge reads.
pub(crate) fn run_mutation(dir: &Path, layer: &str, plan: &MutationPlan) -> Result<Mutated> {
    if let Err(missing) = steps::check_tool(&plan.mutants.argv) {
        println!("{missing}");
        return Ok(Mutated::without_outcomes(finish(
            layer,
            &[plan.mutants.display()],
            Outcome::ToolMissing,
        )));
    }
    if matches!(plan.base, Base::Unresolved { .. }) {
        return Ok(Mutated::without_outcomes(stopped(
            layer,
            &[],
            &plan.base,
            Outcome::Failed,
        )));
    }
    let mut ran = vec![plan.diff.display()];
    let diff_ok = run_step(dir, &plan.diff)? == Outcome::Passed;
    let diff = steps::judge_diff(diff_ok, &plan.diff_path);
    let Some(mutants) = plan.after_diff(diff) else {
        return Ok(Mutated::without_outcomes(stopped(
            layer, &ran, &plan.base, diff,
        )));
    };
    ran.push(mutants.display());
    mutation::clear(&plan.output)?;
    let ended = run_mutants(dir, mutants)?;
    Ok(settled(
        finish(layer, &ran, ended.outcome),
        &ended,
        &plan.output,
    ))
}

/// The run of `cargo mutants` once it ended. A command that failed says what
/// its exit means; one that finished (exit 0, 2 or 3) is judged from the
/// outcomes it wrote.
fn settled(mut run: Run, ended: &Ended, output: &Path) -> Mutated {
    if ended.outcome != Outcome::Passed {
        // A missing tool has its own line; a failed run says what its exit means.
        run.note = (ended.outcome == Outcome::Failed)
            .then(|| format!("`{}` {}", run.step, exit_meaning(ended.code)));
        return Mutated::without_outcomes(run);
    }
    let found = mutation::read(output, ended.code.unwrap_or_default(), &ended.stderr);
    if let Err(why) = &found {
        run.outcome = Outcome::Failed;
        run.note = Some(why.clone());
    }
    Mutated {
        run,
        found: Some(found),
    }
}

/// What a failed `cargo mutants` exit means, in cargo-mutants' own terms.
fn exit_meaning(code: Option<i32>) -> String {
    match code {
        Some(1) => "exited 1: it was given arguments it does not accept".to_string(),
        Some(4) => {
            "exited 4: the tests fail in the unmutated tree, so no mutant was tested".to_string()
        }
        Some(code) => format!("exited {code}"),
        None => "was ended by a signal".to_string(),
    }
}

/// The run of a layer that stopped on its base or its diff: prints the
/// note and keeps it with the run, so the report says the same. A diff
/// command that ran and failed is named in the kept note, since it did fail;
/// a base that was never tried, or an empty diff, names no command.
fn stopped(layer: &str, ran: &[String], base: &Base, diff: Outcome) -> Run {
    let (outcome, note) = steps::stop_note(base, diff)
        .unwrap_or_else(|| (diff, format!("the mutation diff ended as {diff}")));
    println!("{outcome}: {note}");
    let command_failed = diff == Outcome::Failed && !matches!(base, Base::Unresolved { .. });
    let kept = match ran.last() {
        Some(step) if command_failed => format!("`{step}` failed: {note}"),
        _ => note,
    };
    Run {
        note: Some(kept),
        ..finish(layer, ran, outcome)
    }
}

pub(crate) fn run_step(dir: &Path, step: &Step) -> Result<Outcome> {
    match step.kind {
        Kind::Artifact => {
            return Ok(step.artifact_path().map_or(Outcome::Failed, |path| {
                steps::artifact_outcome(&dir.join(path))
            }));
        }
        Kind::Tool => {
            if let Err(missing) = steps::check_tool(&step.argv) {
                println!("{missing}");
                return Ok(Outcome::ToolMissing);
            }
        }
        Kind::Command => {}
    }
    Ok(match spawn(dir, step)? {
        Spawned::Exited(status) if status.success() => Outcome::Passed,
        Spawned::Exited(_) | Spawned::NoProgram => Outcome::Failed,
        Spawned::Missing => Outcome::ToolMissing,
    })
}

/// How `cargo mutants` ended: its outcome, exit code and what it printed.
struct Ended {
    outcome: Outcome,
    code: Option<i32>,
    stderr: String,
}

impl Ended {
    fn without_output(outcome: Outcome) -> Self {
        Self {
            outcome,
            code: None,
            stderr: String::new(),
        }
    }
}

/// `cargo mutants` exits 2 when mutants were missed and 3 when some timed
/// out: those and 0 are a finished run, judged by the ledger. Any other exit
/// is a failed command. What it prints to stderr is shown and kept, because
/// a run that found nothing to mutate says so there and writes no file.
fn run_mutants(dir: &Path, step: &Step) -> Result<Ended> {
    let Some((program, args)) = step.program() else {
        return Ok(Ended::without_output(Outcome::Failed));
    };
    let mut child = match Command::new(program)
        .args(args)
        .current_dir(dir)
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "{}",
                steps::MissingTool {
                    binary: program.to_string()
                }
            );
            return Ok(Ended::without_output(Outcome::ToolMissing));
        }
        Err(err) => return Err(err).with_context(|| format!("spawn: {}", step.display())),
    };
    let stderr = tee(child.stderr.take());
    let status = child
        .wait()
        .with_context(|| format!("wait: {}", step.display()))?;
    let code = status.code();
    let outcome = if matches!(code, Some(0 | 2 | 3)) {
        Outcome::Passed
    } else {
        Outcome::Failed
    };
    Ok(Ended {
        outcome,
        code,
        stderr,
    })
}

/// Everything `pipe` yields, passed on to our own stderr as it comes.
fn tee(pipe: Option<ChildStderr>) -> String {
    let Some(pipe) = pipe else {
        return String::new();
    };
    let mut kept = String::new();
    for line in BufReader::new(pipe).split(b'\n').map_while(Result::ok) {
        let line = String::from_utf8_lossy(&line).into_owned();
        eprintln!("{line}");
        kept.push_str(&line);
        kept.push('\n');
    }
    kept
}

/// How a spawned command ended.
enum Spawned {
    Exited(ExitStatus),
    /// The program is not there; the missing-tool line is already printed.
    Missing,
    /// The step names no program.
    NoProgram,
}

fn spawn(dir: &Path, step: &Step) -> Result<Spawned> {
    let Some((program, args)) = step.program() else {
        return Ok(Spawned::NoProgram);
    };
    match Command::new(program).args(args).current_dir(dir).status() {
        Ok(status) => Ok(Spawned::Exited(status)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "{}",
                steps::MissingTool {
                    binary: program.to_string()
                }
            );
            Ok(Spawned::Missing)
        }
        Err(err) => Err(err).with_context(|| format!("spawn: {}", step.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::{exit_meaning, run_mutants, run_step, version_ask, Executed, Timing};
    use crate::facts::Outcome;
    use crate::steps::Step;
    use std::fs;
    use std::path::Path;
    use std::time::SystemTime;

    #[test]
    fn a_plugin_asks_itself_and_a_built_in_asks_cargo() {
        assert_eq!(
            version_ask("cargo", Some("deny")),
            ["cargo", "deny", "--version"]
        );
        for built_in in ["test", "build", "check", "doc", "run", "bench"] {
            assert_eq!(
                version_ask("cargo", Some(built_in)),
                ["cargo", "--version"],
                "{built_in}"
            );
        }
        assert_eq!(
            version_ask("cargo", Some("--version")),
            ["cargo", "--version"]
        );
        assert_eq!(version_ask("cargo", None), ["cargo", "--version"]);
        assert_eq!(version_ask("bend", Some("run")), ["bend", "--version"]);
    }

    fn timing(layer: &str, seconds: f64) -> Timing {
        Timing {
            layer: layer.to_string(),
            began: SystemTime::UNIX_EPOCH,
            seconds,
            tool_version: None,
        }
    }

    #[test]
    fn a_layer_is_timed_over_its_own_commands_only() {
        let done = Executed {
            started: SystemTime::now(),
            runs: Vec::new(),
            timing: vec![timing("a", 1.5), timing("b", 4.0), timing("a", 2.0)],
            shared: Vec::new(),
            not_run: Vec::new(),
            listed: 0,
            found: None,
        };
        assert!((done.seconds("a") - 3.5).abs() < 1e-9);
        assert!((done.seconds("b") - 4.0).abs() < 1e-9);
        // A layer that never ran took 0.0, not -0.0.
        assert!(done.seconds("c").abs() < 1e-9 && done.seconds("c").is_sign_positive());
    }

    #[test]
    fn a_missing_program_is_a_missing_tool_and_any_other_spawn_error_is_not() {
        let dir = std::env::temp_dir().join(format!("cargo-tops-exec-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("mkdir");
        let missing = Step::command([dir.join("no-such-program").display().to_string()]);
        assert_eq!(
            run_step(Path::new("."), &missing).expect("a missing program is an outcome"),
            Outcome::ToolMissing
        );
        // A file with no execute bit exists but cannot be spawned.
        let plain = dir.join("plain");
        fs::write(&plain, "x").expect("write");
        let blocked = Step::command([plain.display().to_string()]);
        let err = run_step(Path::new("."), &blocked).expect_err("not spawnable");
        assert!(format!("{err:#}").contains("spawn"), "{err:#}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cargo_mutants_exits_0_2_and_3_are_a_finished_run() {
        let wants = [
            (0, Outcome::Passed),
            (1, Outcome::Failed),
            (2, Outcome::Passed),
            (3, Outcome::Passed),
            (4, Outcome::Failed),
            (101, Outcome::Failed),
        ];
        for (code, want) in wants {
            let step = Step::command(["sh".to_string(), "-c".to_string(), format!("exit {code}")]);
            let got = run_mutants(Path::new("."), &step).expect("it ran");
            assert_eq!(got.outcome, want, "exit {code}");
            assert_eq!(got.code, Some(code), "exit {code}");
            // Any other command still fails on every nonzero exit.
            let plain = run_step(Path::new("."), &step).expect("it ran");
            let plain_want = if code == 0 {
                Outcome::Passed
            } else {
                Outcome::Failed
            };
            assert_eq!(plain, plain_want, "exit {code}");
        }
        let nowhere = Step::command(["/no/such/program/here"]);
        assert_eq!(
            run_mutants(Path::new("."), &nowhere)
                .expect("an outcome")
                .outcome,
            Outcome::ToolMissing
        );
    }

    #[test]
    fn a_mutants_spawn_error_is_an_error_and_a_signal_is_named() {
        let dir = std::env::temp_dir().join(format!("cargo-tops-mutexec-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("mkdir");
        let plain = dir.join("plain");
        fs::write(&plain, "x").expect("write");
        let blocked = Step::command([plain.display().to_string()]);
        let err = run_mutants(Path::new("."), &blocked)
            .err()
            .expect("not spawnable");
        assert!(format!("{err:#}").contains("spawn"), "{err:#}");
        fs::remove_dir_all(&dir).ok();
        let killed = Step::command(["sh".to_string(), "-c".to_string(), "kill -9 $$".to_string()]);
        let ended = run_mutants(Path::new("."), &killed).expect("it ran");
        assert_eq!((ended.outcome, ended.code), (Outcome::Failed, None));
        assert!(exit_meaning(None).contains("signal"));
        assert!(exit_meaning(Some(4)).contains("unmutated tree"));
        assert!(exit_meaning(Some(1)).contains("exited 1"));
        assert_eq!(exit_meaning(Some(77)), "exited 77");
    }
}
