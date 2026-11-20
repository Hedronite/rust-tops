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
use steps::{Base, Kind, MutationPlan, Step};

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
                .and_then(|layer| tool_plan(layer, report))
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

/// The layers whose commands do not depend on where the report goes: each
/// is a list of chains of one tool step. `security-advisory` runs
/// `advisories`; `static-lint` runs the other kinds, so each kind runs once.
const TOOL_LAYERS: &[(&str, &[&[&str]])] = &[
    (
        "static-lint",
        &[
            &["cargo", "fmt", "--all", "--", "--check"],
            &[
                "cargo",
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ],
            &["cargo", "deny", "check", "bans", "licenses", "sources"],
        ],
    ),
    ("unit", &[&NEXTEST]),
    ("integration", &[&NEXTEST]),
    (
        "doc",
        &[&["cargo", "test", "--doc", "--workspace", "--all-features"]],
    ),
    (
        "property",
        &[&[
            "cargo",
            "nextest",
            "run",
            "--workspace",
            "--all-features",
            "--profile",
            "ci",
            "--no-tests=fail",
            "property",
        ]],
    ),
    (
        "security-advisory",
        &[&["cargo", "deny", "check", "advisories"]],
    ),
    ("formal-kani", &[&["cargo", "kani", "--workspace"]]),
];

/// The test run `unit` and `integration` share.
const NEXTEST: [&str; 7] = [
    "cargo",
    "nextest",
    "run",
    "--workspace",
    "--all-features",
    "--profile",
    "ci",
];

/// `cargo crap` over the LCOV, then a check that its report is not empty.
fn crap_steps(report: &Path) -> Vec<Step> {
    let lcov = report.join(steps::LCOV).display().to_string();
    let crap = report.join(steps::CRAP).display().to_string();
    vec![
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
    ]
}

/// `report` is [`laws::artifact_dir`]: the coverage LCOV, the CRAP report
/// and the mutation diff are written there. `base` is where the mutation
/// diff starts.
pub(crate) fn plan_for(layer: &config::Layer, report: &Path, base: &Base) -> Option<Plan> {
    if layer.id == "mutation" {
        Some(Plan::Mutation(MutationPlan::new(report, base.clone())))
    } else {
        tool_plan(layer, report)
    }
}

/// The plan of every layer but `mutation`, which also needs a base.
fn tool_plan(layer: &config::Layer, report: &Path) -> Option<Plan> {
    let id = layer.id.as_str();
    match id {
        "coverage" => Some(Plan::Chains(vec![steps::coverage_steps(report)])),
        "crap" => Some(Plan::Chains(vec![crap_steps(report)])),
        _ => TOOL_LAYERS
            .iter()
            .find(|(layer_id, _)| *layer_id == id)
            .map(|(_, chains)| Plan::Chains(chains.iter().map(|argv| tool(argv)).collect())),
    }
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
        .filter(|l| l.enabled && crate::tiers::tier_for(l) <= max_tier)
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

/// The mutation base, looked up only when a mutation layer is in range.
fn base_for(enabled: &[&config::Layer], dir: &Path, flag: Option<&str>, key: Option<&str>) -> Base {
    if enabled.iter().any(|layer| layer.id == "mutation") {
        steps::resolve_base(dir, flag, key)
    } else {
        Base::unused()
    }
}

/// What each enabled layer other than `laws` does.
fn units_of<'a>(
    enabled: &[&'a config::Layer],
    tests: &TestCommands,
    report: &Path,
    base: &Base,
) -> Vec<Unit<'a>> {
    enabled
        .iter()
        .copied()
        .filter(|layer| layer.id != "laws")
        .map(|layer| {
            let what = match (tests.shared_from(&layer.id), plan_for(layer, report, base)) {
                (Some(from), _) => What::Shared(from),
                (None, Some(Plan::Chains(chains))) => What::Chains(chains),
                (None, Some(Plan::Mutation(plan))) => What::Mutation(plan),
                (None, None) => What::NotRun,
            };
            Unit { layer, what }
        })
        .collect()
}

/// The plan for `cfg` at `max_tier`. `base` is the `--base` flag; with a
/// mutation layer in range it and `gates.base_ref` decide where the diff
/// starts. A config the gate cannot plan from is
/// a [`Fault::config`]; a project whose Cargo metadata cannot be read, or
/// whose report directory cannot be resolved, is a [`Fault::env`].
pub(crate) fn build<'a>(
    dir: &Path,
    cfg: &'a TopsConfig,
    max_tier: u8,
    base_flag: Option<&str>,
) -> Result<GatePlan<'a>, Fault> {
    let enabled = in_range(cfg, max_tier);
    if enabled.is_empty() {
        return Err(Fault::config(anyhow!(
            "no enabled layers at or below tier {max_tier}"
        )));
    }
    let report = report_dir(dir)?;
    let tests = TestCommands::new(&enabled, &report);
    let base = base_for(&enabled, dir, base_flag, cfg.gates.base_ref.as_deref());
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
        above: ids(cfg, |l| l.enabled && crate::tiers::tier_for(l) > max_tier),
        // A layer turned off still shows in the run: it is not run, not omitted.
        disabled: ids(cfg, |l| !l.enabled),
        units: units_of(&enabled, &tests, &report, &base),
        enabled,
        report,
        tests,
        crap_pin,
    })
}

#[cfg(test)]
mod tests {
    use super::{base_for, GatePlan, TestCommands};
    use crate::config::Layer;
    use crate::steps::{Base, BaseSource};
    use std::path::PathBuf;
    use std::process::Command;

    fn layer(id: &str) -> Layer {
        serde_saphyr::from_str(&format!(
            "{{ id: {id}, enabled: true, tier: 0, runner: x }}"
        ))
        .expect("layer")
    }

    #[test]
    fn the_base_is_looked_up_only_with_a_mutation_layer_in_range() {
        let dir = std::env::temp_dir().join(format!("cargo-tops-base-for-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("mkdir");
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("spawn git");
            assert!(out.status.success(), "git {args:?}");
        };
        git(&["init", "-q"]);
        git(&["commit", "-q", "--allow-empty", "-m", "one"]);
        let mutation = layer("mutation");
        let unit = layer("unit");
        let found = Base::Found {
            source: BaseSource::Flag,
            name: "HEAD".to_string(),
        };
        assert_eq!(base_for(&[&mutation], &dir, Some("HEAD"), None), found);
        assert_eq!(
            base_for(&[&unit, &mutation], &dir, Some("HEAD"), None),
            found
        );
        assert_eq!(base_for(&[&unit], &dir, Some("HEAD"), None), Base::unused());
        assert_eq!(base_for(&[], &dir, Some("HEAD"), None), Base::unused());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_layer_with_no_unit_in_the_plan_has_no_commands() {
        let plan = GatePlan {
            max_tier: 0,
            enabled: Vec::new(),
            above: Vec::new(),
            disabled: Vec::new(),
            laws_on: false,
            report: PathBuf::new(),
            tests: TestCommands {
                coverage_in_range: false,
                coverage: Vec::new(),
                unit: Vec::new(),
                integration: Vec::new(),
            },
            crap_pin: None,
            units: Vec::new(),
        };
        assert!(plan.argv_of("static-lint").is_empty());
    }
}
