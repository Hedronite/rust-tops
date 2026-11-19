//! `cargo tops gate` enforces the coverage floors and the CRAP threshold and
//! ratchet of `rust-tops.yaml` (RUST_TOPS.md §2, §7.3, §16, §19), and
//! `cargo tops crap-baseline` records the baseline the ratchet reads.
//!
//! Cargo and its plugins are stubs. The cargo stub writes the coverage
//! summary and the CRAP report the test put in the fixture's `reports/`, so
//! each test states the numbers the gate judges. Only the coverage and crap
//! layers are on.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

/// Writes `$REPORTS/coverage.json` or `$REPORTS/crap.json` to the file after
/// `--output-path` / `--output`, and a stand-in LCOV otherwise. Exits 1 for
/// an argv that matches `$CARGO_STUB_FAIL`. Answers `metadata` from
/// `$REPORTS/metadata.json`.
const CARGO_STUB: &str = r#"#!/bin/sh
if [ -n "${CARGO_STUB_FAIL:-}" ]; then
  case "$*" in *"$CARGO_STUB_FAIL"*) exit 1 ;; esac
fi
if [ -n "${CARGO_STUB_LOG:-}" ]; then echo "$*" >> "$CARGO_STUB_LOG"; fi
if [ "$1 $2" = "crap --version" ]; then
  # The lock `init` writes pins cargo-crap 0.9.0; the gate compares this line.
  echo "cargo-crap 0.9.0"
  exit 0
fi
if [ "$1" = metadata ]; then
  cat "$REPORTS/metadata.json"
  exit 0
fi
prev=
for arg in "$@"; do
  case $prev in
    --output-path | --output)
      mkdir -p "$(dirname "$arg")"
      name=$(basename "$arg")
      if [ -f "$REPORTS/$name" ]; then cp "$REPORTS/$name" "$arg"; else printf 'lcov\n' > "$arg"; fi
      ;;
  esac
  prev=$arg
done
exit 0
"#;

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    bin: PathBuf,
    reports: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}

fn stub(bin: &Path, name: &str, body: &str) {
    let path = bin.join(name);
    fs::write(&path, body).expect("write stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod stub");
    }
}

/// The cargo stub and a stub for every plugin a layer probes for. `cargo-deny`
/// is among them, so no test here depends on the host having it.
fn stub_tools(bin: &Path) {
    stub(bin, "cargo", CARGO_STUB);
    for plugin in [
        "cargo-llvm-cov",
        "cargo-crap",
        "cargo-nextest",
        "cargo-deny",
    ] {
        stub(bin, plugin, "#!/bin/sh\nexit 0\n");
    }
}

/// A library-core instance named `fixture` with only coverage and crap on.
fn fixture(name: &str) -> Fixture {
    fixture_with(name, &["coverage", "crap"])
}

/// A library-core instance named `fixture` with only the `on` layers enabled.
fn fixture_with(name: &str, on: &[&str]) -> Fixture {
    let root =
        std::env::temp_dir().join(format!("cargo-tops-floors-{name}-{}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    let fixture = Fixture {
        repo: root.join("repo"),
        bin: root.join("bin"),
        reports: root.join("reports"),
        root,
    };
    for dir in [&fixture.bin, &fixture.reports] {
        fs::create_dir_all(dir).expect("mkdir");
    }
    let init = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args([
            "init",
            "--class",
            "library-core",
            "--name",
            "fixture",
            "--path",
        ])
        .arg(&fixture.repo)
        .output()
        .expect("spawn init");
    assert!(init.status.success(), "init failed");
    // `cargo metadata` is asked about a project with a manifest.
    fs::write(
        fixture.repo.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");
    stub_tools(&fixture.bin);
    edit(&fixture, |doc| {
        let layers = doc.pointer_mut("/layers").and_then(Value::as_array_mut);
        for layer in layers.expect("layers") {
            let id = layer.get("id").and_then(Value::as_str);
            let enabled = id.is_some_and(|id| on.contains(&id));
            set(layer, "", "enabled", Some(Value::Bool(enabled)));
        }
    });
    fixture
}

/// Sets (or, for `None`, removes) `key` in the object at `pointer`.
fn set(doc: &mut Value, pointer: &str, key: &str, value: Option<Value>) {
    let map = doc
        .pointer_mut(pointer)
        .and_then(Value::as_object_mut)
        .expect("an object at the pointer");
    match value {
        Some(value) => map.insert(key.to_string(), value),
        None => map.remove(key),
    };
}

fn edit(fixture: &Fixture, change: impl FnOnce(&mut Value)) {
    let path = fixture.repo.join("rust-tops.yaml");
    let mut doc: Value =
        serde_saphyr::from_str(&fs::read_to_string(&path).expect("read")).expect("yaml");
    change(&mut doc);
    fs::write(&path, serde_json::to_string_pretty(&doc).expect("json")).expect("write yaml");
}

/// The coverage summary: each file is `(path, lines covered, lines, regions
/// covered, regions)`.
fn summary(fixture: &Fixture, files: &[(&str, u64, u64, u64, u64)]) {
    let files: Vec<Value> = files
        .iter()
        .map(|&(name, lc, ln, rc, rn)| {
            json!({"filename": name, "summary": {
                "lines": {"covered": lc, "count": ln},
                "regions": {"covered": rc, "count": rn}}})
        })
        .collect();
    let doc = json!({"type": "llvm.coverage.json.export", "data": [{"files": files}]});
    fs::write(fixture.reports.join("coverage.json"), doc.to_string()).expect("write summary");
}

/// The CRAP report: each entry is `(file, function, crap)`.
fn crap(fixture: &Fixture, entries: &[(&str, &str, f64)]) {
    let entries: Vec<Value> = entries
        .iter()
        .map(|&(file, function, score)| {
            json!({"file": file, "function": function, "line": 1, "cyclomatic": 1.0, "coverage": 100.0, "crap": score})
        })
        .collect();
    let doc = json!({"version": "0.9.0", "entries": entries});
    fs::write(fixture.reports.join("crap.json"), doc.to_string()).expect("write crap");
}

fn tops(fixture: &Fixture, args: &[&str]) -> (Option<i32>, String) {
    let path = stub_path(fixture);
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(args)
        .arg("--path")
        .arg(&fixture.repo)
        .env("PATH", path)
        .env("REPORTS", &fixture.reports)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("spawn cargo-tops");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code(), text)
}

/// The stubs first, then the inherited `PATH` for `sh`, `cp` and the rest.
fn stub_path(fixture: &Fixture) -> std::ffi::OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let dirs = std::iter::once(fixture.bin.clone()).chain(std::env::split_paths(&inherited));
    std::env::join_paths(dirs).expect("PATH")
}

fn gate(fixture: &Fixture) -> (Option<i32>, String) {
    tops(fixture, &["gate"])
}

fn good_summary(fixture: &Fixture) {
    summary(fixture, &[("src/lib.rs", 95, 100, 90, 100)]);
}

#[test]
fn reports_that_meet_every_floor_pass() {
    let fixture = fixture("pass");
    good_summary(&fixture);
    crap(&fixture, &[("./src/lib.rs", "parse", 6.0)]);
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(0), "{text}");
    assert!(
        text.contains("==> [coverage] floors for ")
            && text.contains(": line >= 95%, region >= 90%, per-file line >= 90%"),
        "{text}"
    );
    assert!(
        text.contains("==> [crap] threshold 6 on new functions in "),
        "{text}"
    );
    assert!(
        text.contains("no baseline: every function is judged as new"),
        "{text}"
    );
    assert_eq!(text.matches("\n    ok\n").count(), 2, "{text}");
}

#[test]
fn a_crate_total_and_every_file_below_their_floors_fail() {
    let fixture = fixture("coverage-miss");
    summary(
        &fixture,
        &[
            ("src/lib.rs", 100, 100, 100, 100),
            ("src/a.rs", 80, 100, 80, 100),
            ("src/b.rs", 85, 100, 90, 100),
            ("src/tests/t.rs", 0, 100, 0, 100),
        ],
    );
    crap(&fixture, &[]);
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(1), "{text}");
    for line in [
        "  - coverage: line coverage 88.33% (265 of 300 lines) is below gates.coverage.line_min_pct 95",
        "  - coverage: src/a.rs line coverage 80.00% (80 of 100 lines) is below gates.coverage.per_file_line_min_pct 90",
        "  - coverage: src/b.rs line coverage 85.00% (85 of 100 lines) is below gates.coverage.per_file_line_min_pct 90",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    // Regions sit at exactly 90%, the floor; the test file is not production.
    assert!(!text.contains("region coverage"), "{text}");
    assert!(!text.contains("src/tests/t.rs"), "{text}");
    assert!(
        text.contains("    3 problem(s), listed at the end"),
        "{text}"
    );
}

#[test]
fn a_new_function_over_the_threshold_fails_until_it_is_in_the_baseline() {
    let fixture = fixture("ratchet");
    good_summary(&fixture);
    let legacy = [
        ("./src/lib.rs", "legacy", 12.0),
        ("./src/tests/t.rs", "helper", 42.0),
    ];
    crap(&fixture, &legacy);
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains(
            "  - crap: src/lib.rs legacy CRAP 12.00 is above the threshold 6 (new function)"
        ),
        "{text}"
    );
    let (code, text) = tops(&fixture, &["crap-baseline"]);
    assert_eq!(code, Some(0), "{text}");
    let baseline = fixture.repo.join("target/crap-baseline.json");
    assert!(
        text.contains(&format!("wrote {} (2 functions from ", baseline.display())),
        "{text}"
    );
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(0), "{text}");
    assert!(!text.contains("no baseline"), "{text}");
}

#[test]
fn against_a_baseline_legacy_may_not_rise_and_new_code_meets_the_threshold() {
    let fixture = fixture("rise");
    good_summary(&fixture);
    crap(&fixture, &[("./src/lib.rs", "legacy", 12.0)]);
    assert_eq!(gate(&fixture).0, Some(1));
    assert_eq!(tops(&fixture, &["crap-baseline"]).0, Some(0));
    crap(
        &fixture,
        &[
            ("./src/lib.rs", "legacy", 12.5),
            ("./src/lib.rs", "fresh", 7.0),
        ],
    );
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains("  - crap: src/lib.rs legacy CRAP 12.50 rose above its baseline 12.00"),
        "{text}"
    );
    assert!(
        text.contains(
            "  - crap: src/lib.rs fresh CRAP 7.00 is above the threshold 6 (new function)"
        ),
        "{text}"
    );
}

#[test]
fn an_instance_baseline_path_is_where_the_baseline_is_written_and_read() {
    let fixture = fixture("tracked");
    good_summary(&fixture);
    crap(&fixture, &[("./src/lib.rs", "legacy", 9.0)]);
    edit(&fixture, |doc| {
        set(
            doc,
            "/gates/crap",
            "baseline",
            Some(json!("ci/crap-baseline.json")),
        );
    });
    assert_eq!(gate(&fixture).0, Some(1));
    let (code, text) = tops(&fixture, &["crap-baseline"]);
    assert_eq!(code, Some(0), "{text}");
    let tracked = fixture.repo.join("ci/crap-baseline.json");
    let doc: Value =
        serde_json::from_str(&fs::read_to_string(&tracked).expect("baseline")).expect("json");
    assert_eq!(
        doc.pointer("/entries/0/function").and_then(Value::as_str),
        Some("legacy")
    );
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(0), "{text}");
    assert!(
        text.contains(&format!("ratchet against {}", tracked.display())),
        "{text}"
    );
    fs::write(&tracked, "not json").expect("write");
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains(&format!(
            "  - crap: baseline {}: the report is not JSON",
            tracked.display()
        )),
        "{text}"
    );
}

#[test]
fn crap_baseline_needs_a_report_from_a_gate() {
    let fixture = fixture("no-report");
    let (code, text) = tops(&fixture, &["crap-baseline"]);
    assert_eq!(code, Some(2), "{text}");
    assert!(
        text.contains("run `cargo tops gate` with the crap layer first"),
        "{text}"
    );
}

/// A live crap exemption on `main`, a live coverage exemption on `src/gen`,
/// and an expired crap exemption on `old`.
fn exemptions() -> Value {
    json!([
        {"path": "src/main.rs", "symbol": "main", "layer": "crap", "owner": "cli",
         "reason": "one arm per subcommand, each a single call", "expires": "2999-01-01"},
        {"path": "src/gen/**", "layer": "coverage", "owner": "codegen",
         "reason": "generated; the decode roundtrip covers it", "expires": "2999-01-01"},
        {"path": "src/lib.rs", "symbol": "old", "layer": "crap", "owner": "core",
         "reason": "was over the threshold before the rewrite", "expires": "2020-01-01"},
    ])
}

#[test]
fn a_live_exemption_exempts_and_an_expired_one_fails() {
    let fixture = fixture("exempt");
    summary(
        &fixture,
        &[
            ("src/lib.rs", 100, 100, 100, 100),
            ("src/gen/x.rs", 10, 100, 100, 100),
        ],
    );
    crap(
        &fixture,
        &[
            ("./src/main.rs", "main", 29.0),
            ("./src/lib.rs", "old", 1.0),
        ],
    );
    edit(&fixture, |doc| {
        set(doc, "/gates/coverage", "line_min_pct", Some(json!(50)));
        set(doc, "", "exemptions", Some(exemptions()));
    });
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains("  - crap: the exemption for src/lib.rs (old) expired on 2020-01-01"),
        "{text}"
    );
    assert!(text.contains("fail: 1 problem(s)"), "{text}");
}

#[test]
fn a_workspace_root_instance_judges_only_its_named_member() {
    let fixture = fixture("workspace");
    fs::write(
        fixture.repo.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\n",
    )
    .expect("write");
    for crate_dir in ["crates/fixture/src", "crates/other/src"] {
        fs::create_dir_all(fixture.repo.join(crate_dir)).expect("mkdir");
        fs::write(fixture.repo.join(crate_dir).join("lib.rs"), "").expect("write");
    }
    let member = fixture
        .repo
        .join("crates/fixture")
        .canonicalize()
        .expect("member");
    let metadata = json!({"packages": [
        {"name": "other", "manifest_path": fixture.repo.join("crates/other/Cargo.toml")},
        {"name": "fixture", "manifest_path": member.join("Cargo.toml")}]});
    fs::write(fixture.reports.join("metadata.json"), metadata.to_string()).expect("write");
    let own = member.join("src/lib.rs").display().to_string();
    let other = fixture
        .repo
        .join("crates/other/src/lib.rs")
        .display()
        .to_string();
    summary(
        &fixture,
        &[(&own, 100, 100, 100, 100), (&other, 0, 100, 0, 100)],
    );
    crap(
        &fixture,
        &[
            ("./crates/fixture/src/lib.rs", "f", 2.0),
            ("./crates/other/src/lib.rs", "g", 90.0),
        ],
    );
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(0), "{text}");
    assert!(
        text.contains(&format!("floors for {}:", member.display())),
        "{text}"
    );
}

#[test]
fn no_floor_and_no_threshold_judge_nothing() {
    let fixture = fixture("unset");
    summary(&fixture, &[]);
    crap(&fixture, &[("./src/lib.rs", "big", 99.0)]);
    edit(&fixture, |doc| {
        for key in ["line_min_pct", "region_min_pct", "per_file_line_min_pct"] {
            set(doc, "/gates/coverage", key, None);
        }
        for key in ["threshold_agent", "threshold_production", "threshold_human"] {
            set(doc, "/gates/crap", key, None);
        }
    });
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(0), "{text}");
    assert!(text.contains("floors for "), "{text}");
    assert!(
        text.contains("line not set, region not set, per-file line not set"),
        "{text}"
    );
    assert!(
        text.contains("no floor is set, so there is nothing to judge"),
        "{text}"
    );
    assert!(
        text.contains("==> [crap] no threshold_agent or threshold_production: not judged here"),
        "{text}"
    );
}

#[test]
fn threshold_production_stands_in_for_a_missing_agent_threshold() {
    let fixture = fixture("production");
    good_summary(&fixture);
    crap(&fixture, &[("./src/lib.rs", "f", 7.0)]);
    edit(&fixture, |doc| {
        set(doc, "/gates/crap", "threshold_agent", None);
        set(doc, "/gates/crap", "threshold_production", Some(json!(8)));
    });
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(0), "{text}");
    assert!(
        text.contains("==> [crap] threshold 8 on new functions"),
        "{text}"
    );
}

#[test]
fn a_report_that_is_missing_or_not_json_fails_and_a_failed_run_is_not_judged() {
    let fixture = fixture("broken");
    summary(&fixture, &[]);
    fs::write(fixture.reports.join("coverage.json"), "{}").expect("write");
    fs::write(fixture.reports.join("crap.json"), "stub").expect("write");
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains("coverage.json: the summary has no data[0].files"),
        "{text}"
    );
    assert!(text.contains("crap.json: the report is not JSON"), "{text}");
    // A coverage run that failed wrote no summary to judge; the crap run
    // still passed, so its report is judged.
    let (code, text) = tops_failing(&fixture, "llvm-cov nextest");
    assert_eq!(code, Some(1), "{text}");
    assert!(!text.contains("floors for "), "{text}");
    assert!(text.contains("crap.json: the report is not JSON"), "{text}");
}

fn tops_failing(fixture: &Fixture, pattern: &str) -> (Option<i32>, String) {
    let path = stub_path(fixture);
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(["gate", "--path"])
        .arg(&fixture.repo)
        .env("PATH", path)
        .env("REPORTS", &fixture.reports)
        .env("CARGO_STUB_FAIL", pattern)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("spawn cargo-tops");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn coverage_floors_are_judged_with_the_crap_layer_off() {
    let fixture = fixture("coverage-only");
    summary(&fixture, &[("src/lib.rs", 50, 100, 50, 100)]);
    edit(&fixture, |doc| {
        let layers = doc.pointer_mut("/layers").and_then(Value::as_array_mut);
        for layer in layers.expect("layers") {
            let crap = layer.get("id").and_then(Value::as_str) == Some("crap");
            if crap {
                set(layer, "", "enabled", Some(Value::Bool(false)));
            }
        }
    });
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains("  - coverage: line coverage 50.00% (50 of 100 lines)"),
        "{text}"
    );
    assert!(!text.contains("==> [crap] threshold"), "{text}");
}

#[test]
fn a_baseline_that_cannot_be_read_fails_and_is_not_taken_as_absent() {
    let fixture = fixture("unreadable");
    good_summary(&fixture);
    crap(&fixture, &[("./src/lib.rs", "f", 2.0)]);
    fs::create_dir_all(fixture.repo.join("ci/baseline.json")).expect("mkdir");
    edit(&fixture, |doc| {
        set(
            doc,
            "/gates/crap",
            "baseline",
            Some(json!("ci/baseline.json")),
        );
    });
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(1), "{text}");
    let shown = fixture.repo.join("ci/baseline.json").display().to_string();
    assert!(
        text.contains(&format!("  - crap: baseline {shown}: ")),
        "{text}"
    );
    assert!(!text.contains("no baseline"), "{text}");
}

/// The argv of every `cargo deny` the stub saw, one per line.
fn deny_runs(log: &Path) -> Vec<String> {
    fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("deny ") && !line.ends_with("--version"))
        .map(str::to_string)
        .collect()
}

#[test]
fn each_deny_check_kind_runs_once_per_gate() {
    let fixture = fixture_with("deny-once", &["static-lint", "security-advisory"]);
    let log = fixture.root.join("cargo.log");
    let (code, text) = tops_with_log(&fixture, &log, None);
    assert_eq!(code, Some(0), "{text}");
    let runs = deny_runs(&log);
    for kind in ["advisories", "bans", "licenses", "sources"] {
        let hits = runs
            .iter()
            .flat_map(|run| run.split_whitespace().skip(2))
            .filter(|word| *word == kind)
            .count();
        assert_eq!(hits, 1, "{kind} ran {hits} times: {runs:?}");
    }
    assert!(
        runs.iter()
            .all(|run| run.split_whitespace().nth(1) == Some("check")),
        "{runs:?}"
    );
}

#[test]
fn a_failing_advisory_check_fails_security_advisory() {
    let fixture = fixture_with("deny-advisory", &["static-lint", "security-advisory"]);
    let log = fixture.root.join("cargo.log");
    let (code, text) = tops_with_log(&fixture, &log, Some("deny check advisories"));
    assert_eq!(code, Some(1), "{text}");
    // The failure is the advisories command exiting 1, not a tool that is
    // missing: all four commands ran, and no other layer failed.
    assert!(
        text.contains("  - security-advisory: `cargo deny check advisories` failed"),
        "{text}"
    );
    assert!(text.contains("count: 4 command(s) ran"), "{text}");
    assert!(text.contains("1 problem(s)"), "{text}");
    assert!(!text.contains("not on PATH"), "{text}");
    assert!(!text.contains("\nok: "), "{text}");
    // The same failure on a kind the other layer owns fails that layer, not
    // `security-advisory`.
    let (code, text) = tops_with_log(&fixture, &log, Some("deny check bans"));
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains("  - static-lint: `cargo deny check bans licenses sources` failed"),
        "{text}"
    );
    assert!(!text.contains("  - security-advisory:"), "{text}");
}

/// `gate`, with the cargo stub logging every argv to `log` and failing the
/// argv that contains `fail`.
fn tops_with_log(fixture: &Fixture, log: &Path, fail: Option<&str>) -> (Option<i32>, String) {
    fs::remove_file(log).ok();
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .arg("gate")
        .arg("--path")
        .arg(&fixture.repo)
        .env("PATH", stub_path(fixture))
        .env("REPORTS", &fixture.reports)
        .env("CARGO_STUB_LOG", log)
        .env("CARGO_STUB_FAIL", fail.unwrap_or(""))
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("spawn cargo-tops");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code(), text)
}

/// The report the last gate wrote.
fn written(fixture: &Fixture) -> Value {
    let raw = fs::read_to_string(fixture.repo.join("target/rust-tops/report.json"))
        .expect("report.json was written");
    serde_json::from_str(&raw).expect("report.json is JSON")
}

/// A member of `value` by JSON pointer; `Null` when it is not there.
fn at<'v>(value: &'v Value, pointer: &str) -> &'v Value {
    static NULL: Value = Value::Null;
    value.pointer(pointer).unwrap_or(&NULL)
}

fn row<'v>(report: &'v Value, id: &str) -> &'v Value {
    at(report, "/layers")
        .as_array()
        .and_then(|rows| rows.iter().find(|row| at(row, "/id") == id))
        .unwrap_or_else(|| at(report, "/missing"))
}

#[test]
fn the_report_carries_the_numbers_a_passing_floor_was_judged_on() {
    let fixture = fixture("numbers-pass");
    good_summary(&fixture);
    crap(&fixture, &[("./src/lib.rs", "parse", 6.0)]);
    let (code, text) = tops(&fixture, &["gate", "--max-tier", "0"]);
    assert_eq!(code, Some(0), "{text}");
    let report = written(&fixture);
    let coverage = row(&report, "coverage");
    assert_eq!(at(coverage, "/verdict"), "pass", "{coverage}");
    assert_eq!(
        at(coverage, "/judged"),
        &json!({
            "line_pct": 95.0, "region_pct": 90.0,
            "line_floor": 95.0, "region_floor": 90.0, "per_file_line_floor": 90.0,
            "files": 1,
        })
    );
    let crap = row(&report, "crap");
    assert_eq!(at(crap, "/verdict"), "pass", "{crap}");
    assert_eq!(
        at(crap, "/judged"),
        &json!({"threshold": 6.0, "max": 6.0, "functions": 1, "over": 0, "baseline": false})
    );
}

#[test]
fn a_run_that_passed_but_missed_a_floor_is_a_failed_layer_with_its_numbers() {
    let fixture = fixture("numbers-miss");
    summary(&fixture, &[("src/a.rs", 80, 100, 80, 100)]);
    crap(
        &fixture,
        &[("./src/lib.rs", "big", 12.0), ("./src/lib.rs", "ok", 2.0)],
    );
    let (code, text) = tops(&fixture, &["gate", "--max-tier", "0"]);
    assert_eq!(code, Some(1), "{text}");
    let report = written(&fixture);
    let coverage = row(&report, "coverage");
    assert_eq!(at(coverage, "/verdict"), "fail", "{coverage}");
    assert_eq!(at(coverage, "/judged/line_pct"), &json!(80.0));
    let crap = row(&report, "crap");
    assert_eq!(at(crap, "/verdict"), "fail", "{crap}");
    assert_eq!(at(crap, "/judged/max"), &json!(12.0));
    assert_eq!(at(crap, "/judged/functions"), &json!(2));
    assert_eq!(at(crap, "/judged/over"), &json!(1));
}

#[test]
fn a_layer_that_takes_another_layers_run_says_which_and_shows_its_commands() {
    let fixture = fixture_with("took", &["coverage", "unit"]);
    good_summary(&fixture);
    let (code, text) = tops(&fixture, &["gate", "--max-tier", "0"]);
    assert_eq!(code, Some(0), "{text}");
    let report = written(&fixture);
    let unit = row(&report, "unit");
    assert_eq!(at(unit, "/took_from"), "coverage", "{unit}");
    assert_eq!(at(unit, "/argv"), at(row(&report, "coverage"), "/argv"));
    assert!(row(&report, "coverage").get("took_from").is_none());
}

#[test]
fn the_crap_layer_alone_runs_the_pin_check_and_only_with_a_lock() {
    let pin = json!(["cargo", "crap", "--version"]);
    let fixture = fixture("pin");
    good_summary(&fixture);
    crap(&fixture, &[("./src/lib.rs", "parse", 2.0)]);
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(0), "{text}");
    let report = written(&fixture);
    assert_eq!(at(row(&report, "crap"), "/argv/0"), &pin);
    assert_ne!(at(row(&report, "coverage"), "/argv/0"), &pin);

    fs::remove_file(fixture.repo.join("rust-tops.lock")).expect("remove the lock");
    let (code, text) = gate(&fixture);
    assert_eq!(code, Some(0), "{text}");
    assert_ne!(at(row(&written(&fixture), "crap"), "/argv/0"), &pin);
}
