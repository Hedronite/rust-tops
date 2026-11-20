//! A diff with nothing to mutate, through the real cargo-mutants. Real 27.1.0
//! exits 0 for a diff of a README or of a comment, writes no `mutants.out`
//! and says so on stderr; the layer passes as zero mutants. Only `cargo-deny`
//! is a stub. The test needs `cargo-mutants` 27.1.0 on `PATH`: without it
//! the test stops, unless `RUST_TOPS_REQUIRE_MUTANTS` is set, which fails it.
mod support;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

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
if [ "$1" = "mutants" ]; then
  out=
  prev=
  for arg in "$@"; do
    if [ "$prev" = "--output" ]; then out=$arg; fi
    prev=$arg
  done
  if [ -z "$out" ]; then echo "no --output given" >&2; exit 64; fi
  if [ -n "${MUTANTS_SAYS:-}" ]; then echo "$MUTANTS_SAYS" >&2; fi
  if [ -n "${MUTANTS_OUTCOMES:-}" ]; then
    mkdir -p "$out/mutants.out" && cp "$MUTANTS_OUTCOMES" "$out/mutants.out/outcomes.json"
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

/// The plugin files `check_tool` looks for. Real cargo hands `cargo deny
/// --version` to the `cargo-deny` file, so it answers with the pinned line, as
/// the real tool does.
fn stub_plugins(bin: &Path) {
    for tool in ["cargo-deny", "cargo-nextest", "cargo-mutants"] {
        let body = if tool == "cargo-deny" {
            "#!/bin/sh\necho 'cargo-deny 0.18.9'\n"
        } else {
            "#!/bin/sh\nexit 0\n"
        };
        stub(bin, tool, body);
    }
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
            "library-util",
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
    support::stub_rustc(&fixture.bin);
    stub_plugins(&fixture.bin);
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

/// The real `cargo-mutants`, or `None` when it is not installed. Without it
/// the tests stop, unless `RUST_TOPS_REQUIRE_MUTANTS` is set: then they fail.
fn real_mutants() -> Option<std::ffi::OsString> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let version = Command::new("cargo")
        .args(["mutants", "--version"])
        .env("PATH", &path)
        .output();
    let said = version
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string());
    if said.as_deref() == Some("cargo-mutants 27.1.0") {
        return Some(path);
    }
    assert!(
        std::env::var_os("RUST_TOPS_REQUIRE_MUTANTS").is_none(),
        "RUST_TOPS_REQUIRE_MUTANTS is set and `cargo mutants --version` printed {said:?}, not cargo-mutants 27.1.0"
    );
    eprintln!("cargo-mutants 27.1.0 is not installed: this test did not run");
    None
}

/// A fixture with only `mutation` and `security-advisory` on, `cargo-deny`
/// a stub and everything else real, whose `feature` branch holds one more
/// commit that writes `file` as `text`. Returns the gate's output.
fn gate_after_writing(name: &str, file: &str, text: &str) -> Option<(Fixture, Output)> {
    let path = real_mutants()?;
    let fixture = fixture(name, "main");
    edit(&fixture, |doc| {
        let layers = doc.pointer_mut("/layers").and_then(Value::as_array_mut);
        for layer in layers.expect("layers") {
            let id = layer.get("id").and_then(Value::as_str).unwrap_or_default();
            let on = matches!(id, "security-advisory" | "mutation");
            set(layer, "enabled", Value::Bool(on));
        }
    });
    // Only `cargo-deny` stays a stub: the real cargo and cargo-mutants run.
    for real in ["cargo", "cargo-mutants", "cargo-nextest"] {
        fs::remove_file(fixture.bin.join(real)).expect("remove the stub");
    }
    fs::write(
        fixture.repo.join("src/lib.rs"),
        "pub fn one() -> u8 {\n    1\n}\n",
    )
    .expect("lib");
    git(&fixture, &fixture.repo, &["add", "-A"]);
    git(&fixture, &fixture.repo, &["commit", "-q", "-m", "lib"]);
    git(
        &fixture,
        &fixture.repo,
        &["checkout", "-q", "-b", "feature"],
    );
    fs::write(fixture.repo.join(file), text).expect("write the change");
    git(&fixture, &fixture.repo, &["add", "-A"]);
    git(&fixture, &fixture.repo, &["commit", "-q", "-m", "change"]);
    let mut dirs = vec![fixture.bin.clone()];
    dirs.extend(std::env::split_paths(&path));
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(["gate", "--base", "main", "--path"])
        .arg(&fixture.repo)
        .env("PATH", std::env::join_paths(dirs).expect("PATH"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CI")
        .output()
        .expect("spawn cargo-tops gate");
    Some((fixture, out))
}

fn expect_zero_mutants(fixture: &Fixture, out: &Output, said: &str) {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stdout}{stderr}");
    assert!(
        stderr.contains(said),
        "cargo mutants said {said:?}: {stderr}"
    );
    let report = report(fixture);
    let row = mutation(&report);
    assert_eq!(at(row, "/verdict"), "pass", "{row}");
    assert_eq!(at(row, "/judged/caught"), 0, "{row}");
    assert_eq!(at(row, "/judged/missed"), 0, "{row}");
    assert!(mutation_problems(&report).is_empty(), "{report}");
    // Real 27.1.0 writes no `mutants.out` for a diff with nothing to mutate.
    let moved = fixture.repo.join("target/rust-tops/mutants.out");
    assert!(!moved.exists(), "27.1.0 wrote {}", moved.display());
    let here = fixture.repo.join("mutants.out");
    assert!(!here.exists(), "27.1.0 wrote {}", here.display());
}

#[test]
fn a_readme_only_diff_passes_through_the_real_cargo_mutants() {
    if let Some((fixture, out)) = gate_after_writing("readme-only", "README.md", "# fixture\n") {
        expect_zero_mutants(&fixture, &out, "Diff changes no Rust source files");
    }
}

#[test]
fn a_comment_only_diff_passes_through_the_real_cargo_mutants() {
    let changed = "/// One.\npub fn one() -> u8 {\n    1\n}\n";
    if let Some((fixture, out)) = gate_after_writing("comment-only", "src/lib.rs", changed) {
        expect_zero_mutants(&fixture, &out, "No mutants to filter");
    }
}
