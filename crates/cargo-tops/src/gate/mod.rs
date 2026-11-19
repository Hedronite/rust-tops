//! `cargo tops gate`: plan, execute, judge, report.
//!
//! - [`plan`] decides which layers run and with what, and runs nothing.
//! - [`execute`] runs the commands and times them.
//! - [`judge`] turns the runs, the floors and the laws into problems.
//! - [`report`] prints the summary, writes `report.json` and picks the exit
//!   code.
//!
//! Exit codes: 0 pass; 1 the gate ran and failed (a dry run included); 2 the
//! config is invalid; 3 the environment (a required tool or `cargo metadata`
//! is missing). Where more than one applies, 2 wins over 3, and 3 over 1;
//! every problem is still listed in the report.
use std::path::Path;
use std::process::ExitCode;

use crate::config::TopsConfig;
use crate::laws;

pub(crate) mod execute;
pub(crate) mod judge;
pub(crate) mod plan;
pub(crate) mod report;

/// Whether the gate runs its commands or only lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Run,
    DryRun,
}

impl From<bool> for Mode {
    /// `true` is `--dry-run`.
    fn from(dry_run: bool) -> Self {
        if dry_run {
            Self::DryRun
        } else {
            Self::Run
        }
    }
}

/// What `gate` was asked for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Options {
    pub(crate) max_tier: u8,
    pub(crate) mode: Mode,
}

/// Why a gate could not reach a verdict from its commands.
pub(crate) struct Fault {
    /// `true` for the config (exit 2), `false` for the environment (exit 3).
    pub(crate) config: bool,
    pub(crate) error: anyhow::Error,
    /// The message is already on stdout.
    pub(crate) announced: bool,
}

impl Fault {
    pub(crate) fn config(error: anyhow::Error) -> Self {
        Self {
            config: true,
            error,
            announced: false,
        }
    }

    pub(crate) fn env(error: anyhow::Error) -> Self {
        Self {
            config: false,
            error,
            announced: false,
        }
    }

    /// An environment fault the run has already printed.
    pub(crate) fn announced(message: String) -> Self {
        Self {
            announced: true,
            ..Self::env(anyhow::anyhow!(message))
        }
    }
}

/// Run the gate for `cfg` under `dir`: always writes `report.json`.
pub(crate) fn run(dir: &Path, cfg: &TopsConfig, opts: Options) -> ExitCode {
    // A gate resolves `cargo metadata` afresh, once per project.
    laws::begin_gate();
    let plan = match plan::build(dir, cfg, opts.max_tier) {
        Ok(plan) => plan,
        Err(fault) => return report::fault(dir, Some(cfg), opts, None, &fault),
    };
    // A report from an earlier gate must never read as this one's.
    report::clear(&plan.report);
    println!(
        "rust-tops gate: {} ({}) max-tier={}",
        cfg.crate_.name, cfg.crate_.class, opts.max_tier
    );
    let prepared = match execute::prepare(dir, &plan, opts.mode) {
        Ok(prepared) => prepared,
        Err(fault) => return report::fault(dir, Some(cfg), opts, Some(&plan), &fault),
    };
    report::announce_skipped(&plan);
    let done = match execute::run(dir, &plan, opts.mode) {
        Ok(done) => done,
        Err(fault) => return report::fault(dir, Some(cfg), opts, Some(&plan), &fault),
    };
    match judge::judge(dir, cfg, &plan, &done, prepared, opts.mode) {
        Ok(judged) => report::finish(dir, cfg, opts, &plan, &done, &judged),
        Err(fault) => report::fault(dir, Some(cfg), opts, Some(&plan), &fault),
    }
}

/// The config could not be loaded or failed `check`: exit 2. The report is
/// written when `dir` exists, so the target is known.
pub(crate) fn config_error(dir: &Path, error: &anyhow::Error, opts: Options) -> ExitCode {
    laws::begin_gate();
    eprintln!("error: {error:#}");
    report::config_invalid(dir, error, opts)
}

#[cfg(test)]
mod tests {
    use super::judge::{command_count, ran_commands};
    use super::plan::{plan_for, Plan, TestCommands};
    use crate::facts::{Outcome, Run};
    use crate::steps::{self, Kind, Step};
    use crate::{config, credit};
    use std::fs;
    use std::path::Path;

    fn layer(id: &str) -> config::Layer {
        serde_saphyr::from_str(&format!(
            "{{ id: {id}, enabled: true, tier: 0, runner: x }}"
        ))
        .expect("layer")
    }

    fn chains(id: &str) -> Vec<Vec<Step>> {
        match plan_for(&layer(id), Path::new("/r")) {
            Some(Plan::Chains(chains)) => chains,
            _ => panic!("{id} has no chains"),
        }
    }

    fn tool_step(chain: &[Step]) -> &Step {
        chain
            .iter()
            .find(|step| step.kind == Kind::Tool)
            .expect("a tool step")
    }

    #[test]
    fn the_g0_strings_name_the_tool_they_run() {
        let coverage = "mkdir -p target/coverage && cargo llvm-cov nextest --workspace --all-features --profile ci --lcov --output-path target/coverage/lcov.info && test -s target/coverage/lcov.info";
        let mutation = "sh -c 'git diff origin/main...HEAD --unified=0 > /tmp/rust-tops.diff; cargo mutants --in-diff /tmp/rust-tops.diff -j 2'";
        assert_eq!(
            steps::tool_binary(&[coverage.to_string()]).as_deref(),
            Some("cargo-llvm-cov")
        );
        assert_eq!(
            steps::tool_binary(&[mutation.to_string()]).as_deref(),
            Some("cargo-mutants")
        );
    }

    #[test]
    fn coverage_and_crap_check_their_own_binary() {
        let coverage = chains("coverage");
        assert_eq!(coverage.len(), 1);
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_1285_44 = coverage[0].clone();
        assert_eq!(
            steps::tool_binary(&tool_step(&tops_idx_1285_44).argv).as_deref(),
            Some("cargo-llvm-cov")
        );
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_1293_13 = coverage[0].clone();
        assert_eq!(
            tops_idx_1293_13.last().map(|step| step.kind),
            Some(Kind::Artifact)
        );
        let crap = chains("crap");
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_1302_44 = crap[0].clone();
        assert_eq!(
            steps::tool_binary(&tool_step(&tops_idx_1302_44).argv).as_deref(),
            Some("cargo-crap")
        );
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_1310_13 = crap[0].clone();
        assert_eq!(
            tops_idx_1310_13.last().and_then(Step::artifact_path),
            Some(Path::new("/r/crap.json"))
        );
    }

    #[test]
    fn coverage_and_crap_write_under_the_artifact_dir() {
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let coverage: Vec<String> = chains("coverage")[0].iter().map(Step::display).collect();
        assert_eq!(
            coverage,
            [
                "mkdir -p /r",
                "cargo llvm-cov nextest --workspace --all-features --profile ci --lcov --output-path /r/lcov.info",
                "test -s /r/lcov.info",
                "cargo llvm-cov report --json --summary-only --output-path /r/coverage.json",
                "test -s /r/coverage.json",
            ]
        );
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let crap: Vec<String> = chains("crap")[0].iter().map(Step::display).collect();
        assert_eq!(
            crap,
            [
                "cargo crap --lcov /r/lcov.info --format json --output /r/crap.json",
                "test -s /r/crap.json",
            ]
        );
    }

    fn argv(words: &str) -> Vec<String> {
        words.split_whitespace().map(str::to_string).collect()
    }

    const COVERAGE: &str = "cargo llvm-cov nextest --workspace --all-features --profile ci --lcov";
    const UNIT: &str = "cargo nextest run --workspace --all-features --profile ci";

    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "already in this crate when the strict table was turned on"
    )]
    fn tests_with(in_range: bool, coverage: &str, integration: &str) -> TestCommands {
        TestCommands {
            coverage_in_range: in_range,
            coverage: argv(coverage),
            unit: argv(UNIT),
            integration: argv(integration),
        }
    }

    fn run(layer: &str, outcome: Outcome) -> Run {
        Run {
            layer: layer.to_string(),
            cmd: format!("{layer} cmd"),
            step: format!("{layer} step"),
            outcome,
        }
    }

    #[test]
    fn the_shipped_test_commands_share_one_coverage_run() {
        let layers: Vec<config::Layer> = ["unit", "integration", "coverage", "property"]
            .into_iter()
            .map(layer)
            .collect();
        let enabled: Vec<&config::Layer> = layers.iter().collect();
        let tests = TestCommands::new(&enabled, Path::new("/r"));
        assert!(tests.coverage_in_range);
        assert!(
            credit::coverage_grants_credit(&tests.coverage),
            "{:?}",
            tests.coverage
        );
        assert_eq!(tests.unit, tests.integration);
        for id in ["unit", "integration", "property"] {
            assert_eq!(tests.shared_from(id), Some("coverage"), "{id}");
        }
        for id in ["coverage", "doc", "static-lint", "crap"] {
            assert_eq!(tests.shared_from(id), None, "{id}");
        }
        // Out of range, nothing names a coverage command and none is shared.
        let unit_only: Vec<&config::Layer> = enabled
            .iter()
            .copied()
            .filter(|l| l.id != "coverage")
            .collect();
        let tests = TestCommands::new(&unit_only, Path::new("/r"));
        assert!(!tests.coverage_in_range);
        assert!(tests.coverage.is_empty());
        assert_eq!(tests.shared_from("integration"), Some("unit"));
        assert_eq!(tests.shared_from("unit"), None);
        assert_eq!(tests.shared_from("property"), None);
    }

    #[test]
    fn a_coverage_command_missing_a_flag_shares_nothing() {
        for missing in ["--workspace", "--all-features", "--profile ci"] {
            let coverage = COVERAGE.replace(&format!(" {missing}"), "");
            let tests = tests_with(true, &coverage, UNIT);
            for id in ["unit", "integration", "property"] {
                assert_eq!(tests.shared_from(id), None, "{missing}: {id}");
            }
        }
        let tests = tests_with(
            true,
            &COVERAGE.replace("--profile ci", "--profile default"),
            UNIT,
        );
        assert_eq!(tests.shared_from("unit"), None);
    }

    #[test]
    fn integration_takes_unit_only_when_its_command_is_unit_s() {
        let tests = tests_with(false, COVERAGE, UNIT);
        assert_eq!(tests.shared_from("integration"), Some("unit"));
        let tests = tests_with(false, COVERAGE, &format!("{UNIT} --test it"));
        assert_eq!(tests.shared_from("integration"), None);
        // Unit off or out of range: no unit command, so integration runs.
        let mut tests = tests_with(false, COVERAGE, UNIT);
        tests.unit.clear();
        assert_eq!(tests.shared_from("integration"), None);
    }

    #[test]
    fn a_shared_layer_records_its_source_run_and_no_source_is_no_pass() {
        let tests = tests_with(true, COVERAGE, UNIT);
        for outcome in [Outcome::Passed, Outcome::Failed, Outcome::ToolMissing] {
            let took = tests.take("unit", "coverage", &[run("coverage", outcome)]);
            assert_eq!(took.outcome, outcome);
            assert_eq!(took.layer, "unit");
            assert_eq!(took.cmd, "coverage cmd");
            assert_eq!(took.step, "coverage step");
        }
        let took = tests.take("unit", "coverage", &[run("coverage", Outcome::Vacuous)]);
        assert_eq!(took.outcome, Outcome::Failed);
        let took = tests.take("property", "coverage", &[run("unit", Outcome::Passed)]);
        assert_eq!(took.outcome, Outcome::Failed);
        assert_eq!(took.cmd, "");
        let tests = tests_with(false, COVERAGE, UNIT);
        let took = tests.take("integration", "unit", &[run("unit", Outcome::Failed)]);
        assert_eq!(took.outcome, Outcome::Failed);
        assert_eq!(took.cmd, "unit cmd");
    }

    #[test]
    fn a_shared_run_is_not_a_command_and_its_source_is_named() {
        let runs = [
            run("coverage", Outcome::Passed),
            run("unit", Outcome::Passed),
            run("doc", Outcome::ToolMissing),
        ];
        assert_eq!(ran_commands(&runs, false, &[("unit", "coverage")]), 1);
        assert_eq!(ran_commands(&runs, true, &[]), 3);
        let layers: Vec<config::Layer> =
            ["coverage", "unit", "doc"].into_iter().map(layer).collect();
        let enabled: Vec<&config::Layer> = layers.iter().collect();
        let (line, short) = command_count(1, &enabled, &runs, false, 1, &[("unit", "coverage")]);
        assert!(short);
        assert_eq!(
            line,
            "count: 1 command(s) ran; 2 of 3 enabled layer(s) at tier <= 1 ran one; unit took the coverage run; none ran for: doc"
        );
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let (line, short) =
            command_count(2, &enabled[..2], &runs, false, 1, &[("unit", "coverage")]);
        assert!(!short);
        assert!(
            line.ends_with("ran one; unit took the coverage run"),
            "{line}"
        );
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let (line, _) = command_count(2, &enabled[..2], &runs, false, 1, &[("unit", "unit")]);
        assert!(line.ends_with("ran one; unit took the unit run"), "{line}");
    }

    #[test]
    fn the_mutation_diff_uses_merge_base_and_the_report_dir() {
        let report = Path::new("/work/target/rust-tops");
        let Some(Plan::Mutation(plan)) = plan_for(&layer("mutation"), report) else {
            panic!("mutation has no plan");
        };
        assert_eq!(plan.diff_path, report.join("rust-tops.diff"));
        let script = plan.diff.argv.join(" ");
        assert!(
            script.contains("git merge-base origin/main HEAD"),
            "{script}"
        );
        assert!(script.contains("refs/remotes/origin/main"), "{script}");
        assert!(script.contains("&& git diff"), "{script}");
        assert!(!script.contains("mutants"), "{script}");
        assert!(!script.contains("/tmp"), "{script}");
        assert!(plan
            .mutants
            .argv
            .contains(&"/work/target/rust-tops/rust-tops.diff".to_string()));
        assert_eq!(
            steps::tool_binary(&plan.mutants.argv).as_deref(),
            Some("cargo-mutants")
        );
    }

    #[test]
    fn no_mapped_layer_writes_under_tmp() {
        for id in [
            "static-lint",
            "unit",
            "integration",
            "doc",
            "coverage",
            "crap",
            "property",
            "security-advisory",
        ] {
            for chain in chains(id) {
                for step in chain {
                    assert!(!step.display().contains("/tmp"), "{id}: {}", step.display());
                }
            }
        }
    }

    #[test]
    fn unmapped_layers_have_no_plan() {
        for id in [
            "acceptance",
            "fuzz",
            "miri",
            "sanitizer",
            "bench-criterion",
            "bench-callgrind",
            "density-audit",
            "laws",
        ] {
            assert!(plan_for(&layer(id), Path::new("/r")).is_none(), "{id}");
        }
    }

    #[test]
    fn formal_kani_runs_cargo_kani() {
        let kani = chains("formal-kani");
        assert_eq!(kani.len(), 1);
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let step = tool_step(&kani[0]);
        assert_eq!(step.argv, ["cargo", "kani", "--workspace"]);
        assert_eq!(
            steps::tool_binary(&step.argv).as_deref(),
            Some("cargo-kani")
        );
    }

    #[test]
    fn a_failed_diff_is_failed_even_over_a_stale_diff() {
        let dir = std::env::temp_dir().join(format!("cargo-tops-diff-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("mkdir");
        let stale = dir.join("stale.diff");
        fs::write(&stale, "+changed\n").expect("write");
        let empty = dir.join("empty.diff");
        fs::write(&empty, "").expect("write");
        assert_eq!(steps::judge_diff(false, &stale), Outcome::Failed);
        assert_eq!(steps::judge_diff(false, &empty), Outcome::Failed);
        assert_eq!(steps::judge_diff(true, &empty), Outcome::Vacuous);
        assert_eq!(steps::judge_diff(true, &stale), Outcome::Passed);
        assert_eq!(
            steps::judge_diff(true, &dir.join("absent")),
            Outcome::Failed
        );
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&dir);
        assert!(!Outcome::Vacuous.fails_the_gate());
        assert!(!Outcome::Vacuous.counts_for_a_law());
    }

    #[test]
    fn only_a_fault_the_run_already_printed_is_announced() {
        let said = super::Fault::announced("said".to_string());
        assert!(said.announced && !said.config);
        assert!(!super::Fault::env(anyhow::anyhow!("x")).announced);
        assert!(!super::Fault::config(anyhow::anyhow!("x")).announced);
    }
}
