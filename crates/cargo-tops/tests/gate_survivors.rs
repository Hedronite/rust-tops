//! The survivors ledger. After `cargo mutants` runs, the gate reads
//! `mutants.out/outcomes.json` (pinned to cargo-mutants 27.1.0) and judges
//! the missed mutants: one that a live `layer: mutation` exemption covers
//! (same rule as coverage and crap) is explained, the rest are not, and the
//! layer passes when the unexplained count is at most
//! `gates.mutation.unexplained_survivors_max`. Timeouts and unviable mutants
//! are counted, not survivors. A file that is missing, unreadable or of an
//! unknown shape fails the layer.
//!
//! `cargo` is a stub that, for `mutants`, copies a planted `outcomes.json`
//! into `mutants.out/` and exits as cargo-mutants does (2 when mutants were
//! missed, 3 when some timed out). The planted files come from the fixture
//! recorded from a real 27.1.0 run. Every report is validated against the
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
if [ "$1" = "mutants" ]; then
  if [ -n "${MUTANTS_OUTCOMES:-}" ]; then
    mkdir -p mutants.out && cp "$MUTANTS_OUTCOMES" mutants.out/outcomes.json
  fi
  exit "${MUTANTS_EXIT:-0}"
fi
exit 0
"#;

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    bin: PathBuf,
    home: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
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

fn git(fixture: &Fixture, dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("HOME", &fixture.home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
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
fn fixture(name: &str, branch: &str) -> Fixture {
    let fixture = scaffold(name);
    edit(&fixture, |doc| {
        let layers = doc.pointer_mut("/layers").and_then(Value::as_array_mut);
        for layer in layers.expect("layers") {
            let id = layer.get("id").and_then(Value::as_str).unwrap_or_default();
            let on = matches!(
                id,
                "static-lint" | "security-advisory" | "property" | "mutation"
            );
            set(layer, "enabled", Value::Bool(on));
        }
    });
    git(&fixture, &fixture.repo, &["init", "-q", "-b", branch]);
    git(&fixture, &fixture.repo, &["add", "-A"]);
    git(&fixture, &fixture.repo, &["commit", "-q", "-m", "adopt"]);
    fixture
}

/// A commit that adds `file` under `src/`.
fn commit_file(fixture: &Fixture, file: &str) -> String {
    fs::write(fixture.repo.join("src").join(file), format!("// {file}\n")).expect("write");
    git(fixture, &fixture.repo, &["add", "-A"]);
    git(fixture, &fixture.repo, &["commit", "-q", "-m", file]);
    git(fixture, &fixture.repo, &["rev-parse", "HEAD"])
}

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

fn mutation(report: &Value) -> &Value {
    at(report, "/layers")
        .as_array()
        .and_then(|layers| layers.iter().find(|row| at(row, "/id") == "mutation"))
        .unwrap_or_else(|| at(report, "/missing"))
}

/// The entries of `problems` that start with `mutation:`.
fn mutation_problems(report: &Value) -> Vec<String> {
    at(report, "/problems")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|problem| problem.starts_with("mutation:"))
        .map(str::to_string)
        .collect()
}

/// The real 27.1.0 run: 11 caught, 10 missed (5 in `double`, 5 in
/// `inner::triple`), 1 timeout, 1 unviable.
const RECORDED: &str = include_str!("fixtures/mutants-27.1.0/outcomes.json");

/// The recorded run with `change` applied, planted under the fixture.
fn planted(fixture: &Fixture, change: impl FnOnce(&mut Value)) -> PathBuf {
    let mut doc: Value = serde_json::from_str(RECORDED).expect("fixture is JSON");
    change(&mut doc);
    let path = fixture.root.join("outcomes.json");
    fs::write(&path, doc.to_string()).expect("plant outcomes.json");
    path
}

type Edit = fn(&mut Value);

fn outcomes_mut(doc: &mut Value) -> &mut Vec<Value> {
    doc.get_mut("outcomes")
        .and_then(Value::as_array_mut)
        .expect("outcomes")
}

/// Keep only the missed mutants for which `keep(function)` holds, and
/// correct the totals.
fn keep_missed(doc: &mut Value, keep: impl Fn(&str) -> bool) {
    outcomes_mut(doc).retain(|outcome| {
        let function = at(outcome, "/scenario/Mutant/function/function_name")
            .as_str()
            .unwrap_or_default();
        at(outcome, "/summary") != "MissedMutant" || keep(function)
    });
    let missed = outcomes_mut(doc)
        .iter()
        .filter(|outcome| at(outcome, "/summary") == "MissedMutant")
        .count();
    set(doc, "missed", json!(missed));
}

/// One survivor in `double` and one in `inner::triple`.
fn one_each(doc: &mut Value) {
    let mut seen = Vec::new();
    outcomes_mut(doc).retain(|outcome| {
        let function = at(outcome, "/scenario/Mutant/function/function_name")
            .as_str()
            .unwrap_or_default()
            .to_string();
        if at(outcome, "/summary") != "MissedMutant" {
            return true;
        }
        let first = !seen.contains(&function);
        seen.push(function);
        first
    });
    set(doc, "missed", json!(2));
}

/// Run the gate with `outcomes` planted, the way cargo-mutants exits for it.
fn gate_with(fixture: &Fixture, outcomes: Option<&Path>, exit: &str) -> (Output, String) {
    git(fixture, &fixture.repo, &["checkout", "-q", "-b", "feature"]);
    commit_file(fixture, "b.rs");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cargo-tops"));
    cmd.args(["gate", "--base", "HEAD~1", "--path"])
        .arg(&fixture.repo)
        .env("PATH", stub_path(fixture))
        .env("HOME", &fixture.home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("MUTANTS_EXIT", exit)
        .env_remove("MUTANTS_OUTCOMES")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CI");
    if let Some(path) = outcomes {
        cmd.env("MUTANTS_OUTCOMES", path);
    }
    let out = cmd.output().expect("spawn cargo-tops gate");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out, text)
}

fn stub_path(fixture: &Fixture) -> std::ffi::OsString {
    std::env::join_paths([
        fixture.bin.as_path(),
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .expect("PATH")
}

/// A live `layer: mutation` record for `path`, and `symbol` when given.
fn exempt(fixture: &Fixture, path: &str, symbol: Option<&str>, expires: &str) {
    edit(fixture, |doc| {
        let mut record = json!({
            "path": path,
            "layer": "mutation",
            "owner": "fixture",
            "reason": "a planted survivor",
            "expires": expires,
        });
        if let Some(symbol) = symbol {
            set(&mut record, "symbol", json!(symbol));
        }
        set(doc, "exemptions", json!([record]));
    });
}

fn set_max(fixture: &Fixture, max: u64) {
    edit(fixture, |doc| {
        let mutation = doc.pointer_mut("/gates/mutation").expect("gates.mutation");
        set(mutation, "unexplained_survivors_max", json!(max));
    });
}

/// The row's `judged` numbers, in the order the SPEC names them.
fn counts(row: &Value) -> [u64; 6] {
    [
        "caught",
        "missed",
        "explained",
        "unexplained",
        "timeouts",
        "unviable",
    ]
    .map(|key| {
        at(row, &format!("/judged/{key}"))
            .as_u64()
            .unwrap_or(u64::MAX)
    })
}

fn expect_failed_naming(text: &str, report: &Value, named: &str, not_named: &str) {
    let problems = mutation_problems(report);
    assert_eq!(problems.len(), 1, "{problems:?}\n{text}");
    assert!(problems.iter().all(|p| p.contains(named)), "{problems:?}");
    assert!(
        problems.iter().all(|p| !p.contains(not_named)),
        "{problems:?}"
    );
}

#[test]
fn one_explained_and_one_unexplained_survivor_fails_naming_only_the_second() {
    let fixture = fixture("one-each", "main");
    exempt(&fixture, "src/**", Some("double"), "2099-01-01");
    let file = planted(&fixture, one_each);
    let (out, text) = gate_with(&fixture, Some(&file), "2");
    assert_eq!(out.status.code(), Some(1), "{text}");
    let report = report(&fixture);
    let row = mutation(&report);
    assert_eq!(at(row, "/verdict"), "fail", "{row}");
    expect_failed_naming(&text, &report, "inner::triple", "double");
    // caught 11, missed 2 (1 explained, 1 not), 1 timeout, 1 unviable.
    assert_eq!(counts(row), [11, 2, 1, 1, 1, 1], "{row}");
    assert_eq!(at(row, "/judged/unexplained_survivors_max"), 0, "{row}");
}

#[test]
fn survivors_that_every_record_explains_pass() {
    let fixture = fixture("both-explained", "main");
    edit(&fixture, |doc| {
        let record = |symbol: &str| {
            json!({
                "path": "src/lib.rs",
                "symbol": symbol,
                "layer": "mutation",
                "owner": "fixture",
                "reason": "planted for this test",
                "expires": "2099-01-01",
            })
        };
        set(
            doc,
            "exemptions",
            json!([record("double"), record("inner::triple")]),
        );
    });
    let file = planted(&fixture, |_| {});
    let (out, text) = gate_with(&fixture, Some(&file), "2");
    assert_eq!(out.status.code(), Some(0), "{text}");
    let report = report(&fixture);
    let row = mutation(&report);
    assert_eq!(at(row, "/verdict"), "pass", "{row}");
    assert_eq!(counts(row), [11, 10, 10, 0, 1, 1], "{row}");
    assert!(mutation_problems(&report).is_empty());
}

#[test]
fn a_record_with_no_symbol_covers_every_function_in_its_glob() {
    let fixture = fixture("no-symbol", "main");
    exempt(&fixture, "src/lib.rs", None, "2099-01-01");
    let file = planted(&fixture, |_| {});
    let (out, text) = gate_with(&fixture, Some(&file), "2");
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert_eq!(counts(mutation(&report(&fixture))), [11, 10, 10, 0, 1, 1]);
}

#[test]
fn a_record_for_another_file_symbol_or_layer_explains_nothing() {
    for (path, symbol, layer) in [
        ("src/other.rs", Some("double"), "mutation"),
        ("src/lib.rs", Some("nothing"), "mutation"),
        ("src/lib.rs", Some("double"), "crap"),
    ] {
        let fixture = fixture("explains-nothing", "main");
        edit(&fixture, |doc| {
            let mut record = json!({
                "path": path, "layer": layer, "owner": "fixture",
                "reason": "planted for this test", "expires": "2099-01-01",
            });
            set(&mut record, "symbol", json!(symbol));
            set(doc, "exemptions", json!([record]));
        });
        let file = planted(&fixture, |doc| keep_missed(doc, |f| f == "double"));
        let (out, text) = gate_with(&fixture, Some(&file), "2");
        assert_eq!(
            out.status.code(),
            Some(1),
            "{path} {symbol:?} {layer}: {text}"
        );
        let report = report(&fixture);
        assert_eq!(counts(mutation(&report))[2], 0, "{path} {symbol:?} {layer}");
    }
}

#[test]
fn an_expired_mutation_record_explains_nothing() {
    let fixture = fixture("expired-record", "main");
    exempt(&fixture, "src/lib.rs", None, "2020-01-01");
    let file = planted(&fixture, |_| {});
    let (out, text) = gate_with(&fixture, Some(&file), "2");
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert_eq!(counts(mutation(&report(&fixture)))[2], 0);
}

#[test]
fn a_maximum_of_one_passes_one_unexplained_survivor_and_fails_two() {
    // A crate-wide record lifts the class floor for the max in `check` and,
    // naming no file a mutant is in, explains nothing.
    let first = fixture("max-one", "main");
    set_max(&first, 1);
    exempt(&first, ".", None, "2099-01-01");
    let one = planted(&first, |doc| {
        keep_missed(doc, |f| f == "double");
        outcomes_mut(doc).retain(|o| {
            at(o, "/summary") != "MissedMutant" || at(o, "/scenario/Mutant/replacement") == "0"
        });
        set(doc, "missed", json!(1));
    });
    let (out, text) = gate_with(&first, Some(&one), "2");
    assert_eq!(out.status.code(), Some(0), "{text}");
    let report = report(&first);
    let row = mutation(&report);
    assert_eq!(counts(row), [11, 1, 0, 1, 1, 1], "{row}");
    assert_eq!(at(row, "/judged/unexplained_survivors_max"), 1);

    let second = fixture("max-one-two", "main");
    set_max(&second, 1);
    exempt(&second, ".", None, "2099-01-01");
    let two = planted(&second, one_each);
    let (out, text) = gate_with(&second, Some(&two), "2");
    assert_eq!(out.status.code(), Some(1), "{text}");
}

#[test]
fn timeouts_and_unviable_mutants_are_counted_and_are_not_survivors() {
    let fixture = fixture("not-survivors", "main");
    let file = planted(&fixture, |doc| keep_missed(doc, |_| false));
    let (out, text) = gate_with(&fixture, Some(&file), "3");
    assert_eq!(out.status.code(), Some(0), "{text}");
    let report = report(&fixture);
    let row = mutation(&report);
    assert_eq!(at(row, "/verdict"), "pass", "{row}");
    assert_eq!(counts(row), [11, 0, 0, 0, 1, 1], "{row}");
}

#[test]
fn an_unknown_shape_fails_with_a_message_and_never_passes() {
    let cases: [(&str, Edit); 3] = [
        ("another release", |d| {
            set(d, "cargo_mutants_version", json!("99.0.0"))
        }),
        ("an unknown outcome", |d| {
            if let Some(first) = outcomes_mut(d).get_mut(1) {
                set(first, "summary", json!("Skipped"));
            }
        }),
        ("totals that disagree", |d| set(d, "missed", json!(0))),
    ];
    for (what, change) in cases {
        let fixture = fixture("unknown-shape", "main");
        let file = planted(&fixture, change);
        let (out, text) = gate_with(&fixture, Some(&file), "0");
        assert_eq!(out.status.code(), Some(1), "{what}: {text}");
        let report = report(&fixture);
        assert_eq!(at(mutation(&report), "/verdict"), "fail", "{what}");
        let problems = mutation_problems(&report);
        assert!(
            problems.iter().all(|p| p.contains("outcomes.json")),
            "{what}: {problems:?}"
        );
        assert_eq!(problems.len(), 1, "{what}: {problems:?}");
    }
}

#[test]
fn a_file_that_is_not_json_fails() {
    let fixture = fixture("not-json", "main");
    let path = fixture.root.join("garbage.json");
    fs::write(&path, "not json").expect("plant");
    let (out, text) = gate_with(&fixture, Some(&path), "0");
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("outcomes.json"), "{text}");
}

#[test]
fn no_outcomes_file_is_a_failure_and_a_stale_one_is_never_read() {
    let fixture = fixture("no-outcomes", "main");
    // An earlier run left a clean file behind. This run wrote none.
    let stale = fixture.repo.join("mutants.out");
    fs::create_dir_all(&stale).expect("mkdir");
    let clean = planted(&fixture, |doc| keep_missed(doc, |_| false));
    fs::copy(&clean, stale.join("outcomes.json")).expect("plant stale outcomes");
    let (out, text) = gate_with(&fixture, None, "0");
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("outcomes.json"), "{text}");
}

#[test]
fn a_dry_run_reads_no_outcomes() {
    let fixture = fixture("no-run", "main");
    let (out, text) = gate(&fixture, &["--dry-run"], None);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(!fixture.repo.join("mutants.out").exists());
}
