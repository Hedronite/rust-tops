//! Whether a gate layer runs its own command or takes another layer's outcome.
//!
//! This module does not spawn commands. The gate calls [`runs_itself`] before
//! a layer and [`recorded_outcome`] after the source layer has finished.
//! A missing or failed coverage run is not a pass. A coverage command that
//! lacks `--workspace`, `--all-features`, or the nextest profile grants no
//! credit, and `unit`, `integration`, and `property` run themselves.

#![allow(dead_code, reason = "main.rs calls the credit decision")]

/// Nextest profile a coverage command must pass for credit.
pub const NEXTEST_PROFILE: &str = "ci";

/// What the gate recorded for a layer it already ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed,
    ToolMissing,
}

/// Inputs for one layer. Argv is the command that layer would run.
pub struct Query<'a> {
    pub layer: &'a str,
    pub coverage_in_range: bool,
    pub coverage_argv: &'a [String],
    /// `None` until coverage has finished. That is not a pass.
    pub coverage_outcome: Option<Outcome>,
    pub unit_argv: &'a [String],
    pub integration_argv: &'a [String],
    /// `None` until unit has finished. That is not a pass.
    pub unit_outcome: Option<Outcome>,
}

/// Coverage may stand in for unit, integration, and property only when its
/// command carries the workspace, all-features, and nextest-profile flags.
#[must_use]
pub fn coverage_grants_credit(argv: &[String]) -> bool {
    let tokens = tokens(argv);
    has(&tokens, "--workspace")
        && has(&tokens, "--all-features")
        && has_profile(&tokens, NEXTEST_PROFILE)
}

/// `true` when `layer` adds its own command.
#[must_use]
pub fn runs_itself(query: &Query<'_>) -> bool {
    !takes_coverage(query) && !takes_unit(query)
}

/// Outcome to record when [`runs_itself`] is false.
///
/// A missing source is [`Outcome::Failed`], never [`Outcome::Passed`].
/// [`None`] when the layer runs itself.
#[must_use]
pub fn recorded_outcome(query: &Query<'_>) -> Option<Outcome> {
    if takes_coverage(query) {
        return Some(query.coverage_outcome.unwrap_or(Outcome::Failed));
    }
    if takes_unit(query) {
        return Some(query.unit_outcome.unwrap_or(Outcome::Failed));
    }
    None
}

fn takes_coverage(query: &Query<'_>) -> bool {
    query.coverage_in_range
        && coverage_grants_credit(query.coverage_argv)
        && matches!(query.layer, "unit" | "integration" | "property")
}

fn takes_unit(query: &Query<'_>) -> bool {
    !query.coverage_in_range
        && query.layer == "integration"
        && same_command(query.unit_argv, query.integration_argv)
}

fn same_command(left: &[String], right: &[String]) -> bool {
    tokens(left) == tokens(right)
}

fn tokens(argv: &[String]) -> Vec<&str> {
    argv.iter().flat_map(|arg| arg.split_whitespace()).collect()
}

fn has(tokens: &[&str], flag: &str) -> bool {
    tokens.contains(&flag)
}

fn has_profile(tokens: &[&str], profile: &str) -> bool {
    let paired = tokens
        .windows(2)
        .any(|pair| pair[0] == "--profile" && pair[1] == profile);
    let inline = tokens.iter().any(|token| {
        token
            .strip_prefix("--profile=")
            .is_some_and(|value| value == profile)
    });
    paired || inline
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    fn qualifying() -> Vec<String> {
        args(&[
            "cargo",
            "llvm-cov",
            "nextest",
            "--workspace",
            "--all-features",
            "--lcov",
            "--profile",
            "ci",
        ])
    }

    fn unit() -> Vec<String> {
        args(&[
            "cargo",
            "nextest",
            "run",
            "--workspace",
            "--all-features",
            "--profile",
            "ci",
        ])
    }

    fn property() -> Vec<String> {
        let mut argv = unit();
        argv.push("property".into());
        argv
    }

    fn query<'a>(
        layer: &'a str,
        coverage: &'a [String],
        in_range: bool,
        unit_argv: &'a [String],
        integration_argv: &'a [String],
    ) -> Query<'a> {
        Query {
            layer,
            coverage_in_range: in_range,
            coverage_argv: coverage,
            coverage_outcome: None,
            unit_argv,
            integration_argv,
            unit_outcome: None,
        }
    }

    #[test]
    fn a_qualifying_coverage_run_is_the_outcome_of_the_three() {
        let coverage = qualifying();
        let unit_argv = unit();
        let property_argv = property();
        for layer in ["unit", "integration", "property"] {
            let integration_argv = if layer == "property" {
                &property_argv
            } else {
                &unit_argv
            };
            let mut q = query(layer, &coverage, true, &unit_argv, integration_argv);
            assert!(!runs_itself(&q), "{layer}");
            q.coverage_outcome = Some(Outcome::Passed);
            assert_eq!(recorded_outcome(&q), Some(Outcome::Passed));
            q.coverage_outcome = Some(Outcome::Failed);
            assert_eq!(recorded_outcome(&q), Some(Outcome::Failed));
            q.coverage_outcome = Some(Outcome::ToolMissing);
            assert_eq!(recorded_outcome(&q), Some(Outcome::ToolMissing));
            q.coverage_outcome = None;
            assert_eq!(recorded_outcome(&q), Some(Outcome::Failed));
        }
    }

    fn without(argv: &[String], flag: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut skip_next = false;
        for arg in argv {
            if skip_next {
                skip_next = false;
                continue;
            }
            if arg == flag {
                skip_next = flag == "--profile";
                continue;
            }
            out.push(arg.clone());
        }
        out
    }

    #[test]
    fn a_coverage_command_missing_a_required_flag_grants_no_credit() {
        let full = qualifying();
        let unit_argv = unit();
        for flag in ["--workspace", "--all-features", "--profile"] {
            let coverage = without(&full, flag);
            assert!(!coverage_grants_credit(&coverage), "{flag}");
            for layer in ["unit", "integration", "property"] {
                let q = query(layer, &coverage, true, &unit_argv, &unit_argv);
                assert!(runs_itself(&q), "{layer} missing {flag}");
                assert_eq!(recorded_outcome(&q), None);
            }
        }
        let mut equals = qualifying();
        equals.retain(|arg| arg != "--profile");
        equals.push("--profile=ci".into());
        assert!(coverage_grants_credit(&equals));
    }

    #[test]
    fn below_coverage_unit_runs_once_and_property_runs_itself() {
        let coverage = qualifying();
        let unit_argv = unit();
        let property_argv = property();
        let unit_q = query("unit", &coverage, false, &unit_argv, &unit_argv);
        assert!(runs_itself(&unit_q));
        let integration = query("integration", &coverage, false, &unit_argv, &unit_argv);
        assert!(!runs_itself(&integration));
        let mut with_unit = integration;
        with_unit.unit_outcome = Some(Outcome::Passed);
        assert_eq!(recorded_outcome(&with_unit), Some(Outcome::Passed));
        with_unit.unit_outcome = Some(Outcome::Failed);
        assert_eq!(recorded_outcome(&with_unit), Some(Outcome::Failed));
        let different = query("integration", &coverage, false, &unit_argv, &property_argv);
        assert!(runs_itself(&different));
        let property_q = query("property", &coverage, false, &unit_argv, &unit_argv);
        assert!(runs_itself(&property_q));
    }
}
