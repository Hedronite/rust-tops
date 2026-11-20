//! The credited run is real nextest. The cargo stub plays `cargo llvm-cov
//! nextest` by running `cargo nextest run --profile ci` on a small crate, so
//! the JUnit report is written where nextest writes it, not where the gate
//! looks. These tests cover a `CARGO_TARGET_DIR` that moves the build, a
//! `[store] dir` that moves nextest's reports, and a report left behind by an
//! earlier run (PROTOCOL.md §9.5).
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// `llvm-cov nextest` writes the stand-in LCOV, then runs real nextest when
/// `$REAL_NEXTEST` is set. `llvm-cov report` writes a fully covered summary.
/// Everything else (`cargo metadata`) is the real cargo.
const CARGO_STUB: &str = r#"#!/bin/sh
if [ "$1 $2" = "llvm-cov nextest" ]; then
  prev=
  for arg in "$@"; do
    case $prev in --output-path) mkdir -p "$(dirname "$arg")"; printf 'lcov\n' > "$arg" ;; esac
    prev=$arg
  done
  if [ -n "${REAL_NEXTEST:-}" ]; then
    PATH="$REAL_PATH" "$REAL_CARGO" nextest run --workspace --all-features --profile ci >&2 || exit 1
  fi
  exit 0
fi
if [ "$1 $2" = "llvm-cov report" ]; then
  prev=
  for arg in "$@"; do
    case $prev in --output-path) mkdir -p "$(dirname "$arg")"; printf '%s\n' '{"data":[{"files":[{"filename":"src/lib.rs","summary":{"lines":{"count":10,"covered":10},"regions":{"count":10,"covered":10}}}]}]}' > "$arg" ;; esac
    prev=$arg
  done
  exit 0
fi
PATH="$REAL_PATH" exec "$REAL_CARGO" "$@"
"#;

const WITH_TEST: &str =
    "#[cfg(test)]\nmod tests {\n    #[test]\n    fn property_roundtrip() {}\n}\n";
const WITHOUT_TEST: &str = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn it_works() {}\n}\n";

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    bin: PathBuf,
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

/// A real one-crate repo: coverage, `security-advisory` and `property` on.
fn fixture(name: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!("cargo-tops-nx-{name}-{}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    let fixture = Fixture {
        repo: root.join("repo"),
        bin: root.join("bin"),
        root,
    };
    fs::create_dir_all(&fixture.bin).expect("mkdir");
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
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");
    fs::create_dir_all(fixture.repo.join("src")).expect("mkdir src");
    set_tests(&fixture, WITH_TEST);
    stub(&fixture.bin, "cargo", CARGO_STUB);
    for plugin in ["cargo-llvm-cov", "cargo-deny"] {
        stub(&fixture.bin, plugin, "#!/bin/sh\nexit 0\n");
    }
    enable_required_layers(&fixture);
    fixture
}

/// Coverage, `security-advisory` and `property` on; every other layer off.
fn enable_required_layers(fixture: &Fixture) {
    let path = fixture.repo.join("rust-tops.yaml");
    let mut doc: Value =
        serde_saphyr::from_str(&fs::read_to_string(&path).expect("read")).expect("yaml");
    for layer in doc
        .pointer_mut("/layers")
        .and_then(Value::as_array_mut)
        .expect("layers")
    {
        let id = layer.get("id").and_then(Value::as_str).unwrap_or_default();
        let on = matches!(id, "coverage" | "security-advisory" | "property");
        layer
            .as_object_mut()
            .expect("a layer is an object")
            .insert("enabled".to_string(), Value::Bool(on));
    }
    fs::write(&path, serde_json::to_string_pretty(&doc).expect("json")).expect("write yaml");
}

fn set_tests(fixture: &Fixture, source: &str) {
    fs::write(fixture.repo.join("src/lib.rs"), source).expect("write lib.rs");
}

fn append_nextest_config(fixture: &Fixture, text: &str) {
    let path = fixture.repo.join(".config/nextest.toml");
    let old = fs::read_to_string(&path).expect("init writes .config/nextest.toml");
    fs::write(path, format!("{old}\n{text}\n")).expect("write nextest.toml");
}

/// What one gate run left: the exit code, stdout and stderr, and the
/// `property` row and `problems` of `report.json`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Credited {
    RealNextest,
    /// The credited run writes no report.
    Idle,
}

struct Gate {
    code: Option<i32>,
    text: String,
    verdict: String,
    detail: String,
    problems: Vec<String>,
}

/// The first `cargo` on the inherited `PATH`, before any stub.
fn real_cargo(path: &std::ffi::OsStr) -> PathBuf {
    std::env::split_paths(path)
        .map(|dir| dir.join("cargo"))
        .find(|candidate| candidate.is_file())
        .expect("a real cargo on PATH")
}

/// Runs the gate. `credited` says whether the credited run is real nextest;
/// `target_dir` is a `CARGO_TARGET_DIR`.
fn gate(fixture: &Fixture, credited: Credited, target_dir: Option<&Path>) -> Gate {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let dirs = std::iter::once(fixture.bin.clone()).chain(std::env::split_paths(&inherited));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cargo-tops"));
    cmd.args(["gate", "--path"])
        .arg(&fixture.repo)
        .env("PATH", std::env::join_paths(dirs).expect("PATH"))
        .env("REAL_PATH", &inherited)
        .env("REAL_CARGO", real_cargo(&inherited))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("REAL_NEXTEST");
    if credited == Credited::RealNextest {
        cmd.env("REAL_NEXTEST", "1");
    }
    if let Some(dir) = target_dir {
        cmd.env("CARGO_TARGET_DIR", dir);
    }
    let out = cmd.output().expect("spawn cargo-tops");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let report_dir = target_dir.map_or_else(|| fixture.repo.join("target"), Path::to_path_buf);
    let (verdict, detail, problems) = read_report(&report_dir);
    Gate {
        code: out.status.code(),
        verdict,
        detail,
        problems,
        text,
    }
}

/// The `property` row's verdict and detail, and the `problems`, of the
/// `report.json` under `report_dir`.
fn read_report(report_dir: &Path) -> (String, String, Vec<String>) {
    let report = fs::read_to_string(report_dir.join("rust-tops/report.json"))
        .expect("the gate wrote report.json");
    let report: Value = serde_json::from_str(&report).expect("report json");
    let row = report
        .get("layers")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("id").and_then(Value::as_str) == Some("property"))
        })
        .expect("a property row");
    let text_of = |key: &str| {
        row.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let problems = report
        .get("problems")
        .and_then(Value::as_array)
        .map(|all| {
            all.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    (text_of("verdict"), text_of("detail"), problems)
}

#[test]
fn a_cargo_target_dir_does_not_move_the_evidence() {
    let fixture = fixture("target-dir");
    let target = fixture.root.join("elsewhere");
    let run = gate(&fixture, Credited::RealNextest, Some(&target));
    assert_eq!(run.code, Some(0), "{}", run.text);
    assert_eq!(run.verdict, "pass", "{}", run.text);
    // nextest wrote under the workspace root, not under the target dir.
    assert!(fixture.repo.join("target/nextest/ci/junit.xml").is_file());
    assert!(!target.join("nextest").exists());
}

#[test]
fn a_moved_store_dir_is_read_where_nextest_wrote_it() {
    let fixture = fixture("store");
    append_nextest_config(&fixture, "[store]\ndir = \"target/nt\"");
    let run = gate(&fixture, Credited::RealNextest, None);
    assert_eq!(run.code, Some(0), "{}", run.text);
    assert_eq!(run.verdict, "pass", "{}", run.text);
    assert!(fixture.repo.join("target/nt/ci/junit.xml").is_file());
}

#[test]
fn a_report_from_an_earlier_run_is_stale_and_kept() {
    let fixture = fixture("stale");
    let first = gate(&fixture, Credited::RealNextest, None);
    assert_eq!(first.verdict, "pass", "{}", first.text);
    let old = fixture.repo.join("target/nextest/ci/junit.xml");
    assert!(old.is_file());

    // The test goes, the store moves, and the next run writes elsewhere:
    // the old report still holds the test, and is not read.
    set_tests(&fixture, WITHOUT_TEST);
    append_nextest_config(&fixture, "[store]\ndir = \"target/nt\"");
    let moved = gate(&fixture, Credited::RealNextest, None);
    assert_eq!(moved.code, Some(1), "{}", moved.text);
    assert_eq!(moved.verdict, "fail", "{}", moved.text);
    assert!(
        moved.detail.contains("target/nt/ci/junit.xml"),
        "{}",
        moved.detail
    );
    assert!(moved.detail.contains("no test matches"), "{}", moved.detail);

    // A credited run that writes no report leaves only the old one behind.
    let config = fixture.repo.join(".config/nextest.toml");
    let text = fs::read_to_string(&config)
        .expect("read")
        .replace("target/nt", "target/nextest");
    fs::write(&config, text).expect("write");
    let idle = gate(&fixture, Credited::Idle, None);
    assert_eq!(idle.code, Some(1), "{}", idle.text);
    assert_eq!(idle.verdict, "fail", "{}", idle.text);
    assert!(
        idle.detail.contains("target/nextest/ci/junit.xml"),
        "{}",
        idle.detail
    );
    assert!(
        idle.detail.contains("older than the credited run"),
        "{}",
        idle.detail
    );
    assert!(old.is_file(), "the gate never deletes the report");
}

#[test]
fn a_failed_evidence_check_is_worded_once_in_detail_problems_and_stdout() {
    let fixture = fixture("wording");
    set_tests(&fixture, WITHOUT_TEST);
    let run = gate(&fixture, Credited::RealNextest, None);
    assert_eq!(run.code, Some(1), "{}", run.text);
    let line = "no passed test whose name has a `::` segment starting with `property`";
    assert!(run.detail.contains(line), "{}", run.detail);
    let problem = run
        .problems
        .iter()
        .find(|problem| problem.starts_with("property:"))
        .expect("a property problem");
    assert!(problem.contains(line), "{problem}");
    assert!(problem.contains("target/nextest/ci/junit.xml"), "{problem}");
    assert!(
        run.text.contains(&format!(
            "[property] took the coverage run, but it is no credit: {}",
            run.detail
        )),
        "{}",
        run.text
    );
    assert!(!run.text.contains("run it took is Failed"), "{}", run.text);
    assert!(
        !run.problems.iter().any(|p| p.contains("run it took is")),
        "{:?}",
        run.problems
    );
}
