//! `cargo tops gate` writes `<target>/rust-tops/report.json` on every run and
//! exits 0 (pass), 1 (the gate ran and failed, a dry run included), 2 (the
//! config is invalid) or 3 (the environment). Where more than one applies, 2
//! wins over 3 and 3 over 1.
//!
//! Cargo and its plugins are stubs. Every report is validated against
//! `schema/gate-report.schema.json`.
mod support;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

/// The validator `cargo tops check` uses, and the schema it reads.
#[path = "../src/schema.rs"]
mod schema;

mod profiles {
    pub(crate) const SCHEMA_JSON: &str = include_str!("../templates/rust-tops.schema.json");
}

/// A member of `value` by JSON pointer; `Null` when it is not there, so the
/// assertion that reads it fails with the report in its message.
fn at<'v>(value: &'v Value, pointer: &str) -> &'v Value {
    static NULL: Value = Value::Null;
    value.pointer(pointer).unwrap_or(&NULL)
}

const REPORT_SCHEMA: &str = include_str!("../../../schema/gate-report.schema.json");

/// Fails for an argv that matches `$CARGO_STUB_FAIL`; answers every
/// `--version` ask; passes everything else.
const CARGO_STUB: &str = r#"#!/bin/sh
if [ -n "${CARGO_STUB_LOG:-}" ]; then echo "$*" >> "$CARGO_STUB_LOG"; fi
case "$1 $2" in
  "nextest --version") echo "cargo-nextest 0.9.145 (stub)"; exit 0 ;;
  "llvm-cov --version") echo "cargo-llvm-cov 0.9.1"; exit 0 ;;
  "mutants --version") echo "cargo-mutants 27.1.0"; exit 0 ;;
  "crap --version") echo "cargo-crap 0.9.0"; exit 0 ;;
  "deny --version") echo "cargo-deny 0.18.9"; exit 0 ;;
  "fuzz --version") echo "cargo-fuzz 0.12.0"; exit 0 ;;
  "kani --version") echo "Kani Rust Verifier 0.68.0 (cargo plugin)"; exit 0 ;;
  "clippy --version") echo "clippy 0.1.99 (stub)"; exit 0 ;;
  "fmt --version") echo "rustfmt 1.10.0-stable (stub)"; exit 0 ;;
esac
case "$*" in *--version) echo "stub-tool 1.2.3"; exit 0 ;; esac
if [ -n "${CARGO_STUB_FAIL:-}" ]; then
  case "$*" in *"$CARGO_STUB_FAIL"*) exit 1 ;; esac
fi
exit 0
"#;

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

/// A library-core instance named `fixture` with only `static-lint` on: three
/// commands, `fmt`, `clippy` and `deny`.
fn fixture(name: &str) -> Fixture {
    let root =
        std::env::temp_dir().join(format!("cargo-tops-report-{name}-{}", std::process::id()));
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
    // `cargo metadata` is asked about a project with a manifest.
    fs::write(
        fixture.repo.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");
    stub(&fixture.bin, "cargo", CARGO_STUB);
    support::stub_rustc(&fixture.bin);
    stub(&fixture.bin, "cargo-deny", "#!/bin/sh\nexit 0\n");
    stub(&fixture.bin, "cargo-nextest", "#!/bin/sh\nexit 0\n");
    edit(&fixture, |doc| {
        let layers = doc.pointer_mut("/layers").and_then(Value::as_array_mut);
        for layer in layers.expect("layers") {
            let on = stays_on(layer);
            set(layer, "enabled", Value::Bool(on));
        }
    });
    fixture
}

/// Whether the fixture leaves a layer on: `static-lint`, plus the layers
/// protocol 1.3.0 requires of a library-core instance.
fn stays_on(layer: &Value) -> bool {
    matches!(
        layer.get("id").and_then(Value::as_str),
        Some("static-lint" | "security-advisory" | "property")
    )
}

/// Sets `key` in an object.
fn set(doc: &mut Value, key: &str, value: Value) {
    doc.as_object_mut()
        .expect("an object")
        .insert(key.to_string(), value);
}

fn edit(fixture: &Fixture, change: impl FnOnce(&mut Value)) {
    let path = fixture.repo.join("rust-tops.yaml");
    let mut doc: Value =
        serde_saphyr::from_str(&fs::read_to_string(&path).expect("read")).expect("yaml");
    change(&mut doc);
    fs::write(&path, serde_json::to_string_pretty(&doc).expect("json")).expect("write yaml");
}

/// The stubs, then the system directories for `sh` and the rest. No
/// `cargo-deny` is there unless a stub put it first.
fn path(fixture: &Fixture) -> std::ffi::OsString {
    std::env::join_paths([
        fixture.bin.as_path(),
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .expect("PATH")
}

fn gate(fixture: &Fixture, args: &[&str], fail: Option<&str>) -> (Output, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cargo-tops"));
    cmd.arg("gate")
        .args(args)
        .arg("--path")
        .arg(&fixture.repo)
        .env("PATH", path(fixture))
        .env("CARGO_STUB_LOG", fixture.root.join("cargo.log"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_STUB_FAIL");
    if let Some(pattern) = fail {
        cmd.env("CARGO_STUB_FAIL", pattern);
    }
    let out = cmd.output().expect("spawn cargo-tops gate");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out, text)
}

fn report_path(fixture: &Fixture) -> PathBuf {
    fixture.repo.join("target/rust-tops/report.json")
}

/// The report the last gate wrote, checked against the schema.
fn report(fixture: &Fixture) -> Value {
    let raw = fs::read_to_string(report_path(fixture)).expect("report.json was written");
    let doc: Value = serde_json::from_str(&raw).expect("report.json is JSON");
    let schema: Value = serde_json::from_str(REPORT_SCHEMA).expect("schema is JSON");
    let errors = schema::validate(&schema, &doc);
    assert!(
        errors.is_empty(),
        "report.json breaks the schema: {errors:?}\n{raw}"
    );
    doc
}

fn layer<'v>(report: &'v Value, id: &str) -> &'v Value {
    at(report, "/layers")
        .as_array()
        .and_then(|layers| layers.iter().find(|row| at(row, "/id") == id))
        .unwrap_or_else(|| at(report, "/missing"))
}

/// What every report holds, whatever the run.
fn expect_consistent(report: &Value, code: i64) {
    assert_eq!(at(report, "/exit_code"), &json!(code), "{report}");
    assert_eq!(at(report, "/ok"), &json!(code == 0), "{report}");
    assert_eq!(
        at(report, "/problems")
            .as_array()
            .is_some_and(Vec::is_empty),
        code == 0,
        "{report}"
    );
    for row in at(report, "/layers").as_array().expect("layers") {
        let seconds = at(row, "/seconds").as_f64().expect("seconds is a number");
        assert!(seconds >= 0.0, "{row}");
    }
}

#[test]
fn a_pass_exits_0_and_the_report_says_so() {
    let fixture = fixture("pass");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    let report = report(&fixture);
    expect_consistent(&report, 0);
    assert_eq!(at(&report, "/kind"), "pass");
    assert_eq!(at(&report, "/crate"), "fixture");
    assert_eq!(at(&report, "/max_tier"), 1);
    assert_eq!(at(&report, "/dry_run"), false);
    assert_eq!(at(&report, "/laws"), &json!([]), "laws are off");
    let lint = layer(&report, "static-lint");
    assert_eq!(at(lint, "/verdict"), "pass");
    assert_eq!(
        at(lint, "/argv").as_array().map(Vec::len),
        Some(5),
        "{lint}"
    );
    // The pinned tools are asked first: the toolchain, then cargo-deny.
    assert_eq!(at(lint, "/argv/0"), &json!(["rustc", "--version"]));
    assert_eq!(at(lint, "/argv/1"), &json!(["cargo", "deny", "--version"]));
    assert_eq!(
        at(lint, "/argv/2"),
        &json!(["cargo", "fmt", "--all", "--", "--check"])
    );
    assert_eq!(
        at(lint, "/tool_version"),
        "rustfmt 1.10.0-stable (stub); clippy 0.1.99 (stub); cargo-deny 0.18.9"
    );
    assert!(at(&report, "/disabled")
        .as_array()
        .is_some_and(|d| !d.is_empty()));
}

#[test]
fn a_failed_command_exits_1_and_names_the_layer() {
    let fixture = fixture("fail");
    let (out, text) = gate(&fixture, &[], Some("clippy"));
    assert_eq!(out.status.code(), Some(1), "{text}");
    let report = report(&fixture);
    expect_consistent(&report, 1);
    assert_eq!(at(&report, "/kind"), "fail");
    assert_eq!(at(layer(&report, "static-lint"), "/verdict"), "fail");
    let problems = at(&report, "/problems").to_string();
    assert!(problems.contains("static-lint"), "{problems}");
}

#[test]
fn a_missing_tool_exits_3_and_the_layer_is_tool_missing() {
    let fixture = fixture("missing");
    fs::remove_file(fixture.bin.join("cargo-deny")).expect("remove stub");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(3), "{text}");
    let report = report(&fixture);
    expect_consistent(&report, 3);
    assert_eq!(at(&report, "/kind"), "environment");
    assert_eq!(
        at(layer(&report, "static-lint"), "/verdict"),
        "tool-missing"
    );
    assert!(
        at(&report, "/problems").to_string().contains("cargo-deny"),
        "{report}"
    );
}

#[test]
fn the_environment_outranks_a_failure_and_every_problem_is_listed() {
    let fixture = fixture("mixed");
    fs::remove_file(fixture.bin.join("cargo-deny")).expect("remove stub");
    let (out, text) = gate(&fixture, &[], Some("clippy"));
    assert_eq!(out.status.code(), Some(3), "{text}");
    let report = report(&fixture);
    expect_consistent(&report, 3);
    let problems = at(&report, "/problems").to_string();
    assert!(
        problems.contains("clippy"),
        "the failure is listed: {problems}"
    );
    assert!(
        problems.contains("cargo-deny"),
        "the missing tool is listed: {problems}"
    );
}

#[test]
fn a_dry_run_exits_1_and_writes_a_report_with_nothing_run() {
    let fixture = fixture("dry");
    let (out, text) = gate(&fixture, &["--dry-run"], None);
    assert_eq!(out.status.code(), Some(1), "{text}");
    let report = report(&fixture);
    expect_consistent(&report, 1);
    assert_eq!(at(&report, "/dry_run"), true);
    let lint = layer(&report, "static-lint");
    assert_eq!(at(lint, "/verdict"), "not-run");
    assert_eq!(
        at(lint, "/argv").as_array().map(Vec::len),
        Some(5),
        "{lint}"
    );
    assert_eq!(at(lint, "/seconds").as_f64(), Some(0.0));
    assert!(
        at(lint, "/tool_version").is_null(),
        "nothing was asked: {lint}"
    );
}

#[test]
fn an_invalid_config_exits_2_and_still_writes_a_report() {
    let fixture = fixture("config");
    fs::write(fixture.repo.join("rust-tops.yaml"), "crate: [unclosed\n").expect("write");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(2), "{text}");
    let report = report(&fixture);
    expect_consistent(&report, 2);
    assert_eq!(at(&report, "/kind"), "config-invalid");
    assert!(
        at(&report, "/error")
            .as_str()
            .is_some_and(|e| e.contains("rust-tops.yaml")),
        "{report}"
    );
    assert_eq!(at(&report, "/layers"), &json!([]));
}

#[test]
fn a_config_that_fails_check_exits_2() {
    let fixture = fixture("check");
    edit(&fixture, |doc| {
        set(
            doc.get_mut("protocol").expect("protocol"),
            "id",
            json!("other"),
        );
    });
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(2), "{text}");
    expect_consistent(&report(&fixture), 2);
}

#[test]
fn no_enabled_layer_in_range_exits_2() {
    let fixture = fixture("no-layers");
    edit(&fixture, |doc| {
        for layer in doc
            .pointer_mut("/layers")
            .and_then(Value::as_array_mut)
            .expect("layers")
        {
            set(layer, "enabled", Value::Bool(false));
        }
    });
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(2), "{text}");
    let report = report(&fixture);
    expect_consistent(&report, 2);
    assert!(
        at(&report, "/error")
            .as_str()
            .is_some_and(|e| e.contains("every class requires the security-advisory layer")),
        "{report}"
    );
}

#[test]
fn a_stale_report_is_replaced_by_the_next_run() {
    let fixture = fixture("stale");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert_eq!(at(&report(&fixture), "/ok"), &json!(true));
    let (out, text) = gate(&fixture, &[], Some("clippy"));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert_eq!(
        at(&report(&fixture), "/ok"),
        &json!(false),
        "the earlier pass is gone"
    );
    // A report that is not a report at all is replaced too.
    fs::write(report_path(&fixture), "{\"stale\": true}\n").expect("write");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(report(&fixture).get("stale").is_none());
}

#[test]
fn a_layer_above_the_max_tier_is_not_evaluated() {
    let fixture = fixture("above");
    edit(&fixture, |doc| {
        for layer in doc
            .pointer_mut("/layers")
            .and_then(Value::as_array_mut)
            .expect("layers")
        {
            if at(layer, "/id") == "formal-kani" {
                set(layer, "enabled", Value::Bool(true));
            }
        }
    });
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    let report = report(&fixture);
    let kani = layer(&report, "formal-kani");
    assert_eq!(at(kani, "/verdict"), "not-evaluated", "{kani}");
    assert_eq!(at(kani, "/argv"), &json!([]));
}

#[test]
fn help_names_the_four_exit_codes_and_the_report() {
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(["gate", "--help"])
        .output()
        .expect("spawn");
    let text = String::from_utf8_lossy(&out.stdout);
    for want in [
        "report.json",
        "0  pass",
        "1  the gate ran and failed",
        "2  the config is invalid",
        "3  the environment",
    ] {
        assert!(text.contains(want), "help lacks {want:?}: {text}");
    }
}

/// Turns `ids` on, in addition to what the fixture has on.
fn enable(fixture: &Fixture, ids: &[&str]) {
    edit(fixture, |doc| {
        let layers = doc.pointer_mut("/layers").and_then(Value::as_array_mut);
        for layer in layers.expect("layers") {
            if ids.iter().any(|id| at(layer, "/id") == *id) {
                set(layer, "enabled", Value::Bool(true));
            }
        }
    });
}

#[test]
fn the_report_names_the_head_commit_of_a_repo_and_nothing_otherwise() {
    let fixture = fixture("sha");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        report(&fixture).get("git_sha").is_none(),
        "the fixture is no repo"
    );

    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(&fixture.repo)
            .output()
            .expect("spawn git");
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    git(&["commit", "-q", "--allow-empty", "-m", "t"]);
    let head = git(&["rev-parse", "HEAD"]);
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert_eq!(at(&report(&fixture), "/git_sha"), &json!(head));

    // A git that prints something and fails names no commit.
    stub(&fixture.bin, "git", "#!/bin/sh\necho abc123\nexit 1\n");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(report(&fixture).get("git_sha").is_none());
}

#[test]
fn a_cargo_that_cannot_say_where_the_target_is_exits_3() {
    let fixture = fixture("metadata");
    let (out, text) = gate(&fixture, &[], Some("metadata"));
    assert_eq!(out.status.code(), Some(3), "{text}");
    assert!(text.contains("cargo metadata is unavailable"), "{text}");
}

#[test]
fn a_layer_at_the_max_tier_is_judged_and_the_one_above_is_not() {
    let fixture = fixture("at-max");
    enable(&fixture, &["integration"]);
    let (out, text) = gate(&fixture, &["--max-tier", "0"], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    let report = report(&fixture);
    assert_eq!(at(layer(&report, "static-lint"), "/verdict"), "pass");
    assert_eq!(
        at(layer(&report, "integration"), "/verdict"),
        "not-evaluated"
    );
}

#[test]
fn only_the_layer_with_no_command_carries_the_detail() {
    let fixture = fixture("unmapped");
    enable(&fixture, &["fuzz"]);
    let (out, text) = gate(&fixture, &["--max-tier", "3"], None);
    assert_eq!(out.status.code(), Some(1), "{text}");
    let report = report(&fixture);
    let coverage = layer(&report, "fuzz");
    assert_eq!(at(coverage, "/verdict"), "not-run", "{coverage}");
    assert_eq!(
        at(coverage, "/detail"),
        "enabled and no command is mapped",
        "{coverage}"
    );
    assert!(layer(&report, "static-lint").get("detail").is_none());
}

#[test]
fn a_dry_run_counts_the_mutation_commands_it_lists() {
    let fixture = fixture("dry-mutation");
    enable(&fixture, &["mutation"]);
    let (out, text) = gate(&fixture, &["--dry-run"], None);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("dry-run: 6 command(s) listed, 0 run"),
        "{text}"
    );
}

/// `gate`, with `CARGO_TARGET_DIR` set to a directory under the fixture.
fn gate_with_target_dir(fixture: &Fixture, fail: Option<&str>) -> (Output, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cargo-tops"));
    cmd.arg("gate")
        .arg("--path")
        .arg(&fixture.repo)
        .env("PATH", path(fixture))
        .env("CARGO_TARGET_DIR", fixture.root.join("elsewhere"))
        .env_remove("CARGO_STUB_FAIL");
    if let Some(pattern) = fail {
        cmd.env("CARGO_STUB_FAIL", pattern);
    }
    let out = cmd.output().expect("spawn cargo-tops gate");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out, text)
}

#[test]
fn a_broken_manifest_exits_3_whether_or_not_the_target_dir_is_set() {
    let fixture = fixture("manifest-env");
    let (out, text) = gate_with_target_dir(&fixture, Some("metadata"));
    assert_eq!(out.status.code(), Some(3), "{text}");
    assert!(text.contains("cargo metadata is unavailable"), "{text}");
    // The same tree with the variable set and a cargo that answers passes.
    let (out, text) = gate_with_target_dir(&fixture, None);
    assert_eq!(out.status.code(), Some(0), "{text}");
}

#[test]
fn a_directory_with_no_manifest_is_an_unavailable_cargo_metadata() {
    let fixture = fixture("no-manifest");
    fs::remove_file(fixture.repo.join("Cargo.toml")).expect("remove the manifest");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(3), "{text}");
    assert!(text.contains("cargo metadata is unavailable"), "{text}");
    let report = report(&fixture);
    expect_consistent(&report, 3);
    assert_eq!(at(&report, "/kind"), "environment");
}

#[test]
fn a_tool_version_is_asked_from_the_crate_directory() {
    let fixture = fixture("version-dir");
    // No pins: the check asks nothing, so only the report's own ask is here.
    fs::remove_file(fixture.repo.join("rust-tops.lock")).expect("init wrote a lock");
    stub(
        &fixture.bin,
        "cargo",
        "#!/bin/sh\ncase \"$*\" in *--version) echo \"asked in $(basename \"$PWD\")\"; exit 0 ;; esac\nexit 0\n",
    );
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    let report = report(&fixture);
    assert_eq!(
        at(layer(&report, "static-lint"), "/tool_version"),
        "asked in repo"
    );
}

#[test]
fn a_missing_mutation_tool_has_no_version_and_a_layer_that_never_ran_took_zero_seconds() {
    let fixture = fixture("mutation-missing");
    enable(&fixture, &["mutation"]);
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(3), "{text}");
    let report = report(&fixture);
    let mutation = layer(&report, "mutation");
    assert_eq!(at(mutation, "/verdict"), "tool-missing", "{mutation}");
    assert!(at(mutation, "/tool_version").is_null(), "{mutation}");

    let (_, _) = gate(&fixture, &["--dry-run"], None);
    let raw = fs::read_to_string(report_path(&fixture)).expect("report");
    assert!(!raw.contains("-0.0"), "{raw}");
}

#[test]
fn the_law_total_is_left_out_when_no_judgement_counted_it() {
    let fixture = fixture("laws-total");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    // The pack is counted even with the laws off.
    assert!(at(&report(&fixture), "/laws_total").is_u64());
    fs::write(fixture.repo.join("rust-tops.yaml"), "crate: [unclosed\n").expect("write");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(report(&fixture).get("laws_total").is_none());
}

/// The cargo stub, except that `cargo <sub> --version` answers `answer`
/// (`exit 101` with no output when `answer` is `None`), as the tool would.
fn answer_version(fixture: &Fixture, sub: &str, answer: Option<&str>) {
    let reply = answer.map_or_else(
        || "exit 101".to_string(),
        |line| format!("echo '{line}'; exit 0"),
    );
    let log = "if [ -n \"${CARGO_STUB_LOG:-}\" ]; then echo \"$*\" >> \"$CARGO_STUB_LOG\"; fi\n";
    let body = CARGO_STUB.replacen(
        log,
        &format!("{log}if [ \"$1 $2\" = \"{sub} --version\" ]; then {reply}; fi\n"),
        1,
    );
    stub(&fixture.bin, "cargo", &body);
}

fn problems(report: &Value) -> Vec<String> {
    at(report, "/problems")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

#[test]
fn a_pinned_tool_at_another_version_is_a_tool_mismatch_worded_once() {
    let fixture = fixture("mismatch");
    answer_version(&fixture, "deny", Some("cargo-deny 0.18.8"));
    let (out, text) = gate(&fixture, &[], None);
    let said = "mismatch cargo-deny: pin 0.18.9, found 0.18.8";
    assert_eq!(out.status.code(), Some(3), "{text}");
    let report = report(&fixture);
    expect_consistent(&report, 3);
    // The same words on stdout, in the layer's detail and in the problems.
    assert!(
        text.contains(&format!("tool-mismatch [static-lint]: {said}")),
        "{text}"
    );
    assert_eq!(
        at(layer(&report, "static-lint"), "/verdict"),
        "tool-mismatch"
    );
    assert_eq!(at(layer(&report, "static-lint"), "/detail"), said);
    assert!(
        problems(&report).contains(&format!("static-lint: {said}")),
        "{report}"
    );
    assert_eq!(
        at(layer(&report, "security-advisory"), "/verdict"),
        "tool-mismatch"
    );
    // Nothing of the layer ran: no fmt, no clippy, no deny check.
    let log = cargo_log(&fixture);
    assert!(
        !log.lines()
            .filter(|line| !line.ends_with("--version"))
            .any(|line| line.starts_with("fmt ") || line.starts_with("clippy ")),
        "{log}"
    );
}

#[test]
fn the_toolchain_pin_judges_the_layers_that_need_rustc() {
    let fixture = fixture("rustc-mismatch");
    stub(
        &fixture.bin,
        "rustc",
        "#!/bin/sh\necho 'rustc 1.98.0 (stub 2025-01-01)'\n",
    );
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(3), "{text}");
    let report = report(&fixture);
    assert_eq!(
        at(layer(&report, "static-lint"), "/verdict"),
        "tool-mismatch"
    );
    assert_eq!(
        at(layer(&report, "static-lint"), "/detail"),
        "mismatch rustc: pin 1.99.1, found 1.98.0; mismatch clippy: pin 1.99.1, found 1.98.0; mismatch rustfmt: pin 1.99.1, found 1.98.0"
    );
}

#[test]
fn clippy_that_will_not_run_is_a_missing_tool_not_a_version_miss() {
    let fixture = fixture("no-clippy");
    answer_version(&fixture, "clippy", None);
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(3), "{text}");
    // The correction: a probe that failed names the tool and the command it ran
    // (it used to say `cargo` was not on PATH), once, on stdout, in the
    // layer's detail and in the problems.
    let said = "missing  clippy: pin 1.99.1, `cargo clippy --version` exited 101";
    assert!(
        text.contains(&format!("tool-missing [static-lint]: {said}")),
        "{text}"
    );
    assert!(text.contains(&format!("static-lint: {said}")), "{text}");
    let report = report(&fixture);
    assert_eq!(at(layer(&report, "static-lint"), "/detail"), said);
    assert!(
        problems(&report).contains(&format!("static-lint: {said}")),
        "{report}"
    );
    assert_eq!(
        at(layer(&report, "static-lint"), "/verdict"),
        "tool-missing"
    );
}

#[test]
fn a_tool_only_a_layer_above_the_range_needs_is_not_asked() {
    let fixture = fixture("above-range");
    // `property` is tier 1 and needs nextest; at --max-tier 0 it is not in range.
    answer_version(&fixture, "nextest", Some("cargo-nextest 0.0.1"));
    let (out, text) = gate(&fixture, &["--max-tier", "0"], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    let log = cargo_log(&fixture);
    assert!(
        !log.lines().any(|line| line.starts_with("nextest ")),
        "{log}"
    );
    let (out, text) = gate(&fixture, &["--max-tier", "1"], None);
    assert_eq!(out.status.code(), Some(3), "{text}");
    assert!(
        text.contains("mismatch cargo-nextest: pin 0.9.145, found 0.0.1"),
        "{text}"
    );
}

#[test]
fn a_dry_run_names_the_pins_and_asks_nothing() {
    let fixture = fixture("dry-pins");
    answer_version(&fixture, "deny", Some("cargo-deny 0.0.1"));
    let (out, text) = gate(&fixture, &["--dry-run"], None);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("[static-lint] rust-tops.lock pins, asked first: rustc 1.99.1, clippy 1.99.1, rustfmt 1.99.1, cargo-deny 0.18.9"),
        "{text}"
    );
    assert!(!text.contains("tool-mismatch"), "{text}");
}

#[test]
fn a_layer_that_takes_another_layers_run_keeps_that_runs_mismatch() {
    let fixture = fixture("takes-mismatch");
    // `property` runs itself here, so turn coverage on: it takes coverage's run.
    edit(&fixture, |doc| {
        let layers = doc.pointer_mut("/layers").and_then(Value::as_array_mut);
        for layer in layers.expect("layers") {
            let on = stays_on(layer) || layer.get("id").and_then(Value::as_str) == Some("coverage");
            set(layer, "enabled", Value::Bool(on));
        }
    });
    answer_version(&fixture, "llvm-cov", Some("cargo-llvm-cov 0.8.0"));
    let (out, text) = gate(&fixture, &[], None);
    let said = "mismatch cargo-llvm-cov: pin 0.9.1, found 0.8.0";
    assert_eq!(out.status.code(), Some(3), "{text}");
    let report = report(&fixture);
    assert_eq!(at(layer(&report, "coverage"), "/verdict"), "tool-mismatch");
    assert_eq!(at(layer(&report, "property"), "/verdict"), "tool-mismatch");
    assert_eq!(at(layer(&report, "property"), "/detail"), said);
}

#[test]
fn a_pre_release_of_a_pinned_tool_is_a_tool_mismatch_not_a_pass() {
    let fixture = fixture("pre-release");
    answer_version(&fixture, "deny", Some("cargo-deny 0.18.9-rc.1"));
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(3), "{text}");
    assert!(
        text.contains("mismatch cargo-deny: pin 0.18.9, found 0.18.9-rc.1"),
        "{text}"
    );
}

#[test]
fn a_pin_that_is_not_a_string_stops_the_gate_with_exit_2_naming_the_key() {
    let fixture = fixture("lock-not-a-string");
    fs::write(
        fixture.repo.join("rust-tops.lock"),
        "[tools]\ncargo-deny = 0.18\n",
    )
    .expect("write lock");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(text.contains("cargo-deny"), "{text}");
    assert!(text.contains("found float"), "{text}");
    let log = cargo_log(&fixture);
    assert!(!log.lines().any(|line| line.starts_with("deny ")), "{log}");
}

#[test]
fn a_lock_with_no_tools_table_stops_the_gate_with_exit_2() {
    for header in ["[tool]", "[Tools]"] {
        let fixture = fixture("lock-no-tools");
        fs::write(
            fixture.repo.join("rust-tops.lock"),
            format!("{header}\ncargo-deny = \"0.18.9\"\n"),
        )
        .expect("write lock");
        let (out, text) = gate(&fixture, &[], None);
        assert_eq!(out.status.code(), Some(2), "{header}: {text}");
        assert!(text.contains("has no [tools] table"), "{header}: {text}");
        let log = cargo_log(&fixture);
        assert!(!log.lines().any(|line| line.starts_with("deny ")), "{log}");
    }
}

#[test]
fn an_unknown_key_is_a_warning_and_the_tool_it_was_meant_for_is_named() {
    let fixture = fixture("unknown-key");
    let pins = "[tools]\nrustc = \"1.99.1\"\nclippy = \"1.99.1\"\nrustfmt = \"1.99.1\"\ncargo-deny = \"0.18.9\"\ncargo-nextest = \"0.9.145\"\n";
    let lock = fixture.repo.join("rust-tops.lock");
    fs::write(&lock, format!("{pins}cargo-nexttest = \"0.9.145\"\n")).expect("write lock");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains(
            "warning: rust-tops.lock key cargo-nexttest is not a tool rust-tops knows, not checked"
        ),
        "{text}"
    );
    // The misspelt key stands where the right one is missing: both are named.
    let without = pins.replace("cargo-nextest = \"0.9.145\"\n", "");
    fs::write(&lock, format!("{without}cargo-nexttest = \"0.9.145\"\n")).expect("write lock");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains(
            "warning: rust-tops.lock has no pin for cargo-nextest, and holds cargo-nexttest"
        ),
        "{text}"
    );
}

fn cargo_log(fixture: &Fixture) -> String {
    fs::read_to_string(fixture.root.join("cargo.log")).unwrap_or_default()
}
