//! A credited `property` or `acceptance` layer passes only when the credited
//! run's nextest JUnit report holds a passed test whose name contains the
//! layer's `test_filter` (PROTOCOL.md §6, §9).
//!
//! Cargo and its plugins are stubs. The cargo stub plays the coverage run:
//! it writes the JUnit report the test put in the fixture's `reports/` to
//! `target/nextest/ci/junit.xml` in the repo, the way nextest does, and
//! writes no report when the test left none. Only the layers the protocol
//! requires of a library-core instance, plus the one under test, are on.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

/// Answers `crap --version` and `metadata`, copies the stand-in reports the
/// way the floors tests do, and, for `llvm-cov nextest`, plays nextest: the
/// JUnit report in `$REPORTS/junit.xml` goes to the path nextest uses.
const CARGO_STUB: &str = r#"#!/bin/sh
if [ "$1 $2" = "crap --version" ]; then echo "cargo-crap 0.9.0"; exit 0; fi
if [ "$1" = metadata ]; then cat "$REPORTS/metadata.json"; exit 0; fi
if [ "$1 $2" = "llvm-cov nextest" ] && [ -f "$REPORTS/junit.xml" ]; then
  mkdir -p "$REPORTS/../repo/target/nextest/ci"
  cp "$REPORTS/junit.xml" "$REPORTS/../repo/target/nextest/ci/junit.xml"
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

/// A library-core instance named `fixture` with coverage, the layers the
/// protocol requires of the class, and `extra` on, and coverage's JUnit
/// report `junit` (or none).
fn fixture(name: &str, extra: &[&str], junit: Option<&str>) -> Fixture {
    let root =
        std::env::temp_dir().join(format!("cargo-tops-credit-{name}-{}", std::process::id()));
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
    fs::write(
        fixture.repo.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");
    stage_tools_and_reports(&fixture, junit);
    enable_only(&fixture, extra);
    fixture
}

/// The tool stubs, the coverage summary, and coverage's JUnit report.
fn stage_tools_and_reports(fixture: &Fixture, junit: Option<&str>) {
    stub(&fixture.bin, "cargo", CARGO_STUB);
    for plugin in [
        "cargo-llvm-cov",
        "cargo-crap",
        "cargo-nextest",
        "cargo-deny",
    ] {
        stub(&fixture.bin, plugin, "#!/bin/sh\nexit 0\n");
    }
    fs::write(
        fixture.reports.join("coverage.json"),
        json!({"type": "llvm.coverage.json.export", "data": [{"files": [
            {"filename": "src/lib.rs", "summary": {
                "lines": {"covered": 100, "count": 100},
                "regions": {"covered": 100, "count": 100}}}]}]})
        .to_string(),
    )
    .expect("write summary");
    if let Some(xml) = junit {
        fs::write(fixture.reports.join("junit.xml"), xml).expect("write junit");
    }
}

/// Coverage, the required layers and `extra` on; every other layer off.
fn enable_only(fixture: &Fixture, extra: &[&str]) {
    edit(fixture, |doc| {
        let layers = doc.pointer_mut("/layers").and_then(Value::as_array_mut);
        for layer in layers.expect("layers") {
            let id = layer.get("id").and_then(Value::as_str).unwrap_or_default();
            let on =
                matches!(id, "coverage" | "security-advisory" | "property") || extra.contains(&id);
            layer
                .as_object_mut()
                .expect("a layer is an object")
                .insert("enabled".to_string(), Value::Bool(on));
        }
    });
}

/// Sets `gates.<layer>.test_filter`.
fn set_filter(fixture: &Fixture, layer: &str, filter: &str) {
    edit(fixture, |doc| {
        let gates = doc
            .get_mut("gates")
            .and_then(Value::as_object_mut)
            .expect("gates");
        gates
            .entry(layer.to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("a gate is an object")
            .insert("test_filter".to_string(), json!(filter));
    });
}

fn edit(fixture: &Fixture, change: impl FnOnce(&mut Value)) {
    let path = fixture.repo.join("rust-tops.yaml");
    let mut doc: Value =
        serde_saphyr::from_str(&fs::read_to_string(&path).expect("read")).expect("yaml");
    change(&mut doc);
    fs::write(&path, serde_json::to_string_pretty(&doc).expect("json")).expect("write yaml");
}

/// A JUnit report holding these tests, each `(name, passed)`.
fn junit(tests: &[(&str, bool)]) -> String {
    let cases: String = tests
        .iter()
        .map(|&(name, passed)| {
            if passed {
                format!("<testcase name=\"{name}\" classname=\"fixture\" time=\"0.01\"/>")
            } else {
                format!(
                    "<testcase name=\"{name}\" classname=\"fixture\" time=\"0.01\">\
                     <failure message=\"boom\"/></testcase>"
                )
            }
        })
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites name=\"nextest-run\">\
         <testsuite name=\"fixture\">{cases}</testsuite></testsuites>"
    )
}

/// The gate, and the verdict and detail `report.json` gives `layer`.
fn gate(fixture: &Fixture, layer: &str) -> (Option<i32>, String, String, String) {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let dirs = std::iter::once(fixture.bin.clone()).chain(std::env::split_paths(&inherited));
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(["gate", "--path"])
        .arg(&fixture.repo)
        .env("PATH", std::env::join_paths(dirs).expect("PATH"))
        .env("REPORTS", &fixture.reports)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("spawn cargo-tops");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let report = fs::read_to_string(fixture.repo.join("target/rust-tops/report.json"))
        .expect("the gate wrote report.json");
    let report: Value = serde_json::from_str(&report).expect("report json");
    let row = report
        .get("layers")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("id").and_then(Value::as_str) == Some(layer))
        })
        .expect("the layer has a row");
    let field = |key: &str| {
        row.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    (out.status.code(), text, field("verdict"), field("detail"))
}

#[test]
fn a_coverage_run_with_no_property_test_fails_property() {
    let fixture = fixture("none", &[], Some(&junit(&[("plain", true)])));
    let (code, text, verdict, detail) = gate(&fixture, "property");
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(verdict, "fail", "{text}");
    assert!(detail.contains("`property`"), "{detail}");
    assert!(detail.contains("target/nextest/ci/junit.xml"), "{detail}");
}

#[test]
fn a_property_test_in_the_coverage_run_passes_property() {
    let fixture = fixture("some", &[], Some(&junit(&[("property_roundtrip", true)])));
    let (code, text, verdict, _) = gate(&fixture, "property");
    assert_eq!(code, Some(0), "{text}");
    assert_eq!(verdict, "pass", "{text}");
}

#[test]
fn a_failed_property_test_is_not_evidence() {
    let fixture = fixture(
        "failed",
        &[],
        Some(&junit(&[("property_roundtrip", false)])),
    );
    let (code, _, verdict, detail) = gate(&fixture, "property");
    assert_ne!(code, Some(0));
    assert_eq!(verdict, "fail");
    assert!(detail.contains("failed or was skipped"), "{detail}");
}

#[test]
fn a_missing_junit_report_fails_and_names_the_setting() {
    let fixture = fixture("nojunit", &[], None);
    let (code, text, verdict, detail) = gate(&fixture, "property");
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(verdict, "fail", "{text}");
    assert!(detail.contains("[profile.ci.junit]"), "{detail}");
}

#[test]
fn a_custom_test_filter_is_honoured() {
    let tests = junit(&[("prop_roundtrip", true)]);
    let fixture = fixture("filter", &[], Some(&tests));
    set_filter(&fixture, "property", "prop_");
    let (code, text, verdict, _) = gate(&fixture, "property");
    assert_eq!(code, Some(0), "{text}");
    assert_eq!(verdict, "pass", "{text}");
    // The default filter finds nothing in a report that names only `prop_`.
    set_filter(&fixture, "property", "property");
    let (_, _, verdict, detail) = gate(&fixture, "property");
    assert_eq!(verdict, "fail");
    assert!(detail.contains("`property`"), "{detail}");
}

#[test]
fn acceptance_follows_the_same_rule_with_its_own_filter() {
    let with = fixture(
        "accept-pass",
        &["acceptance"],
        Some(&junit(&[
            ("property_a", true),
            ("acceptance_journey", true),
        ])),
    );
    let (code, text, verdict, _) = gate(&with, "acceptance");
    assert_eq!(code, Some(0), "{text}");
    assert_eq!(verdict, "pass", "{text}");

    let without = fixture(
        "accept-fail",
        &["acceptance"],
        Some(&junit(&[("property_a", true)])),
    );
    let (code, text, verdict, detail) = gate(&without, "acceptance");
    assert_eq!(code, Some(1), "{text}");
    assert_eq!(verdict, "fail", "{text}");
    assert!(detail.contains("`acceptance`"), "{detail}");

    set_filter(&without, "acceptance", "property");
    let (_, _, verdict, _) = gate(&without, "acceptance");
    assert_eq!(verdict, "pass", "its own filter, not property's");
}

/// The kit holds the property tests its own `property` layer credits. This is
/// the cheap half of the plant: removing `tests/property.rs` leaves the kit's
/// coverage run with no test named `property`, and the gate fails the layer.
#[test]
fn the_kit_has_at_least_three_property_tests() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/property.rs"))
            .expect("the kit's tests/property.rs");
    let named = source.matches("fn property_").count();
    assert!(named >= 3, "{named} property tests");
    assert!(source.contains("proptest!"), "they use proptest");
}
