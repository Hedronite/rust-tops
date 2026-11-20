//! Every exemption record is checked, whatever its layer, whether that layer
//! is on or off, and whatever `--max-tier` is: one that has expired, or whose
//! date is not a real day, is a gate problem and the gate exits 1. A date
//! passing does not make the config invalid, so it is never exit 2. `check`
//! prints the same lines as warnings and still exits 0.
//!
//! Cargo and its plugins are stubs. Every report is validated against the
//! schema.
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

const REPORT_SCHEMA: &str = include_str!("../../../schema/gate-report.schema.json");

/// Logs its argv when `$CARGO_STUB_LOG` is set; answers every `--version`;
/// passes everything else.
const CARGO_STUB: &str = r#"#!/bin/sh
if [ -n "${CARGO_STUB_LOG:-}" ]; then echo "$*" >> "$CARGO_STUB_LOG"; fi
if [ "$1 $2" = "crap --version" ]; then echo "cargo-crap 0.9.0"; exit 0; fi
case "$*" in *--version) echo "stub-tool 1.2.3"; exit 0 ;; esac
exit 0
"#;

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    bin: PathBuf,
    home: PathBuf,
}

fn at<'v>(value: &'v Value, pointer: &str) -> &'v Value {
    static NULL: Value = Value::Null;
    value.pointer(pointer).unwrap_or(&NULL)
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

/// A library-core instance named `fixture`, in a directory of its own with
/// a `Cargo.toml`, one source file and stub tools, not yet a git repository.
fn scaffold(name: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!("cargo-tops-base-{name}-{}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    let fixture = Fixture {
        repo: root.join("repo"),
        bin: root.join("bin"),
        home: root.join("home"),
        root,
    };
    fs::create_dir_all(&fixture.bin).expect("mkdir bin");
    fs::create_dir_all(&fixture.home).expect("mkdir home");
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
    fs::create_dir_all(fixture.repo.join("src")).expect("mkdir src");
    fs::write(fixture.repo.join("src/a.rs"), "pub fn a() {}\n").expect("write a.rs");
    fs::write(fixture.repo.join(".gitignore"), "/target\n").expect("write .gitignore");
    stub(&fixture.bin, "cargo", CARGO_STUB);
    for tool in ["cargo-deny", "cargo-nextest", "cargo-mutants"] {
        stub(&fixture.bin, tool, "#!/bin/sh\nexit 0\n");
    }
    fixture
}

/// The scaffold with `static-lint`, `security-advisory`, `property` and
/// `mutation` on, in a git repository whose one branch is `branch` and whose
/// one commit is everything `init` wrote. There is no remote.
/// A commit that adds `file` under `src/`.
/// What a clone leaves: a remote `origin` holding `branch`, with
/// `refs/remotes/origin/HEAD` pointing at it.
/// `refs/remotes/origin/<name>` at `commit`.
fn gate(fixture: &Fixture, args: &[&str], ci: Option<&str>) -> (Output, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cargo-tops"));
    cmd.arg("gate")
        .args(args)
        .arg("--path")
        .arg(&fixture.repo)
        .env(
            "PATH",
            std::env::join_paths([
                fixture.bin.as_path(),
                Path::new("/usr/bin"),
                Path::new("/bin"),
            ])
            .expect("PATH"),
        )
        .env("HOME", &fixture.home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("CARGO_STUB_LOG", fixture.root.join("cargo.log"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CI");
    if let Some(value) = ci {
        cmd.env("CI", value);
    }
    let out = cmd.output().expect("spawn cargo-tops gate");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out, text)
}

fn report(fixture: &Fixture) -> Value {
    let raw = fs::read_to_string(fixture.repo.join("target/rust-tops/report.json"))
        .expect("report.json was written");
    let doc: Value = serde_json::from_str(&raw).expect("report.json is JSON");
    let schema: Value = serde_json::from_str(REPORT_SCHEMA).expect("schema is JSON");
    let errors = schema::validate(&schema, &doc);
    assert!(
        errors.is_empty(),
        "report.json breaks the schema: {errors:?}\n{raw}"
    );
    doc
}

/// The scaffold with `static-lint`, `security-advisory` and `property` on.
fn fixture(name: &str) -> Fixture {
    let fixture = scaffold(name);
    edit(&fixture, |doc| {
        let layers = doc.pointer_mut("/layers").and_then(Value::as_array_mut);
        for layer in layers.expect("layers") {
            let id = layer.get("id").and_then(Value::as_str).unwrap_or_default();
            let on = matches!(id, "static-lint" | "security-advisory" | "property");
            set(layer, "enabled", Value::Bool(on));
        }
    });
    fixture
}

/// One record for `layer` over `src/**`, expiring on `expires`.
fn exempt(fixture: &Fixture, layer: &str, expires: &str) {
    edit(fixture, |doc| {
        let record = json!({
            "path": "src/**",
            "layer": layer,
            "owner": "fixture",
            "reason": "planted for the expiry rule",
            "expires": expires,
        });
        set(doc, "exemptions", json!([record]));
    });
}

fn check(fixture: &Fixture) -> (Output, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(["check", "--path"])
        .arg(&fixture.repo)
        .output()
        .expect("spawn check");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out, text)
}

/// The report's problems that mention an exemption.
fn exemption_problems(report: &Value) -> Vec<String> {
    at(report, "/problems")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|problem| problem.contains("exemption"))
        .map(str::to_string)
        .collect()
}

#[test]
fn an_expired_record_for_a_layer_that_is_off_fails_the_gate() {
    // `miri` is off in this instance and above `--max-tier 1` besides.
    let fixture = fixture("expired-miri");
    exempt(&fixture, "miri", "2020-01-01");
    let (out, text) = gate(&fixture, &["--max-tier", "1"], None);
    assert_eq!(out.status.code(), Some(1), "{text}");
    let report = report(&fixture);
    assert_eq!(at(&report, "/exit_code"), 1, "{report}");
    assert_eq!(at(&report, "/kind"), "fail", "{report}");
    let problems = exemption_problems(&report);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems.iter().all(|p| p.starts_with("miri:")
            && p.contains("src/**")
            && p.contains("expired on 2020-01-01")),
        "{problems:?}"
    );
    assert!(text.contains("expired on 2020-01-01"), "{text}");
}

#[test]
fn an_expired_record_fails_for_every_layer_and_every_state_of_it() {
    // On and in range, on and above the range, and off.
    for layer in [
        "static-lint",
        "security-advisory",
        "miri",
        "fuzz",
        "mutation",
        "coverage",
        "crap",
        "acceptance",
    ] {
        for tier in ["0", "1"] {
            let fixture = fixture("expired-any");
            exempt(&fixture, layer, "2020-01-01");
            let (out, text) = gate(&fixture, &["--max-tier", tier], None);
            assert_eq!(out.status.code(), Some(1), "{layer} at {tier}: {text}");
            let problems = exemption_problems(&report(&fixture));
            assert_eq!(problems.len(), 1, "{layer} at {tier}: {problems:?}");
        }
    }
}

#[test]
fn a_record_whose_date_is_not_a_day_fails() {
    for date in ["2027-02-30", "not-a-date", "2024-13-01", "2099-1-1"] {
        let fixture = fixture("not-a-day");
        exempt(&fixture, "miri", date);
        let (out, text) = gate(&fixture, &["--max-tier", "1"], None);
        assert_eq!(out.status.code(), Some(1), "{date}: {text}");
        let problems = exemption_problems(&report(&fixture));
        assert!(
            problems
                .iter()
                .all(|p| p.contains(date) && p.contains("not a day")),
            "{date}: {problems:?}"
        );
        assert_eq!(problems.len(), 1, "{date}: {problems:?}");
    }
}

#[test]
fn a_live_record_passes() {
    let fixture = fixture("live");
    for layer in ["miri", "mutation", "static-lint"] {
        exempt(&fixture, layer, "2099-01-01");
        let (out, text) = gate(&fixture, &["--max-tier", "1"], None);
        assert_eq!(out.status.code(), Some(0), "{layer}: {text}");
        assert!(exemption_problems(&report(&fixture)).is_empty(), "{layer}");
    }
}

#[test]
fn an_expired_record_is_not_a_config_error() {
    // Exit 1 with a report of kind `fail`, never 2.
    let fixture = fixture("not-config");
    exempt(&fixture, "miri", "2020-01-01");
    let (out, _) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(at(&report(&fixture), "/kind"), "fail");
}

#[test]
fn every_expired_record_is_named_not_only_the_first() {
    let fixture = fixture("two-expired");
    edit(&fixture, |doc| {
        let record = |layer: &str, expires: &str| {
            json!({
                "path": "src/**", "layer": layer, "owner": "fixture",
                "reason": "planted for this test", "expires": expires,
            })
        };
        let records = json!([
            record("miri", "2020-01-01"),
            record("fuzz", "2021-06-30"),
            record("crap", "2099-01-01"),
        ]);
        set(doc, "exemptions", records);
    });
    let (out, text) = gate(&fixture, &["--max-tier", "1"], None);
    assert_eq!(out.status.code(), Some(1), "{text}");
    let problems = exemption_problems(&report(&fixture));
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems.iter().any(|p| p.starts_with("miri:")),
        "{problems:?}"
    );
    assert!(
        problems.iter().any(|p| p.starts_with("fuzz:")),
        "{problems:?}"
    );
}

#[test]
fn check_warns_about_an_expired_record_and_still_exits_0() {
    let fixture = fixture("check-warns");
    exempt(&fixture, "miri", "2020-01-01");
    let (out, text) = check(&fixture);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("expired on 2020-01-01"), "{text}");
    assert!(text.to_lowercase().contains("warn"), "{text}");
    // The same file, with a live record, has no warning.
    exempt(&fixture, "miri", "2099-01-01");
    let (out, text) = check(&fixture);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(!text.contains("expired"), "{text}");
}

#[test]
fn a_not_a_day_date_is_a_check_warning_too() {
    let fixture = fixture("check-not-a-day");
    exempt(&fixture, "miri", "2027-02-30");
    let (out, text) = check(&fixture);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("2027-02-30"), "{text}");
}
