//! The plan of one gate run: which layers run, with which commands, and
//! which take another layer's run. Nothing here runs a layer.
use std::path::{Path, PathBuf};

use anyhow::anyhow;

use super::Fault;
use crate::config::{self, TopsConfig};
use crate::facts::{Outcome, Run};
use crate::{credit, laws, steps, TOOL_LOCK};
use anyhow::{Context, Result};
use std::fs;
use steps::{Kind, MutationPlan, Step};

/// The command whose first line must name the pinned cargo-crap.
pub(crate) const CRAP_VERSION: [&str; 3] = ["cargo", "crap", "--version"];

/// What a layer runs.
pub(crate) enum Plan {
    /// Each chain runs on its own. Inside a chain only a pass continues.
    Chains(Vec<Vec<Step>>),
    /// The diff, judged, then `cargo mutants` on a non-empty diff.
    Mutation(MutationPlan),
}

/// The test commands of the enabled layers in range, which decide whether a
/// layer runs its own or takes another layer's run ([`credit`]). A layer
/// that is off or above the range has no command here, so nothing takes its
/// run.
pub(crate) struct TestCommands {
    pub(crate) coverage_in_range: bool,
    pub(crate) coverage: Vec<String>,
    pub(crate) unit: Vec<String>,
    pub(crate) integration: Vec<String>,
}

impl TestCommands {
    pub(crate) fn new(enabled: &[&config::Layer], report: &Path) -> Self {
        let argv = |id: &str| {
            enabled
                .iter()
                .find(|layer| layer.id == id)
                .and_then(|layer| plan_for(layer, report))
                .and_then(|plan| match plan {
                    Plan::Chains(chains) => chains
                        .into_iter()
                        .flatten()
                        .find(|step| step.kind == Kind::Tool),
                    Plan::Mutation(_) => None,
                })
                .map(|step| step.argv)
                .unwrap_or_default()
        };
        Self {
            coverage_in_range: enabled.iter().any(|layer| layer.id == "coverage"),
            coverage: argv("coverage"),
            unit: argv("unit"),
            integration: argv("integration"),
        }
    }

    pub(crate) fn query<'a>(&'a self, layer: &'a str, runs: &[Run]) -> credit::Query<'a> {
        let outcome = |id: &str| {
            runs.iter()
                .find(|run| run.layer == id)
                .map(|run| match run.outcome {
                    Outcome::Passed => credit::Outcome::Passed,
                    Outcome::ToolMissing => credit::Outcome::ToolMissing,
                    // A test run is never Vacuous; if one were, it is no pass.
                    Outcome::Failed | Outcome::Vacuous => credit::Outcome::Failed,
                })
        };
        credit::Query {
            layer,
            coverage_in_range: self.coverage_in_range,
            coverage_argv: &self.coverage,
            coverage_outcome: outcome("coverage"),
            unit_argv: &self.unit,
            integration_argv: &self.integration,
            unit_outcome: outcome("unit"),
        }
    }

    /// The layer whose run `layer` takes, or `None` when it runs its own.
    /// Credit from coverage is only possible in range, and from `unit` only
    /// out of it, so the range names the source.
    pub(crate) fn shared_from(&self, layer: &str) -> Option<&'static str> {
        if credit::runs_itself(&self.query(layer, &[])) {
            None
        } else if self.coverage_in_range {
            Some("coverage")
        } else {
            Some("unit")
        }
    }

    /// The run `layer` records once `from` has run: `from`'s command, and the
    /// outcome [`credit::recorded_outcome`] gives. A source that did not run
    /// is not a pass.
    pub(crate) fn take(&self, layer: &str, from: &str, runs: &[Run]) -> Run {
        let outcome = match credit::recorded_outcome(&self.query(layer, runs)) {
            Some(credit::Outcome::Passed) => Outcome::Passed,
            Some(credit::Outcome::ToolMissing) => Outcome::ToolMissing,
            Some(credit::Outcome::Failed) | None => Outcome::Failed,
        };
        let source = runs.iter().find(|run| run.layer == from);
        Run {
            layer: layer.to_string(),
            cmd: source.map(|run| run.cmd.clone()).unwrap_or_default(),
            step: source.map(|run| run.step.clone()).unwrap_or_default(),
            outcome,
        }
    }
}

/// The `[tools]` version that `dir/rust-tops.lock` pins for `tool`. No lock,
/// or no line for `tool`, is `None`. A lock that does not parse is an error.
pub(crate) fn tool_pin(dir: &Path, tool: &str) -> Result<Option<String>> {
    let path = dir.join(TOOL_LOCK);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("read {}", path.display())),
    };
    let lock: toml::Table =
        toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    Ok(lock
        .get("tools")
        .and_then(|tools| tools.get(tool))
        .and_then(toml::Value::as_str)
        .map(str::to_string))
}

pub(crate) fn tool(argv: &[&str]) -> Vec<Step> {
    vec![Step::tool(argv.iter().copied())]
}

/// `report` is [`laws::artifact_dir`]: the coverage LCOV, the CRAP report
/// and the mutation diff are written there.
#[expect(clippy::too_many_lines, reason = "over the kit line threshold")]
pub(crate) fn plan_for(layer: &config::Layer, report: &Path) -> Option<Plan> {
    let chains = match layer.id.as_str() {
        "static-lint" => vec![
            tool(&["cargo", "fmt", "--all", "--", "--check"]),
            tool(&[
                "cargo",
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ]),
            // `security-advisory` runs `advisories`; each kind runs once.
            tool(&["cargo", "deny", "check", "bans", "licenses", "sources"]),
        ],
        "unit" | "integration" => vec![tool(&[
            "cargo",
            "nextest",
            "run",
            "--workspace",
            "--all-features",
            "--profile",
            "ci",
        ])],
        "doc" => vec![tool(&[
            "cargo",
            "test",
            "--doc",
            "--workspace",
            "--all-features",
        ])],
        "coverage" => vec![steps::coverage_steps(report)],
        "crap" => {
            let lcov = report.join(steps::LCOV).display().to_string();
            let crap = report.join(steps::CRAP).display().to_string();
            vec![vec![
                Step::tool([
                    "cargo",
                    "crap",
                    "--lcov",
                    lcov.as_str(),
                    "--format",
                    "json",
                    "--output",
                    crap.as_str(),
                ]),
                Step::artifact(crap),
            ]]
        }
        "mutation" => return Some(Plan::Mutation(MutationPlan::new(report))),
        "property" => vec![tool(&[
            "cargo",
            "nextest",
            "run",
            "--workspace",
            "--all-features",
            "--profile",
            "ci",
            "--no-tests=fail",
            "property",
        ])],
        "security-advisory" => vec![tool(&["cargo", "deny", "check", "advisories"])],
        "formal-kani" => vec![tool(&["cargo", "kani", "--workspace"])],
        _ => return None,
    };
    Some(Plan::Chains(chains))
}

/// What one enabled layer does in this gate.
pub(crate) enum What {
    /// Each chain runs on its own.
    Chains(Vec<Vec<Step>>),
    /// The diff, judged, then `cargo mutants` on a non-empty diff.
    Mutation(MutationPlan),
    /// No command of its own: it takes the run of the named layer.
    Shared(&'static str),
    /// Enabled, in range, and no command is mapped: a skip is never a pass.
    NotRun,
}

/// One enabled layer in range, other than `laws`, and what it does.
pub(crate) struct Unit<'a> {
    pub(crate) layer: &'a config::Layer,
    pub(crate) what: What,
}

/// Everything a gate decides before it runs a command.
pub(crate) struct GatePlan<'a> {
    pub(crate) max_tier: u8,
    /// Enabled layers at or below the max tier, in `rust-tops.yaml` order.
    pub(crate) enabled: Vec<&'a config::Layer>,
    /// Enabled layers above the max tier: named, never judged.
    pub(crate) above: Vec<&'a str>,
    /// Layers turned off: named, never run.
    pub(crate) disabled: Vec<&'a str>,
    pub(crate) laws_on: bool,
    /// The coverage LCOV, CRAP report, mutation diff, laws report and
    /// `report.json` live here, as an absolute path.
    pub(crate) report: PathBuf,
    pub(crate) tests: TestCommands,
    pub(crate) crap_pin: Option<String>,
    pub(crate) units: Vec<Unit<'a>>,
}

impl GatePlan<'_> {
    /// The commands of `layer` as argv lists, in the order they run.
    pub(crate) fn argv_of(&self, layer: &str) -> Vec<Vec<String>> {
        let Some(unit) = self.units.iter().find(|unit| unit.layer.id == layer) else {
            return Vec::new();
        };
        match &unit.what {
            What::Chains(chains) => {
                let pin = (layer == "crap" && self.crap_pin.is_some())
                    .then(|| CRAP_VERSION.map(String::from).to_vec());
                pin.into_iter()
                    .chain(chains.iter().flatten().map(|step| step.argv.clone()))
                    .collect()
            }
            What::Mutation(plan) => vec![plan.diff.argv.clone(), plan.mutants.argv.clone()],
            What::Shared(from) => self.argv_of(from),
            What::NotRun => Vec::new(),
        }
    }
}

/// Enabled layers at or below `max_tier`, in `rust-tops.yaml` order.
fn in_range(cfg: &TopsConfig, max_tier: u8) -> Vec<&config::Layer> {
    cfg.layers
        .iter()
        .filter(|l| l.enabled && l.tier.unwrap_or(0) <= max_tier)
        .collect()
}

/// The ids of the layers `keep` selects.
fn ids(cfg: &TopsConfig, keep: impl Fn(&config::Layer) -> bool) -> Vec<&str> {
    cfg.layers
        .iter()
        .filter(|layer| keep(layer))
        .map(|layer| layer.id.as_str())
        .collect()
}

/// The report directory as an absolute path. The laws report, the mutation
/// diff, the LCOV and the CRAP report share it, a nonempty `CARGO_TARGET_DIR`
/// moves it, and steps run with `dir` as the working directory, so it must
/// not be relative to the caller's.
fn report_dir(dir: &Path) -> Result<PathBuf, Fault> {
    let report = std::path::absolute(laws::artifact_dir(dir)).map_err(|err| {
        Fault::env(
            anyhow::Error::new(err).context(format!("resolve the report dir of {}", dir.display())),
        )
    })?;
    if laws::metadata_unavailable(dir) {
        return Err(Fault::env(anyhow!(
            "cargo metadata is unavailable for {}: no target directory to report into",
            dir.display()
        )));
    }
    Ok(report)
}

/// What each enabled layer other than `laws` does.
fn units_of<'a>(
    enabled: &[&'a config::Layer],
    tests: &TestCommands,
    report: &Path,
) -> Vec<Unit<'a>> {
    enabled
        .iter()
        .copied()
        .filter(|layer| layer.id != "laws")
        .map(|layer| {
            let what = match (tests.shared_from(&layer.id), plan_for(layer, report)) {
                (Some(from), _) => What::Shared(from),
                (None, Some(Plan::Chains(chains))) => What::Chains(chains),
                (None, Some(Plan::Mutation(plan))) => What::Mutation(plan),
                (None, None) => What::NotRun,
            };
            Unit { layer, what }
        })
        .collect()
}

/// The plan for `cfg` at `max_tier`. A config the gate cannot plan from is
/// a [`Fault::Config`]; a project whose Cargo metadata cannot be read, or
/// whose report directory cannot be resolved, is a [`Fault::Env`].
pub(crate) fn build<'a>(
    dir: &Path,
    cfg: &'a TopsConfig,
    max_tier: u8,
) -> Result<GatePlan<'a>, Fault> {
    let enabled = in_range(cfg, max_tier);
    if enabled.is_empty() {
        return Err(Fault::config(anyhow!(
            "no enabled layers at or below tier {max_tier}"
        )));
    }
    let report = report_dir(dir)?;
    let tests = TestCommands::new(&enabled, &report);
    let crap_pin = if enabled.iter().any(|layer| layer.id == "crap") {
        tool_pin(dir, "cargo-crap").map_err(Fault::config)?
    } else {
        None
    };
    Ok(GatePlan {
        max_tier,
        laws_on: enabled.iter().any(|l| l.id == "laws"),
        // Enabled layers above the range are not judged, so they cannot
        // fail the gate. The run names them.
        above: ids(cfg, |l| l.enabled && l.tier.unwrap_or(0) > max_tier),
        // A layer turned off still shows in the run: it is not run, not omitted.
        disabled: ids(cfg, |l| !l.enabled),
        units: units_of(&enabled, &tests, &report),
        enabled,
        report,
        tests,
        crap_pin,
    })
}
