//! Regression probes from the BeautifulRust eval cases (slice 1.8, G5).
//!
//! The fixtures under `tests/fixtures/beautifulrust/` are the starting files of
//! four non-Bevy upstream cases. The upstream grader files are not vendored:
//! the table below lists each grader by its upstream name, `<case>/<file
//! without .md>`, and says what we expect of it. A grader a script can check on
//! the starting files names the rule ID and the line it expects; a grader that
//! needs judgment is listed as judgment, with no expectation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const VENDORED: &str = "Vendored from BeautifulRust v1.5.0 (40db61c6)";

/// A finding the grader's defect must produce: the rule and its 1-based line.
/// `anchor` is text on that line, so a moved line fails with a clear message.
struct Expect {
    rule: &'static str,
    line: usize,
    anchor: &'static str,
}

enum Grade {
    /// A script checks this on the starting file: these findings must appear.
    Checked(&'static [Expect]),
    /// No script checks this, but a judgment rule covers it: that ID, for the
    /// reviewer's answers file.
    Judged(&'static str),
    /// Graded on the agent's output, so there is nothing to expect here.
    Judgment,
}

struct Grader {
    case: &'static str,
    /// The upstream grader file name, without `.md`.
    name: &'static str,
    /// The starting file the expectations point into, under `<case>/files/`.
    source: &'static str,
    grade: Grade,
}

const fn expect(rule: &'static str, line: usize, anchor: &'static str) -> Expect {
    Expect { rule, line, anchor }
}

const INVENTORY: &str = "inventory.rs";
const REFACTOR: &str = "refactor-inventory-module";
const REVIEW: &str = "review-inventory-module";
const PORT_C: &str = "port-c-config-parser";
const PORT_PY: &str = "port-python-task-graph";

const DENY: &[Expect] = &[expect("IDIOM-1", 2, "#![deny(warnings)]")];
const PTR_ARG: &[Expect] = &[expect("API-1", 72, "line: &String")];
const UNWRAPS: &[Expect] = &[
    expect("PANIC-1", 55, "get(n).unwrap()"),
    expect("PANIC-1", 63, "get(name).unwrap()"),
    expect("PANIC-1", 77, "parts[1]"),
    expect("PANIC-1", 78, "parts[2]"),
    expect("PANIC-1", 84, "read_to_string(path).unwrap()"),
];
const BOOL_PARAM: &[Expect] = &[expect("API-3", 96, "verbose: bool")];
const HASH_ORDER: &[Expect] = &[expect("IDIOM-6", 98, "self.items.borrow().iter()")];
const CONCAT: &[Expect] = &[
    expect("IDIOM-5", 99, "s = s + &k"),
    expect("IDIOM-5", 101, "s = s + \"  price=\""),
];
const C_TESTS: &[Expect] = &[
    expect("PORT-4", 7, "test_basic"),
    expect("PORT-4", 21, "test_untrimmed_values_keep_whitespace"),
    expect("PORT-4", 27, "test_get_int_errors"),
    expect("PORT-4", 38, "test_duplicates"),
    expect("PORT-4", 47, "test_case_insensitive"),
    expect("PORT-4", 55, "test_syntax_errors"),
    expect("PORT-4", 63, "test_empty_input"),
];
// PORT-4 is checked at the method level only. Grader 03 also wants each of the
// six `subTest` cases in `test_parse_errors_report_line` checked, with its line
// number. A `subTest` case is a runtime parameter of one test method, not a test
// of its own, so a fixed-pattern inventory cannot see it. That half of the
// grader is judgment, with no machine expectation: each `subTest` case should
// map to a Rust case (PM ruling, 2026-11-18).
const PY_TESTS: &[Expect] = &[
    expect("PORT-4", 20, "test_parses_fields"),
    expect("PORT-4", 28, "test_negative_priority"),
    expect("PORT-4", 31, "test_parse_errors_report_line"),
    expect("PORT-4", 46, "test_errors_share_a_base_class"),
    expect("PORT-4", 53, "test_order_respects_deps_and_priority"),
    expect("PORT-4", 57, "test_missing_dependency"),
    expect("PORT-4", 62, "test_cycle_is_named"),
    expect("PORT-4", 68, "test_empty"),
    expect("PORT-4", 73, "test_counts_every_kind_in_order"),
];

const fn grader(
    case: &'static str,
    name: &'static str,
    source: &'static str,
    grade: Grade,
) -> Grader {
    Grader {
        case,
        name,
        source,
        grade,
    }
}

const GRADERS: &[Grader] = &[
    grader(
        REFACTOR,
        "01-deny-warnings-removed",
        INVENTORY,
        Grade::Checked(DENY),
    ),
    grader(
        REFACTOR,
        "02-no-rc-refcell-in-api",
        INVENTORY,
        Grade::Judgment,
    ),
    grader(
        REFACTOR,
        "03-borrowed-param-types",
        INVENTORY,
        Grade::Checked(PTR_ARG),
    ),
    grader(
        REFACTOR,
        "04-error-implements-error",
        INVENTORY,
        Grade::Judgment,
    ),
    grader(
        REFACTOR,
        "05-no-unwrap-on-fallible-paths",
        INVENTORY,
        Grade::Checked(UNWRAPS),
    ),
    grader(
        REFACTOR,
        "06-getter-renamed",
        INVENTORY,
        Grade::Judged("API-4"),
    ),
    grader(
        REFACTOR,
        "07-bool-param-replaced",
        INVENTORY,
        Grade::Checked(BOOL_PARAM),
    ),
    grader(
        REFACTOR,
        "08-deterministic-report",
        INVENTORY,
        Grade::Checked(HASH_ORDER),
    ),
    grader(
        REFACTOR,
        "09-format-over-concat",
        INVENTORY,
        Grade::Checked(CONCAT),
    ),
    grader(
        REFACTOR,
        "10-no-check-then-unwrap",
        INVENTORY,
        Grade::Judgment,
    ),
    grader(
        REFACTOR,
        "11-changes-listed-with-reasons",
        INVENTORY,
        Grade::Judgment,
    ),
    grader(
        REFACTOR,
        "12-signature-changes-called-out",
        INVENTORY,
        Grade::Judgment,
    ),
    grader(
        REVIEW,
        "01-flags-panics-as-top-priority",
        INVENTORY,
        Grade::Checked(UNWRAPS),
    ),
    grader(
        REVIEW,
        "02-flags-rc-refcell-api",
        INVENTORY,
        Grade::Judgment,
    ),
    grader(
        REVIEW,
        "03-flags-borrowed-param-types",
        INVENTORY,
        Grade::Checked(PTR_ARG),
    ),
    grader(REVIEW, "04-flags-error-type", INVENTORY, Grade::Judgment),
    grader(
        REVIEW,
        "05-flags-bool-parameter",
        INVENTORY,
        Grade::Checked(BOOL_PARAM),
    ),
    grader(
        REVIEW,
        "06-flags-nondeterministic-output",
        INVENTORY,
        Grade::Checked(HASH_ORDER),
    ),
    grader(
        REVIEW,
        "07-flags-deny-warnings",
        INVENTORY,
        Grade::Checked(DENY),
    ),
    grader(
        REVIEW,
        "08-findings-cite-locations",
        INVENTORY,
        Grade::Judgment,
    ),
    grader(
        REVIEW,
        "09-findings-cite-named-rules",
        INVENTORY,
        Grade::Judgment,
    ),
    grader(REVIEW, "10-fixes-shown-as-code", INVENTORY, Grade::Judgment),
    grader(REVIEW, "11-says-what-is-good", INVENTORY, Grade::Judgment),
    grader(REVIEW, "12-code-left-unchanged", INVENTORY, Grade::Judgment),
    grader(
        PORT_C,
        "01-builds-and-tests-pass",
        "test_config.c",
        Grade::Judgment,
    ),
    grader(PORT_C, "02-clippy-clean", "test_config.c", Grade::Judgment),
    grader(PORT_C, "03-no-unsafe", "test_config.c", Grade::Judgment),
    grader(
        PORT_C,
        "04-every-test-ported",
        "test_config.c",
        Grade::Checked(C_TESTS),
    ),
    grader(
        PORT_C,
        "05-errors-as-result",
        "test_config.c",
        Grade::Judgment,
    ),
    grader(
        PORT_C,
        "06-error-implements-error",
        "test_config.c",
        Grade::Judgment,
    ),
    grader(
        PORT_C,
        "07-option-for-absent",
        "test_config.c",
        Grade::Judgment,
    ),
    grader(PORT_C, "08-typed-flags", "test_config.c", Grade::Judgment),
    grader(
        PORT_C,
        "09-behavior-preserved",
        "test_config.c",
        Grade::Judgment,
    ),
    grader(PORT_C, "10-porting-notes", "test_config.c", Grade::Judgment),
    grader(PORT_C, "11-proportionate", "test_config.c", Grade::Judgment),
    grader(
        PORT_PY,
        "01-builds-and-tests-pass",
        "test_taskgraph.py",
        Grade::Judgment,
    ),
    grader(
        PORT_PY,
        "02-clippy-clean",
        "test_taskgraph.py",
        Grade::Judgment,
    ),
    grader(
        PORT_PY,
        "03-every-test-ported",
        "test_taskgraph.py",
        Grade::Checked(PY_TESTS),
    ),
    grader(
        PORT_PY,
        "04-exceptions-as-error-enum",
        "test_taskgraph.py",
        Grade::Judgment,
    ),
    grader(
        PORT_PY,
        "05-kind-is-enum",
        "test_taskgraph.py",
        Grade::Judgment,
    ),
    grader(
        PORT_PY,
        "06-priority-is-option",
        "test_taskgraph.py",
        Grade::Judgment,
    ),
    grader(
        PORT_PY,
        "07-definition-order-preserved",
        "test_taskgraph.py",
        Grade::Judgment,
    ),
    grader(
        PORT_PY,
        "08-order-and-cycle-match",
        "test_taskgraph.py",
        Grade::Judgment,
    ),
    grader(
        PORT_PY,
        "09-summary-every-kind-in-order",
        "test_taskgraph.py",
        Grade::Judgment,
    ),
    grader(
        PORT_PY,
        "10-porting-notes",
        "test_taskgraph.py",
        Grade::Judgment,
    ),
    grader(
        PORT_PY,
        "11-proportionate",
        "test_taskgraph.py",
        Grade::Judgment,
    ),
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beautifulrust")
}

/// Every file under `dir`, sorted, as paths relative to `dir`.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).expect("fixture directory reads") {
            let path = entry.expect("directory entry reads").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(
                    path.strip_prefix(dir)
                        .expect("path is under dir")
                        .to_path_buf(),
                );
            }
        }
    }
    found.sort();
    found
}

/// Rule ID to kind, from the catalog the skills ship.
fn catalog() -> BTreeMap<String, String> {
    let text = include_str!("../templates/skills/rust-tops-style/rules.toml");
    let table: toml::Table = toml::from_str(text).expect("rules.toml parses");
    let rules = table
        .get("rule")
        .and_then(toml::Value::as_array)
        .expect("rules.toml has [[rule]] tables");
    rules
        .iter()
        .filter_map(|rule| {
            let id = rule.get("id")?.as_str()?;
            let kind = rule.get("kind")?.as_str()?;
            Some((id.to_string(), kind.to_string()))
        })
        .collect()
}

/// Problems with a grader table: a name listed twice, a row for a case that is
/// not one of `cases`, or a case with no rows.
fn table_problems(rows: &[(&str, &str)], cases: &[&str]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for (case, name) in rows {
        if !seen.insert(format!("{case}/{name}")) {
            problems.push(format!("listed twice: {case}/{name}"));
        }
        if !cases.contains(case) {
            problems.push(format!("not a known case: {case}/{name}"));
        }
    }
    for case in cases {
        if !rows.iter().any(|(row_case, _)| row_case == case) {
            problems.push(format!("no rows for case {case}"));
        }
    }
    problems
}

/// Why one expectation is wrong: an unknown rule, or a line that is not the anchor.
fn expectation_problems(
    expect: &Expect,
    source: &str,
    catalog: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut problems = Vec::new();
    if !catalog.contains_key(expect.rule) {
        problems.push(format!("{} is not a rule in rules.toml", expect.rule));
    }
    let text = source.lines().nth(expect.line - 1).unwrap_or("");
    if !text.contains(expect.anchor) {
        problems.push(format!(
            "{} line {} does not hold `{}`: `{text}`",
            expect.rule, expect.line, expect.anchor
        ));
    }
    problems
}

const CASES: [&str; 4] = [REFACTOR, REVIEW, PORT_C, PORT_PY];

#[test]
fn the_grader_table_has_46_unique_rows_and_every_case_has_some() {
    let rows: Vec<(&str, &str)> = GRADERS.iter().map(|item| (item.case, item.name)).collect();
    assert_eq!(table_problems(&rows, &CASES), Vec::<String>::new());
    assert_eq!(rows.len(), 46, "four cases: 12 + 12 + 11 + 11 graders");
    for case in CASES {
        assert!(
            fixtures().join(case).join("files").is_dir(),
            "{case} has no files/"
        );
    }
}

#[test]
fn a_repeated_name_an_unknown_case_or_an_empty_case_is_a_problem() {
    let rows = [("a", "01-x"), ("a", "01-x"), ("z", "02-y")];
    let problems = table_problems(&rows, &["a", "b"]);
    assert_eq!(
        problems,
        [
            "listed twice: a/01-x",
            "not a known case: z/02-y",
            "no rows for case b"
        ]
    );
}

#[test]
fn every_checked_grader_names_a_catalog_rule_on_an_anchored_line() {
    let catalog = catalog();
    let mut checked = 0;
    for item in GRADERS {
        let Grade::Checked(expectations) = &item.grade else {
            continue;
        };
        assert!(!expectations.is_empty(), "{}/{}", item.case, item.name);
        let path = fixtures().join(item.case).join("files").join(item.source);
        let source = std::fs::read_to_string(&path).unwrap();
        for expect in *expectations {
            let problems = expectation_problems(expect, &source, &catalog);
            assert!(
                problems.is_empty(),
                "{}/{}: {problems:?}",
                item.case,
                item.name
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 13, "the machine-checkable graders");
}

#[test]
fn a_judged_grader_names_a_judgment_rule() {
    let catalog = catalog();
    let judged: Vec<&str> = GRADERS
        .iter()
        .filter_map(|item| match item.grade {
            Grade::Judged(rule) => Some(rule),
            _ => None,
        })
        .collect();
    assert_eq!(judged, ["API-4"]);
    for rule in judged {
        assert_eq!(
            catalog.get(rule).map(String::as_str),
            Some("judgment"),
            "{rule}"
        );
    }
}

#[test]
fn a_moved_line_or_an_unknown_rule_is_a_problem() {
    let catalog = catalog();
    let source = "line one\nline two\n";
    let moved = expect("API-1", 1, "line two");
    assert_eq!(expectation_problems(&moved, source, &catalog).len(), 1);
    let unknown = expect("NOPE-1", 2, "line two");
    assert_eq!(expectation_problems(&unknown, source, &catalog).len(), 1);
    let past_the_end = expect("API-1", 9, "x");
    assert_eq!(
        expectation_problems(&past_the_end, source, &catalog).len(),
        1
    );
}

#[test]
fn every_fixture_carries_the_mit_line_and_stays_out_of_the_workspace() {
    let root = fixtures();
    let notice = std::fs::read_to_string(root.join("NOTICE")).unwrap();
    assert!(notice.contains("40db61c6fee84bd121cf288755f0e2f22a50e995"));
    for path in files_under(&root) {
        let name = path.to_string_lossy().into_owned();
        assert_ne!(
            path.file_name().and_then(|part| part.to_str()),
            Some("Cargo.toml"),
            "{name} would make a fixture a workspace member"
        );
        if name == "NOTICE" || name.starts_with("golden/") {
            continue;
        }
        let head: String = std::fs::read_to_string(root.join(&path))
            .unwrap()
            .lines()
            .take(7)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            head.contains(VENDORED),
            "{name}: no MIT line in the first lines"
        );
    }
}

// ---------------------------------------------------------------------------
// `rust-tops-style.nu check` on the starting files.

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("templates/skills/rust-tops-style/rust-tops-style.nu")
}

/// The nu binary, or `None` to skip. `RUST_TOPS_REQUIRE_NU` makes a missing one fail.
fn nu_or_skip() -> Option<PathBuf> {
    let found = Command::new("sh")
        .args(["-c", "command -v nu"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| PathBuf::from(text.trim()))
        .filter(|path| path.is_file());
    assert!(
        found.is_some() || std::env::var_os("RUST_TOPS_REQUIRE_NU").is_none(),
        "nu is not installed; RUST_TOPS_REQUIRE_NU is set"
    );
    if found.is_none() {
        eprintln!("nu is not installed; the style evals are skipped");
    }
    found
}

/// A new, empty directory of its own: process id plus a per-call counter, so
/// tests that share a process (cargo test) never share a directory.
fn temp_root() -> PathBuf {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("cargo-tops-evals-{}-{call}", std::process::id()));
    std::fs::create_dir_all(&root).expect("temp root");
    root
}

/// A library crate holding `source` as `src/lib.rs`, in a directory of its own.
fn scaffold(source: &str) -> PathBuf {
    let root = temp_root();
    std::fs::create_dir_all(root.join("crate/src")).expect("scaffold dirs");
    let manifest = "[package]\nname = \"eval-fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n";
    std::fs::write(root.join("crate/Cargo.toml"), manifest).expect("Cargo.toml");
    std::fs::write(root.join("crate/src/lib.rs"), source).expect("lib.rs");
    root
}

/// Runs `check` on `source` and returns its stdout. The fixture denies
/// `warnings` on purpose (the IDIOM-1 plant); the script caps lints at warn
/// itself, so the run is the one an adopter gets.
fn style_check(nu: &Path, source: &str) -> String {
    let root = scaffold(source);
    let out = Command::new(nu)
        .arg(script())
        .args(["check", "--path"])
        .arg(root.join("crate"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .expect("spawn nu");
    std::fs::remove_dir_all(&root).expect("remove scaffold");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8")
}

fn findings_of(stdout: &str) -> Vec<serde_json::Value> {
    let doc: serde_json::Value = serde_json::from_str(stdout.trim()).expect("one JSON object");
    assert!(doc.get("error").is_none(), "the check did not run: {doc}");
    doc.pointer("/data/findings")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .expect("data.findings")
}

/// Expectations that are not found among the findings, by rule and line.
fn not_found(findings: &[serde_json::Value], wanted: &[&Expect]) -> Vec<String> {
    wanted
        .iter()
        .filter(|want| {
            !findings.iter().any(|found| {
                found.get("rule").and_then(serde_json::Value::as_str) == Some(want.rule)
                    && found.get("line").and_then(serde_json::Value::as_u64)
                        == Some(want.line as u64)
            })
        })
        .map(|want| format!("{} at line {} ({})", want.rule, want.line, want.anchor))
        .collect()
}

/// Every style-checked expectation of the graders, once each. The `PORT-`
/// rules belong to `port inventory`, not to the style check.
fn style_expectations() -> Vec<&'static Expect> {
    let mut wanted: Vec<&Expect> = GRADERS
        .iter()
        .filter_map(|item| match &item.grade {
            Grade::Checked(list) => Some(list.iter()),
            _ => None,
        })
        .flatten()
        .filter(|item| !item.rule.starts_with("PORT-"))
        .collect();
    wanted.sort_by_key(|item| (item.rule, item.line));
    wanted.dedup_by_key(|item| (item.rule, item.line));
    wanted
}

fn inventory(case: &str) -> String {
    let path = fixtures().join(case).join("files").join(INVENTORY);
    std::fs::read_to_string(path).expect("inventory.rs")
}

#[test]
fn the_two_inventory_cases_share_one_starting_file() {
    assert_eq!(inventory(REFACTOR), inventory(REVIEW));
}

#[test]
fn style_findings_match_the_golden() {
    let Some(nu) = nu_or_skip() else { return };
    let stdout = style_check(&nu, &inventory(REFACTOR));
    let golden = fixtures().join("golden/style-inventory.json");
    if std::env::var_os("RUST_TOPS_BLESS").is_some() {
        std::fs::create_dir_all(golden.parent().expect("golden dir")).expect("golden dir");
        std::fs::write(&golden, &stdout).expect("bless golden");
    }
    assert_eq!(stdout, std::fs::read_to_string(&golden).expect("golden"));
}

#[test]
fn every_style_checked_grader_has_its_finding() {
    let Some(nu) = nu_or_skip() else { return };
    let found = findings_of(&style_check(&nu, &inventory(REFACTOR)));
    let missing = not_found(&found, &style_expectations());
    assert!(missing.is_empty(), "no finding for: {missing:#?}");
    assert_eq!(
        style_expectations().len(),
        11,
        "expected rule and line pairs"
    );
}

/// Replaces `from` with `to` on one line, so no other line moves.
fn plant(source: &str, line: usize, from: &str, to: &str) -> String {
    let planted: Vec<String> = source
        .lines()
        .enumerate()
        .map(|(index, text)| {
            if index + 1 == line {
                assert!(text.contains(from), "line {line} has no `{from}`: {text}");
                text.replacen(from, to, 1)
            } else {
                text.to_string()
            }
        })
        .collect();
    format!("{}\n", planted.join("\n"))
}

#[test]
fn removing_a_planted_defect_makes_its_grader_fail() {
    let Some(nu) = nu_or_skip() else { return };
    let original = inventory(REFACTOR);
    let wanted = style_expectations();
    let plants = [
        // IDIOM-1, a probe: the crate-level deny of warnings.
        (2, "#![deny(warnings)]", "// no deny", "IDIOM-1 at line 2"),
        // PANIC-1, a lint: the unwrap on the file read.
        (84, ".unwrap()", ".expect(\"read\")", "PANIC-1 at line 84"),
    ];
    for (line, from, to, lost) in plants {
        let planted = plant(&original, line, from, to);
        let found = findings_of(&style_check(&nu, &planted));
        let missing = not_found(&found, &wanted);
        assert_eq!(missing.len(), 1, "{lost}: {missing:?}");
        assert!(
            missing.iter().all(|item| item.starts_with(lost)),
            "{missing:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// `rust-tops-port.nu inventory` on the upstream port cases.

fn port_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/skills/rust-tops-port/rust-tops-port.nu")
}

/// One port case: where its original tests sit and the language flag.
struct PortCase {
    case: &'static str,
    lang: &'static str,
    /// Directory the files are copied into.
    dir: &'static str,
    golden: &'static str,
}

const PORT_C_CASE: PortCase = PortCase {
    case: PORT_C,
    lang: "c",
    dir: "c",
    golden: "golden/port-inventory-c.json",
};
const PORT_PY_CASE: PortCase = PortCase {
    case: PORT_PY,
    lang: "python",
    dir: "python",
    golden: "golden/port-inventory-python.json",
};
const PORT_CASES: [&PortCase; 2] = [&PORT_C_CASE, &PORT_PY_CASE];

/// Runs `inventory` on a copy of the case's starting files, with `plant`
/// (file, from, to) applied first. The working directory is the scratch root
/// and `--from` is relative, so the output holds no absolute path.
fn port_inventory(nu: &Path, case: &PortCase, plant: Option<(&str, &str, &str)>) -> String {
    let root = temp_root();
    let dir = root.join(case.dir);
    std::fs::create_dir_all(&dir).expect("case dir");
    let files = fixtures().join(case.case).join("files");
    for name in files_under(&files) {
        std::fs::copy(files.join(&name), dir.join(&name)).expect("copy fixture");
    }
    if let Some((file, from, to)) = plant {
        let text = std::fs::read_to_string(dir.join(file)).expect("planted file");
        assert!(text.contains(from), "{file} has no `{from}`");
        std::fs::write(dir.join(file), text.replacen(from, to, 1)).expect("plant");
    }
    let out = Command::new(nu)
        .arg(port_script())
        .args(["inventory", "--from", case.dir, "--lang", case.lang])
        .current_dir(&root)
        .output()
        .expect("spawn nu");
    std::fs::remove_dir_all(&root).expect("remove scratch");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8")
}

fn inventoried(stdout: &str) -> Vec<serde_json::Value> {
    let doc: serde_json::Value = serde_json::from_str(stdout.trim()).expect("one JSON object");
    assert_eq!(doc.get("ok"), Some(&serde_json::Value::Bool(true)), "{doc}");
    doc.pointer("/data/tests")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .expect("data.tests")
}

/// The `PORT-4` expectations of a case, from its checked grader.
fn port_expectations(case: &str) -> Vec<&'static Expect> {
    GRADERS
        .iter()
        .filter(|item| item.case == case)
        .filter_map(|item| match &item.grade {
            Grade::Checked(list) => Some(list.iter()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Expectations with no inventoried test of that name on that line, plus any
/// inventoried test nobody expected, so a lost or a new test both show.
fn inventory_gaps(tests: &[serde_json::Value], wanted: &[&Expect]) -> Vec<String> {
    let seen = |test: &serde_json::Value| {
        (
            test.get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string(),
            test.get("line")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        )
    };
    let found: BTreeSet<(String, u64)> = tests.iter().map(seen).collect();
    let expected: BTreeSet<(String, u64)> = wanted
        .iter()
        .map(|want| (want.anchor.to_string(), want.line as u64))
        .collect();
    let lost = expected
        .difference(&found)
        .map(|(name, line)| format!("{} at line {line} is not inventoried", name));
    let extra = found
        .difference(&expected)
        .map(|(name, line)| format!("{name} at line {line} is not expected"));
    lost.chain(extra).collect()
}

#[test]
fn port_inventory_matches_the_golden() {
    let Some(nu) = nu_or_skip() else { return };
    for case in PORT_CASES {
        let stdout = port_inventory(&nu, case, None);
        let golden = fixtures().join(case.golden);
        if std::env::var_os("RUST_TOPS_BLESS").is_some() {
            std::fs::create_dir_all(golden.parent().expect("golden dir")).expect("golden dir");
            std::fs::write(&golden, &stdout).expect("bless golden");
        }
        assert_eq!(
            stdout,
            std::fs::read_to_string(&golden).expect("golden"),
            "{}",
            case.case
        );
    }
}

#[test]
fn every_port_grader_test_is_inventoried_on_its_line() {
    let Some(nu) = nu_or_skip() else { return };
    for case in PORT_CASES {
        let tests = inventoried(&port_inventory(&nu, case, None));
        let wanted = port_expectations(case.case);
        assert!(wanted.iter().all(|item| item.rule == "PORT-4"));
        let gaps = inventory_gaps(&tests, &wanted);
        assert!(gaps.is_empty(), "{}: {gaps:#?}", case.case);
    }
    assert_eq!(port_expectations(PORT_C).len(), 7);
    assert_eq!(port_expectations(PORT_PY).len(), 9);
}

#[test]
fn renaming_an_original_test_makes_its_grader_fail() {
    let Some(nu) = nu_or_skip() else { return };
    let plants = [
        (
            &PORT_C_CASE,
            "test_config.c",
            "test_duplicates(void)",
            "check_duplicates(void)",
            "test_duplicates",
        ),
        (
            &PORT_PY_CASE,
            "test_taskgraph.py",
            "def test_empty(",
            "def check_empty(",
            "test_empty",
        ),
    ];
    for (case, file, from, to, lost) in plants {
        let tests = inventoried(&port_inventory(&nu, case, Some((file, from, to))));
        let gaps = inventory_gaps(&tests, &port_expectations(case.case));
        assert_eq!(gaps.len(), 1, "{}: {gaps:?}", case.case);
        assert!(gaps.iter().all(|item| item.starts_with(lost)), "{gaps:?}");
    }
}

// ---------------------------------------------------------------------------
// `rust-tops-review.nu style` on the starting files.

fn review_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("templates/skills/rust-tops-review/rust-tops-review.nu")
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A repository whose `base` branch holds an empty library and whose head
/// commit adds `source` as `src/lib.rs`, so every line of it is in range.
fn review_repo(source: &str) -> PathBuf {
    let root = temp_root();
    let repo = root.join("repo");
    std::fs::create_dir_all(repo.join("src")).expect("repo dirs");
    let manifest = "[package]\nname = \"eval-fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n";
    std::fs::write(repo.join("Cargo.toml"), manifest).expect("Cargo.toml");
    std::fs::write(repo.join(".gitignore"), "target\n").expect("gitignore");
    std::fs::write(repo.join("src/lib.rs"), "//! base\n").expect("lib.rs");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    git(&repo, &["branch", "base"]);
    std::fs::write(repo.join("src/lib.rs"), source).expect("lib.rs");
    git(&repo, &["commit", "-aqm", "add the module"]);
    root
}

/// Runs `review style --base base --committed-only` on the committed repo,
/// from inside it with `--path .`, so the output holds no absolute path.
fn review_style(nu: &Path, source: &str) -> String {
    let root = review_repo(source);
    let out = Command::new(nu)
        .arg(review_script())
        .args(["style", "--base", "base", "--path", ".", "--committed-only"])
        .current_dir(root.join("repo"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .expect("spawn nu");
    std::fs::remove_dir_all(&root).expect("remove repo");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8")
}

/// The style-checked expectations of one case's graders.
fn case_expectations(case: &str) -> Vec<&'static Expect> {
    GRADERS
        .iter()
        .filter(|item| item.case == case)
        .filter_map(|item| match &item.grade {
            Grade::Checked(list) => Some(list.iter()),
            _ => None,
        })
        .flatten()
        .collect()
}

#[test]
fn review_style_findings_match_the_golden() {
    let Some(nu) = nu_or_skip() else { return };
    let stdout = review_style(&nu, &inventory(REVIEW));
    let golden = fixtures().join("golden/review-style-inventory.json");
    if std::env::var_os("RUST_TOPS_BLESS").is_some() {
        std::fs::write(&golden, &stdout).expect("bless golden");
    }
    assert_eq!(stdout, std::fs::read_to_string(&golden).expect("golden"));
}

#[test]
fn every_review_grader_has_its_finding_in_the_range() {
    let Some(nu) = nu_or_skip() else { return };
    let found = findings_of(&review_style(&nu, &inventory(REVIEW)));
    let wanted = case_expectations(REVIEW);
    assert_eq!(
        wanted.len(),
        9,
        "rule and line pairs the review graders expect"
    );
    let missing = not_found(&found, &wanted);
    assert!(missing.is_empty(), "no finding for: {missing:#?}");
}

#[test]
fn a_whole_file_in_range_gives_the_style_check_findings() {
    let Some(nu) = nu_or_skip() else { return };
    let source = inventory(REVIEW);
    let review = findings_of(&review_style(&nu, &source));
    let style = findings_of(&style_check(&nu, &source));
    assert_eq!(review, style);
}

#[test]
fn removing_a_planted_defect_makes_its_review_grader_fail() {
    let Some(nu) = nu_or_skip() else { return };
    let original = inventory(REVIEW);
    let wanted = case_expectations(REVIEW);
    let plants = [
        (2, "#![deny(warnings)]", "// no deny", "IDIOM-1 at line 2"),
        (84, ".unwrap()", ".expect(\"read\")", "PANIC-1 at line 84"),
    ];
    for (line, from, to, lost) in plants {
        let found = findings_of(&review_style(&nu, &plant(&original, line, from, to)));
        let missing = not_found(&found, &wanted);
        assert_eq!(missing.len(), 1, "{lost}: {missing:?}");
        assert!(
            missing.iter().all(|item| item.starts_with(lost)),
            "{missing:?}"
        );
    }
}
