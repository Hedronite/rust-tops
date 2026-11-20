//! The plan of one gate run: which layers run, with which commands, and
//! which take another layer's run. Nothing here runs a layer.
use std::path::{Path, PathBuf};

use anyhow::anyhow;

use super::Fault;
use crate::config::{self, TopsConfig};
use crate::facts::{Outcome, Run};
use crate::lock::{self, Row};
use crate::{credit, laws, steps};
use anyhow::Result;
use steps::{Base, Kind, MutationPlan, Step};

/// The pinned tools each layer needs, with the pin of each.
pub(crate) type LayerPins = Vec<(String, Vec<(&'static Row, String)>)>;

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
    pub(crate) fn new(enabled: &[&config::Layer], report: &Path, filters: &Filters) -> Self {
        let argv = |id: &str| {
            enabled
                .iter()
                .find(|layer| layer.id == id)
                .and_then(|layer| tool_plan(layer, report, filters))
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
                    Outcome::ToolMissing | Outcome::ToolMismatch => credit::Outcome::ToolMissing,
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
        // A source that stopped on a pinned tool's version says so in the
        // words it used, not as a missing tool.
        let mismatch = source.filter(|run| run.outcome == Outcome::ToolMismatch);
        Run {
            layer: layer.to_string(),
            cmd: source.map(|run| run.cmd.clone()).unwrap_or_default(),
            step: source.map(|run| run.step.clone()).unwrap_or_default(),
            outcome: mismatch.map_or(outcome, |run| run.outcome),
            note: mismatch.and_then(|run| run.note.clone()),
        }
    }
}

/// The `[tools]` pins of `dir/rust-tops.lock`. No lock file is no pins. A lock
/// that cannot be read, parsed or read as pins, or has no `[tools]` table, is
/// an error.
fn lock_pins(dir: &Path) -> Result<std::collections::BTreeMap<String, String>> {
    Ok(lock::lock_file_pins(dir)?.unwrap_or_default())
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
        "security-advisory",
        &[&["cargo", "deny", "check", "advisories"]],
    ),
    ("formal-kani", &[&["cargo", "kani", "--workspace"]]),
];

/// The name prefixes `property` and `acceptance` select tests by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Filters {
    pub(crate) property: String,
    pub(crate) acceptance: String,
}

impl Default for Filters {
    fn default() -> Self {
        Self {
            property: "property".to_string(),
            acceptance: "acceptance".to_string(),
        }
    }
}

impl Filters {
    pub(crate) fn of(gates: &config::Gates) -> Self {
        Self {
            property: gates.test_filter("property"),
            acceptance: gates.test_filter("acceptance"),
        }
    }

    /// The prefix of `layer`, for the two layers that select by name.
    pub(crate) fn for_layer(&self, layer: &str) -> Option<&str> {
        match layer {
            "property" => Some(&self.property),
            "acceptance" => Some(&self.acceptance),
            _ => None,
        }
    }
}

/// The shared nextest run, narrowed to the tests with a `::`-separated name
/// segment that starts with `filter` (HN-L2, a word of `[A-Za-z0-9_]`). The same rule reads the JUnit
/// report of a credited run, so credit equals running.
fn selected_tests(filter: &str) -> Vec<Step> {
    // `check` holds a filter to `[A-Za-z0-9_]+`, which is a regex literal.
    let expr = format!("test(/(^|::){filter}/)");
    let argv = NEXTEST.iter().map(|arg| (*arg).to_string()).chain([
        "--no-tests=fail".to_string(),
        "-E".to_string(),
        expr,
    ]);
    vec![Step::tool(argv)]
}

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
pub(crate) fn plan_for(
    layer: &config::Layer,
    report: &Path,
    base: &Base,
    filters: &Filters,
) -> Option<Plan> {
    if layer.id == "mutation" {
        Some(Plan::Mutation(MutationPlan::new(report, base.clone())))
    } else {
        tool_plan(layer, report, filters)
    }
}

/// The plan of every layer but `mutation`, which also needs a base.
fn tool_plan(layer: &config::Layer, report: &Path, filters: &Filters) -> Option<Plan> {
    let id = layer.id.as_str();
    match id {
        "coverage" => Some(Plan::Chains(vec![steps::coverage_steps(report)])),
        "crap" => Some(Plan::Chains(vec![crap_steps(report)])),
        "property" | "acceptance" => filters
            .for_layer(id)
            .map(|filter| Plan::Chains(vec![selected_tests(filter)])),
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
    /// The name prefixes of `property` and `acceptance`.
    pub(crate) filters: Filters,
    /// The pinned tools each running layer needs, asked before the layer's
    /// commands run. A layer with none is absent.
    pub(crate) pins: LayerPins,
    /// What to say about keys of the lock that nothing will check.
    pub(crate) pin_warnings: Vec<String>,
    pub(crate) units: Vec<Unit<'a>>,
}

impl GatePlan<'_> {
    /// The pinned tools `layer` needs, as the argv that asks each for its
    /// version, once per argv.
    pub(crate) fn asked(&self, layer: &str) -> Vec<Vec<String>> {
        let mut asked: Vec<Vec<String>> = Vec::new();
        for (row, _) in self.pins_of(layer) {
            let argv: Vec<String> = row.argv.iter().map(|word| (*word).to_string()).collect();
            if !asked.contains(&argv) {
                asked.push(argv);
            }
        }
        asked
    }

    /// The pinned tools `layer` needs, each with its pin.
    pub(crate) fn pins_of(&self, layer: &str) -> &[(&'static Row, String)] {
        self.pins
            .iter()
            .find(|(id, _)| id == layer)
            .map_or(&[], |(_, pins)| pins.as_slice())
    }

    /// The commands of `layer` as argv lists, in the order they run.
    pub(crate) fn argv_of(&self, layer: &str) -> Vec<Vec<String>> {
        let Some(unit) = self.units.iter().find(|unit| unit.layer.id == layer) else {
            return Vec::new();
        };
        match &unit.what {
            What::Chains(chains) => self
                .asked(layer)
                .into_iter()
                .chain(chains.iter().flatten().map(|step| step.argv.clone()))
                .collect(),
            What::Mutation(plan) => self
                .asked(layer)
                .into_iter()
                .chain([plan.diff.argv.clone(), plan.mutants.argv.clone()])
                .collect(),
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
    filters: &Filters,
) -> Vec<Unit<'a>> {
    enabled
        .iter()
        .copied()
        .filter(|layer| layer.id != "laws")
        .map(|layer| {
            let what = match (
                tests.shared_from(&layer.id),
                plan_for(layer, report, base, filters),
            ) {
                (Some(from), _) => What::Shared(from),
                (None, Some(Plan::Chains(chains))) => What::Chains(chains),
                (None, Some(Plan::Mutation(plan))) => What::Mutation(plan),
                (None, None) => What::NotRun,
            };
            Unit { layer, what }
        })
        .collect()
}

/// The pinned tools each unit that runs a command needs, and the warnings
/// about the lock's keys that nothing will check. The lock is read only when
/// some such layer needs a tool; one that cannot be read is a
/// [`Fault::config`].
fn pins_of_units(dir: &Path, units: &[Unit<'_>]) -> Result<(LayerPins, Vec<String>), Fault> {
    let running: Vec<&str> = units
        .iter()
        .filter(|unit| matches!(unit.what, What::Chains(_) | What::Mutation(_)))
        .map(|unit| unit.layer.id.as_str())
        .filter(|id| !lock::tools_for([*id]).is_empty())
        .collect();
    if running.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let pins = lock_pins(dir).map_err(Fault::config)?;
    let warnings = lock::pin_warnings(&pins, &lock::tools_for(running.iter().copied()));
    let layers = running
        .into_iter()
        .map(|id| (id.to_string(), lock::pins_for([id], &pins)))
        .filter(|(_, needed)| !needed.is_empty())
        .collect();
    Ok((layers, warnings))
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
    let filters = Filters::of(&cfg.gates);
    let tests = TestCommands::new(&enabled, &report, &filters);
    let base = base_for(&enabled, dir, base_flag, cfg.gates.base_ref.as_deref());
    let units = units_of(&enabled, &tests, &report, &base, &filters);
    let (pins, pin_warnings) = pins_of_units(dir, &units)?;
    Ok(GatePlan {
        max_tier,
        laws_on: enabled.iter().any(|l| l.id == "laws"),
        // Enabled layers above the range are not judged, so they cannot
        // fail the gate. The run names them.
        above: ids(cfg, |l| l.enabled && crate::tiers::tier_for(l) > max_tier),
        // A layer turned off still shows in the run: it is not run, not omitted.
        disabled: ids(cfg, |l| !l.enabled),
        units,
        enabled,
        report,
        tests,
        filters,
        pins,
        pin_warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        base_for, pins_of_units, plan_for, Filters, GatePlan, Plan, TestCommands, Unit, What,
    };
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
    fn the_selecting_layers_run_only_tests_with_a_name_prefix() {
        let filters = Filters {
            property: "prop_".to_string(),
            acceptance: "journey_9".to_string(),
        };
        let shared = ["cargo", "nextest", "run", "--workspace", "--all-features"]
            .into_iter()
            .chain(["--profile", "ci", "--no-tests=fail", "-E"]);
        for (id, expr) in [
            ("property", "test(/(^|::)prop_/)"),
            ("acceptance", "test(/(^|::)journey_9/)"),
        ] {
            let Some(Plan::Chains(chains)) =
                plan_for(&layer(id), &PathBuf::new(), &Base::unused(), &filters)
            else {
                panic!("{id} has no plan");
            };
            let argv: Vec<String> = chains
                .into_iter()
                .flatten()
                .flat_map(|step| step.argv)
                .collect();
            let want: Vec<String> = shared.clone().chain([expr]).map(str::to_string).collect();
            assert_eq!(argv, want, "{id}");
        }
    }

    #[test]
    fn only_the_two_selecting_layers_have_a_filter() {
        let filters = Filters::default();
        assert_eq!(filters.for_layer("property"), Some("property"));
        assert_eq!(filters.for_layer("acceptance"), Some("acceptance"));
        assert_eq!(filters.for_layer("unit"), None);
        assert_eq!(filters.for_layer("integration"), None);
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
            filters: super::Filters::default(),
            pins: Vec::new(),
            pin_warnings: Vec::new(),
            units: Vec::new(),
        };
        assert!(plan.argv_of("static-lint").is_empty());
    }

    #[test]
    fn the_lock_is_read_only_when_a_running_layer_needs_a_pinned_tool() {
        let dir = std::env::temp_dir().join(format!(
            "cargo-tops-plan-lock-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("rust-tops.lock"), "[tools\n").expect("write lock");
        // No tool of a layer that needs none, and a layer that runs nothing,
        // read nothing: a lock that does not parse is not in the way.
        let miri = layer("miri");
        let none = vec![Unit {
            layer: &miri,
            what: What::NotRun,
        }];
        assert!(pins_of_units(&dir, &none)
            .is_ok_and(|(pins, warnings)| pins.is_empty() && warnings.is_empty()));
        // A running layer that needs a pinned tool reads it.
        let lint = layer("static-lint");
        let some = vec![Unit {
            layer: &lint,
            what: What::Chains(Vec::new()),
        }];
        assert!(pins_of_units(&dir, &some).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
