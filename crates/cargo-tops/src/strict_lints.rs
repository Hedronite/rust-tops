//! The strict-table check. Every `clippy.toml` key names an enabled lint,
//! and every `// expect:` marker has to match a diagnostic on that line.
#![allow(dead_code, reason = "called from the unit tests")]

use std::collections::{BTreeMap, BTreeSet};

/// One compiler diagnostic: 1-based line and lint code (`clippy::todo`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Diag {
    pub line: usize,
    pub lint: String,
}

/// Lints `clippy::all` turns on that this kit also configures.
/// `too_many_lines` is pedantic and `cognitive_complexity` is nursery, so
/// `all` does not enable them. Name those in the table, or drop their keys.
const ALL_GROUP_LINTS: [&str; 2] = ["too_many_arguments", "ptr_arg"];

#[expect(
    clippy::fn_params_excessive_bools,
    reason = "already in this crate when the strict table was turned on"
)]
/// Lints the `[lints.clippy]` table turns on for a class.
/// `[lib]` lints are off when `library_only` is false.
/// `all` at warn or deny also enables the threshold lints the kit configures.
pub(crate) fn enabled_clippy_lints(table: &str, library_only: bool) -> BTreeSet<String> {
    let mut enabled = BTreeSet::new();
    let mut all_on = false;
    let mut in_clippy = false;
    for raw in table.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_clippy = line == "[lints.clippy]";
            continue;
        }
        if !in_clippy || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, rest)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if !library_only && line.contains("[lib]") {
            continue;
        }
        let level = rest.split('"').nth(1).unwrap_or("");
        let on = level == "warn" || level == "deny";
        if name == "all" {
            all_on = on;
            continue;
        }
        if on {
            enabled.insert(name.to_string());
        }
    }
    if all_on {
        enabled.extend(ALL_GROUP_LINTS.map(str::to_string));
    }
    enabled
}

/// Keys in a `clippy.toml` body. Comments and blank lines are skipped.
pub(crate) fn clippy_toml_keys(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|raw| {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            line.split_once('=').map(|(key, _)| key.trim().to_string())
        })
        .collect()
}

/// The lint a `clippy.toml` key configures, when it has one.
pub(crate) fn key_lint_name(key: &str) -> Option<&'static str> {
    Some(match key {
        "allow-expect-in-tests" => "expect_used",
        "allow-unwrap-in-tests" => "unwrap_used",
        "allow-dbg-in-tests" => "dbg_macro",
        "allow-print-in-tests" => "print_stdout",
        "max-fn-params-bools" => "fn_params_excessive_bools",
        "cognitive-complexity-threshold" => "cognitive_complexity",
        "too-many-lines-threshold" => "too_many_lines",
        "too-many-arguments-threshold" => "too_many_arguments",
        "type-complexity-threshold" => "type_complexity",
        _ => return None,
    })
}

#[expect(
    clippy::fn_params_excessive_bools,
    reason = "already in this crate when the strict table was turned on"
)]
/// Keys that do not name an enabled lint for this class.
pub(crate) fn unexplained_keys(table: &str, clippy_toml: &str, library_only: bool) -> Vec<String> {
    let enabled = enabled_clippy_lints(table, library_only);
    clippy_toml_keys(clippy_toml)
        .into_iter()
        .filter(|key| match key_lint_name(key) {
            Some(lint) => !enabled.contains(lint),
            None => true,
        })
        .collect()
}

/// `// expect: clippy::lint` markers, keyed by 1-based line.
pub(crate) fn expect_markers(source: &str) -> BTreeMap<usize, BTreeSet<String>> {
    let mut expected: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    for (index, line) in source.lines().enumerate() {
        let line_no = index + 1;
        for part in line.split("// expect:").skip(1) {
            let lint = part
                .trim()
                .split([',', ' '])
                .next()
                .unwrap_or("")
                .trim()
                .trim_end_matches(',');
            if !lint.is_empty() {
                expected
                    .entry(line_no)
                    .or_default()
                    .insert(lint.to_string());
            }
        }
    }
    expected
}

/// Every marker fires on its line, and nothing unmarked fires.
pub(crate) fn prove_markers(source: &str, diags: &[Diag]) -> Result<(), Vec<String>> {
    let expected = expect_markers(source);
    let mut fired: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    for diag in diags {
        fired
            .entry(diag.line)
            .or_default()
            .insert(diag.lint.clone());
    }
    let mut errors = Vec::new();
    for (line, lints) in &expected {
        for lint in lints {
            let hit = fired.get(line).is_some_and(|set| set.contains(lint));
            if !hit {
                errors.push(format!("marker {lint} on line {line} did not fire"));
            }
        }
    }
    for (line, lints) in &fired {
        for lint in lints {
            let marked = expected.get(line).is_some_and(|set| set.contains(lint));
            if !marked {
                errors.push(format!("unmarked {lint} on line {line}"));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// One `kind = "lint"` rule from `rules.toml`: its ID and the lint it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LintRule {
    pub id: String,
    pub check: String,
}

/// The `kind = "lint"` rules in a `rules.toml` body.
/// Probe and judgment rules are skipped. A lint rule with no `id` or no
/// `check` is an error, since nothing could prove it.
pub(crate) fn lint_rules(catalog: &str) -> Result<Vec<LintRule>, String> {
    let value: toml::Table = toml::from_str(catalog).map_err(|err| format!("rules.toml: {err}"))?;
    let rules = value
        .get("rule")
        .and_then(toml::Value::as_array)
        .ok_or("rules.toml: no [[rule]] tables")?;
    let mut lints = Vec::new();
    for rule in rules {
        let field = |key: &str| rule.get(key).and_then(toml::Value::as_str);
        if field("kind") != Some("lint") {
            continue;
        }
        match (field("id"), field("check")) {
            (Some(id), Some(check)) if !check.is_empty() => lints.push(LintRule {
                id: id.to_string(),
                check: check.to_string(),
            }),
            _ => {
                return Err(format!(
                    "rules.toml: a lint rule needs an id and a check: {rule}"
                ))
            }
        }
    }
    Ok(lints)
}

/// Lint rules whose `check` has no `// expect: <check>` marker in the probe.
pub(crate) fn missing_probe_lines(rules: &[LintRule], probe: &str) -> Vec<String> {
    let marked: BTreeSet<String> = expect_markers(probe).into_values().flatten().collect();
    rules
        .iter()
        .filter(|rule| !marked.contains(&rule.check))
        .map(|rule| {
            format!(
                "{}: no `// expect: {}` line in the probe",
                rule.id, rule.check
            )
        })
        .collect()
}

/// A lint name as the compiler prints it in a diagnostic: underscores.
fn normalize_lint(name: &str) -> String {
    name.replace('-', "_")
}

/// Lint names rustc rejected with `E0602` (unknown lint) in
/// `cargo clippy --message-format=json` output. Run clippy with
/// `-W <check>` for each lint rule, and a name clippy does not know shows up here.
pub(crate) fn rejected_lints(output: &str) -> BTreeSet<String> {
    output
        .lines()
        .filter_map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).ok()?;
            let message = value.get("message")?;
            if message.pointer("/code/code")?.as_str()? != "E0602" {
                return None;
            }
            let text = message.get("message")?.as_str()?;
            Some(text.split('`').nth(1)?.to_string())
        })
        .collect()
}

/// Lint rules whose `check` clippy rejected as an unknown lint.
pub(crate) fn unknown_lints(rules: &[LintRule], rejected: &BTreeSet<String>) -> Vec<String> {
    rules
        .iter()
        .filter(|rule| rejected.contains(&rule.check))
        .map(|rule| format!("{}: clippy does not know {}", rule.id, rule.check))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        expect_markers, lint_rules, missing_probe_lines, prove_markers, rejected_lints,
        unexplained_keys, unknown_lints, Diag,
    };

    const TABLE: &str = include_str!("../templates/lints/tops-strict.toml");
    const CLIPPY: &str = include_str!("../templates/drop-in/clippy.toml");
    const CATALOG: &str = include_str!("../templates/skills/rust-tops-style/rules.toml");
    const PROBE: &str = include_str!("../templates/lints/tops-strict-probe.inc");

    #[test]
    fn shipped_clippy_toml_keys_name_enabled_lints_in_every_class() {
        for (name, library_only) in [
            ("library-core", true),
            ("library-util", true),
            ("ffi-bridge", true),
            ("no-std-embedded", true),
            ("parser-codec", true),
            ("unsafe-kernel", true),
            ("binary-cli", false),
            ("workspace-root", false),
        ] {
            assert_eq!(
                unexplained_keys(TABLE, CLIPPY, library_only),
                Vec::<String>::new(),
                "{name}"
            );
        }
        eprintln!("shipped_clippy_toml_keys_name_enabled_lints ok");
    }

    #[test]
    fn a_planted_inert_key_fails_the_check() {
        let planted = format!("{CLIPPY}\nnot-a-real-lint = 1\n");
        assert_eq!(
            unexplained_keys(TABLE, &planted, true),
            vec!["not-a-real-lint".to_string()]
        );
        eprintln!("a_planted_inert_key_fails_the_check ok (planted not-a-real-lint)");
    }

    #[test]
    fn a_comment_that_contains_equals_is_not_a_lint_or_a_key() {
        let enabled = super::enabled_clippy_lints(
            "[lints.rust]\noutside = \"warn\"\n[lints.clippy]\n# not = \"warn\"\n\nunwrap_used = \"warn\"\n",
            true,
        );
        assert_eq!(
            enabled,
            std::collections::BTreeSet::from(["unwrap_used".to_string()])
        );
        let keys = super::clippy_toml_keys("# fake = 1\nallow-unwrap-in-tests = true\n");
        assert_eq!(keys, vec!["allow-unwrap-in-tests".to_string()]);
    }

    #[test]
    fn every_marker_fires_on_its_line_and_removing_one_fails() {
        let source = "pub fn trick_todo() { todo!() } // expect: clippy::todo\n";
        let diags = vec![Diag {
            line: 1,
            lint: "clippy::todo".to_string(),
        }];
        assert_eq!(
            expect_markers(source)
                .get(&1)
                .unwrap()
                .iter()
                .next()
                .unwrap(),
            "clippy::todo"
        );
        assert!(prove_markers(source, &diags).is_ok());
        let stripped = source.replace(" // expect: clippy::todo", "");
        let err = prove_markers(&stripped, &diags).unwrap_err();
        assert!(err.iter().any(|item| item.contains("unmarked")), "{err:?}");
        let missing = prove_markers(source, &[]).unwrap_err();
        assert!(
            missing.iter().any(|item| item.contains("did not fire")),
            "{missing:?}"
        );
    }

    #[test]
    fn clippy_markers_match_the_template_and_removing_one_fails() {
        let source = include_str!("../templates/lints/tops-strict-probe.inc");
        let checks: Vec<String> = lint_rules(CATALOG)
            .unwrap()
            .into_iter()
            .map(|rule| rule.check)
            .collect();
        let diags = run_probe(source, &checks);
        let verdict = prove_markers(source, &diags);
        assert!(verdict.is_ok(), "{verdict:?}");
        let stripped = source.replacen(" // expect: clippy::todo", "", 1);
        let err = prove_markers(&stripped, &diags).unwrap_err();
        assert!(err.iter().any(|item| item.contains("unmarked")), "{err:?}");
        let (tricks, caught) = caught_tricks(source);
        eprintln!("the_probe_catches_at_least_thirteen_of_fifteen ok");
        eprintln!("caught {caught} of {tricks}");
        assert_eq!(tricks, 15);
        assert!(caught >= 13, "caught {caught} of {tricks}");
    }

    /// Lints clippy rejects as unknown when asked for with `-W`. Runs the same
    /// `cargo clippy` the probe test runs, so it cannot be skipped. Fails if
    /// clippy did not build the scratch crate, since silence would mean "all known".
    fn rejected_by_clippy(rules: &[super::LintRule]) -> std::collections::BTreeSet<String> {
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("tops-lint-known-{}-{call}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let manifest =
            "[package]\nname = \"tops-lint-known\"\nversion = \"0.0.0\"\nedition = \"2021\"\n";
        std::fs::write(dir.join("Cargo.toml"), manifest).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();
        let mut command = std::process::Command::new("cargo");
        command.args(["clippy", "--message-format=json", "--"]);
        for rule in rules {
            command.args(["-W", &rule.check]);
        }
        let output = command.current_dir(&dir).output().expect("cargo clippy");
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(
            text.contains("\"build-finished\""),
            "clippy did not run: {text}"
        );
        rejected_lints(&text)
    }

    fn lint_catalog(check: &str) -> String {
        format!("[[rule]]\nid = \"T-1\"\nkind = \"lint\"\ncheck = \"{check}\"\n")
    }

    #[test]
    fn every_lint_rule_has_a_probe_line_and_a_lint_clippy_knows() {
        let rules = lint_rules(CATALOG).unwrap();
        assert!(!rules.is_empty(), "the catalog has no lint rules");
        let missing = missing_probe_lines(&rules, PROBE);
        let unknown = unknown_lints(&rules, &rejected_by_clippy(&rules));
        assert!(
            missing.is_empty() && unknown.is_empty(),
            "no probe line: {missing:?}\nunknown to clippy: {unknown:?}"
        );
        eprintln!(
            "every_lint_rule_has_a_probe_line ok ({} lint rules)",
            rules.len()
        );
    }

    #[test]
    fn a_lint_rule_with_no_probe_line_fails() {
        let rules = lint_rules(&lint_catalog("clippy::absolute_paths")).unwrap();
        let missing = missing_probe_lines(&rules, PROBE);
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(
            missing.iter().all(|item| item.starts_with("T-1:")),
            "{missing:?}"
        );
        let marked = format!("{PROBE}\nfn t() {{}} // expect: clippy::absolute_paths\n");
        assert!(missing_probe_lines(&rules, &marked).is_empty());
        eprintln!("a_lint_rule_with_no_probe_line_fails ok (planted clippy::absolute_paths)");
    }

    #[test]
    fn a_lint_clippy_does_not_know_fails() {
        let rules = lint_rules(&lint_catalog("clippy::not_a_real_lint")).unwrap();
        let unknown = unknown_lints(&rules, &rejected_by_clippy(&rules));
        assert_eq!(
            unknown,
            vec!["T-1: clippy does not know clippy::not_a_real_lint"]
        );
        eprintln!("a_lint_clippy_does_not_know_fails ok (planted not_a_real_lint)");
    }

    #[test]
    fn probe_and_judgment_rules_need_no_probe_line() {
        let catalog = "[[rule]]\nid = \"T-1\"\nkind = \"probe\"\ncheck = \"p\"\n\
                       [[rule]]\nid = \"T-2\"\nkind = \"judgment\"\ncheck = \"\"\n";
        assert_eq!(lint_rules(catalog), Ok(Vec::new()));
    }

    #[test]
    fn a_lint_rule_with_no_check_or_a_bad_catalog_is_an_error() {
        let no_check = "[[rule]]\nid = \"T-1\"\nkind = \"lint\"\ncheck = \"\"\n";
        assert!(lint_rules(no_check).is_err());
        assert!(lint_rules("[[rule]\n").is_err());
        assert!(lint_rules("title = \"x\"\n").is_err());
    }

    #[test]
    fn rejected_lints_reads_only_unknown_lint_errors() {
        let output = concat!(
            "{\"reason\":\"compiler-message\",\"message\":{\"code\":{\"code\":\"E0602\"},\"message\":\"unknown lint: `clippy::nope`\"}}\n",
            "{\"reason\":\"compiler-message\",\"message\":{\"code\":{\"code\":\"E0133\"},\"message\":\"unknown lint: `x`\"}}\n",
            "{\"reason\":\"compiler-message\",\"message\":{\"code\":null,\"message\":\"x\"}}\n",
            "not json\n",
        );
        let rejected = rejected_lints(output);
        assert_eq!(rejected.into_iter().collect::<Vec<_>>(), ["clippy::nope"]);
    }

    /// Compiles the probe as `rust-tops-style` runs it: the strict table, then
    /// `-W` for every lint-kind catalog check, which reaches lints in neither
    /// the table nor `clippy::all`.
    fn run_probe(source: &str, catalog_checks: &[String]) -> Vec<Diag> {
        let dir = std::env::temp_dir().join(format!("tops-strict-probe-{}", std::process::id()));
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let manifest = format!(
            "[package]\nname = \"tops-strict-probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n{TABLE}"
        );
        assert!(manifest.contains(TABLE), "the tested table is the template");
        std::fs::write(dir.join("Cargo.toml"), &manifest).unwrap();
        std::fs::write(dir.join("clippy.toml"), CLIPPY).unwrap();
        std::fs::write(dir.join("src/lib.rs"), source).unwrap();
        let output = std::process::Command::new("cargo")
            .args(["clippy", "--message-format=json", "--", "-W", "clippy::all"])
            .args(
                catalog_checks
                    .iter()
                    .flat_map(|check| ["-W", check.as_str()]),
            )
            .current_dir(&dir)
            .output()
            .expect("cargo clippy");
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = std::fs::remove_dir_all(&dir);
        parse_diags(&text)
    }

    fn caught_tricks(source: &str) -> (usize, usize) {
        let tricks = source
            .lines()
            .filter(|line| line.contains("fn trick_"))
            .count();
        let caught = source
            .lines()
            .filter(|line| line.contains("fn trick_") && line.contains("// expect: clippy::"))
            .count();
        (tricks, caught)
    }

    #[test]
    fn library_profiles_keep_lib_lints_and_binaries_drop_them() {
        let libs = super::enabled_clippy_lints(TABLE, true);
        let bins = super::enabled_clippy_lints(TABLE, false);
        assert!(libs.contains("missing_errors_doc"), "{libs:?}");
        assert!(libs.contains("expect_used"));
        assert!(libs.contains("print_stdout"));
        assert!(!bins.contains("missing_errors_doc"), "{bins:?}");
        assert!(!bins.contains("expect_used"), "{bins:?}");
        assert!(!bins.contains("print_stdout"), "{bins:?}");
        assert!(bins.contains("unwrap_used"));
        assert!(bins.contains("too_many_lines"));
    }

    #[test]
    fn every_known_key_names_its_lint() {
        for (key, lint) in [
            ("allow-expect-in-tests", "expect_used"),
            ("allow-unwrap-in-tests", "unwrap_used"),
            ("allow-dbg-in-tests", "dbg_macro"),
            ("allow-print-in-tests", "print_stdout"),
            ("max-fn-params-bools", "fn_params_excessive_bools"),
            ("cognitive-complexity-threshold", "cognitive_complexity"),
            ("too-many-lines-threshold", "too_many_lines"),
            ("too-many-arguments-threshold", "too_many_arguments"),
            ("type-complexity-threshold", "type_complexity"),
        ] {
            assert_eq!(super::key_lint_name(key), Some(lint), "{key}");
        }
        assert_eq!(super::key_lint_name("not-a-real-lint"), None);
        assert_eq!(super::key_lint_name("avoid-breaking-exported-api"), None);
        assert_eq!(super::key_lint_name("check-private-items"), None);
    }

    #[test]
    fn clippy_all_does_not_enable_pedantic_or_nursery_lints() {
        let enabled = super::enabled_clippy_lints(
            "[lints.clippy]\nall = { level = \"warn\", priority = -1 }\n",
            true,
        );
        assert!(enabled.contains("too_many_arguments"), "{enabled:?}");
        assert!(enabled.contains("ptr_arg"));
        assert!(!enabled.contains("too_many_lines"), "{enabled:?}");
        assert!(!enabled.contains("cognitive_complexity"), "{enabled:?}");
        assert!(!enabled.contains("type_complexity"), "{enabled:?}");
    }

    #[test]
    fn an_allow_level_and_a_line_without_equals_are_not_enabled() {
        let enabled = super::enabled_clippy_lints(
            "[lints.clippy]\nnot a lint\nunwrap_used = \"allow\"\nexpect_used = \"deny\"\n",
            false,
        );
        assert_eq!(
            enabled,
            std::collections::BTreeSet::from(["expect_used".to_string()])
        );
        let keys = super::clippy_toml_keys("no-equals\n\n# comment = 1\n");
        assert!(keys.is_empty(), "{keys:?}");
    }

    #[test]
    fn the_kit_denies_too_many_lines_and_the_template_does_not() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let manifest = std::fs::read_to_string(&root).unwrap();
        assert!(manifest.contains("[workspace.lints.clippy]"), "{root:?}");
        assert!(manifest.contains("too_many_lines = \"deny\""));
        assert!(manifest.contains("indexing_slicing = \"warn\""));
        assert!(!manifest.contains("expect_used"));
        assert!(!manifest.contains("print_stdout"));
        assert!(!TABLE.contains("too_many_lines = \"deny\""));
        assert!(TABLE.contains("too_many_lines = \"warn\""));
        assert!(TABLE
            .lines()
            .any(|line| line.contains("expect_used") && line.contains("[lib]")));
        for example in [
            "../../examples/parser-codec/Cargo.toml",
            "../../examples/no-std-embedded/Cargo.toml",
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(example);
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(
                text.contains("expect_used = \"deny\""),
                "{}",
                path.display()
            );
            assert!(text.contains("too_many_lines = \"deny\""));
        }
    }

    #[test]
    fn the_five_hard_lints_are_not_silenced_for_the_whole_crate() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = [
            "crates/cargo-tops/src/main.rs",
            "xtask/src/main.rs",
            "examples/no-std-embedded/src/lib.rs",
            "examples/parser-codec/src/lib.rs",
        ];
        let lints = [
            "clippy::indexing_slicing",
            "clippy::cast_possible_truncation",
            "clippy::let_underscore_must_use",
            "clippy::fn_params_excessive_bools",
            "clippy::string_add",
        ];
        for rel in files {
            let source = std::fs::read_to_string(root.join(rel)).unwrap();
            assert!(
                crate_level_blocks(&source)
                    .iter()
                    .all(|block| { lints.iter().all(|lint| !block.contains(lint)) }),
                "{rel}"
            );
        }
    }

    fn crate_level_blocks(source: &str) -> Vec<String> {
        let mut blocks = Vec::new();
        let mut current: Option<String> = None;
        for line in source.lines() {
            let start = line.trim();
            if current.is_none()
                && (start.starts_with("#![expect") || start.starts_with("#![cfg_attr"))
            {
                current = Some(String::new());
            }
            if let Some(block) = current.as_mut() {
                block.push_str(line);
                if line.contains(")]") {
                    blocks.push(block.clone());
                    current = None;
                }
            }
        }
        blocks
    }

    /// The lint a diagnostic belongs to. Rustc gives a few lints an error code
    /// (`E0133` for `unsafe_op_in_unsafe_fn`); a note then says
    /// "requested on the command line with `-W <lint>`", and that names it.
    fn lint_of(message: &serde_json::Value, code: &str) -> String {
        let named = message
            .get("children")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|child| child.get("message")?.as_str())
            .find_map(|text| text.split("`-W ").nth(1)?.split('`').next());
        match named {
            Some(name) if code.starts_with('E') => super::normalize_lint(name),
            _ => code.to_string(),
        }
    }

    fn parse_diags(output: &str) -> Vec<Diag> {
        let mut diags = Vec::new();
        for line in output.lines() {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if value.get("reason").and_then(|reason| reason.as_str()) != Some("compiler-message") {
                continue;
            }
            let Some(message) = value.get("message") else {
                continue;
            };
            let Some(code) = message.pointer("/code/code").and_then(|code| code.as_str()) else {
                continue;
            };
            let Some(spans) = message.get("spans").and_then(|spans| spans.as_array()) else {
                continue;
            };
            let Some(span) = spans
                .iter()
                .find(|span| span.get("is_primary") == Some(&serde_json::Value::Bool(true)))
            else {
                continue;
            };
            let Some(line_no) = span.get("line_start").and_then(|n| n.as_u64()) else {
                continue;
            };
            diags.push(Diag {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "already in this crate when the strict table was turned on"
                )]
                line: line_no as usize,
                lint: lint_of(message, code),
            });
        }
        diags
    }
}
