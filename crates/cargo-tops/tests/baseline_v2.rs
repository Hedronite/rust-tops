//! Baseline v2 debt ledger (`rust-tops-baseline.json`) and `cargo tops baseline`
//! (`--init`, refresh, `--check`, B2 moved-ledger guard, regressions, and
//! two-branch conflict-free merge) (PROTOCOL.md §16.1, §16.2).
mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{json, Value};

const CARGO_STUB: &str = r#"#!/bin/sh
case "$1 $2" in
  "nextest --version") echo "cargo-nextest 0.9.145 (stub)"; exit 0 ;;
  "llvm-cov --version") echo "cargo-llvm-cov 0.9.1"; exit 0 ;;
  "mutants --version") echo "cargo-mutants 27.1.0"; exit 0 ;;
  "crap --version") echo "cargo-crap 0.9.0"; exit 0 ;;
  "deny --version") echo "cargo-deny 0.18.9"; exit 0 ;;
  "clippy --version") echo "clippy 0.1.99 (stub)"; exit 0 ;;
  "fmt --version") echo "rustfmt 1.10.0-stable (stub)"; exit 0 ;;
esac
if [ "$1" = metadata ]; then
  if [ -f "$PWD/crates/demo/Cargo.toml" ]; then
    printf '{"packages":[{"name":"demo","manifest_path":"%s/crates/demo/Cargo.toml"}]}' "$PWD"
  else
    printf '{"packages":[{"name":"demo","manifest_path":"%s/Cargo.toml"}]}' "$PWD"
  fi
  exit 0
fi
prev=
for arg in "$@"; do
  case $prev in
    --output-path | --output)
      mkdir -p "$(dirname "$arg")"
      name=$(basename "$arg")
      if [ -f "$REPORTS/$name" ]; then cp "$REPORTS/$name" "$arg"; else printf 'lcov\n' > "$arg"; fi
      if [ "$1 $2" = "llvm-cov nextest" ]; then
        j="$PWD/target/nextest/ci"
        mkdir -p "$j"
        printf '%s\n' '<testsuites><testsuite name="s"><testcase name="property_stub" classname="s"/></testsuite></testsuites>' > "$j/junit.xml"
      fi
      ;;
  esac
  prev=$arg
done
exit 0
"#;

#[derive(Clone, Copy)]
enum Layout {
    Single,
    Workspace,
}

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

fn fixture(name: &str, layout: Layout) -> Fixture {
    fixture_with_class(name, "library-core", layout)
}

fn fixture_with_class(name: &str, class: &str, layout: Layout) -> Fixture {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("cargo-tops-v2-{name}-{}-{seq}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    let f = Fixture {
        repo: root.join("repo"),
        bin: root.join("bin"),
        reports: root.join("reports"),
        root,
    };
    init_repo(&f, class, layout);
    edit_yaml(&f, enable_gate_layers);
    f
}

fn init_repo(f: &Fixture, class: &str, layout: Layout) {
    fs::create_dir_all(&f.bin).expect("mkdir bin");
    fs::create_dir_all(&f.reports).expect("mkdir reports");
    let init = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(["init", "--class", class, "--name", "demo", "--path"])
        .arg(&f.repo)
        .output()
        .expect("init");
    assert!(init.status.success(), "init failed");
    setup_cargo(f, layout);
    stub(&f.bin, "cargo", CARGO_STUB);
    support::stub_rustc(&f.bin);
    for p in [
        "cargo-llvm-cov",
        "cargo-crap",
        "cargo-nextest",
        "cargo-deny",
    ] {
        stub(&f.bin, p, "#!/bin/sh\nexit 0\n");
    }
}

fn enable_gate_layers(doc: &mut Value) {
    if let Some(layers) = doc.pointer_mut("/layers").and_then(Value::as_array_mut) {
        for layer in layers {
            let on = layer.get("id").and_then(Value::as_str).is_some_and(|id| {
                matches!(id, "coverage" | "crap" | "security-advisory" | "property")
            });
            if let Some(map) = layer.as_object_mut() {
                map.insert("enabled".into(), json!(on));
            }
        }
    }
}

fn setup_cargo(f: &Fixture, layout: Layout) {
    match layout {
        Layout::Workspace => {
            fs::write(
                f.repo.join("Cargo.toml"),
                "[workspace]\nmembers = [\"crates/demo\"]\n",
            )
            .expect("ws");
            fs::create_dir_all(f.repo.join("crates/demo/src")).expect("mkdir src");
            fs::write(
                f.repo.join("crates/demo/Cargo.toml"),
                "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
            )
            .expect("pkg");
            fs::write(f.repo.join("crates/demo/src/a.rs"), "").expect("a.rs");
            fs::write(f.repo.join("crates/demo/src/main.rs"), "").expect("main.rs");
        }
        Layout::Single => {
            fs::create_dir_all(f.repo.join("src")).expect("mkdir src");
            fs::write(
                f.repo.join("Cargo.toml"),
                "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
            )
            .expect("pkg");
        }
    }
}

fn edit_yaml(f: &Fixture, change: impl FnOnce(&mut Value)) {
    let path = f.repo.join("rust-tops.yaml");
    let mut doc: Value =
        serde_saphyr::from_str(&fs::read_to_string(&path).expect("read")).expect("yaml");
    change(&mut doc);
    fs::write(&path, serde_json::to_string_pretty(&doc).expect("json")).expect("write");
}

fn set_key(doc: &mut Value, pointer: &str, key: &str, val: Option<Value>) {
    let map = doc
        .pointer_mut(pointer)
        .and_then(Value::as_object_mut)
        .expect("object");
    match val {
        Some(v) => map.insert(key.into(), v),
        None => map.remove(key),
    };
}

fn write_json(path: &Path, val: &Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(val).expect("pretty")),
    )
    .expect("write json");
}

fn write_reports(f: &Fixture, line_cov: u64, crap_entries: &[(&str, &str, f64)]) {
    let cov_file = if f.repo.join("crates/demo/Cargo.toml").exists() {
        "./crates/demo/src/a.rs"
    } else {
        "src/a.rs"
    };
    let cov = json!({"type": "llvm.coverage.json.export", "data": [{"files": [
        {"filename": cov_file, "summary": {"lines": {"covered": line_cov, "count": 100}, "regions": {"covered": line_cov, "count": 100}}}
    ]}]});
    fs::write(f.reports.join("coverage.json"), cov.to_string()).expect("coverage");
    let entries: Vec<Value> = crap_entries
        .iter()
        .map(|&(file, func, score)| {
            json!({"file": file, "function": func, "line": 42, "cyclomatic": 1.0, "coverage": 100.0, "crap": score})
        })
        .collect();
    fs::write(
        f.reports.join("crap.json"),
        json!({"version": "0.9.0", "entries": entries}).to_string(),
    )
    .expect("crap");
}

fn tops(f: &Fixture, args: &[&str]) -> (Output, String) {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(f.bin.clone()).chain(std::env::split_paths(&inherited)),
    )
    .expect("PATH");
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(args)
        .arg("--path")
        .arg(&f.repo)
        .env("PATH", path)
        .env("REPORTS", &f.reports)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("spawn cargo-tops");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out, text)
}

fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(repo)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn seed_crap_exemption(f: &Fixture) {
    edit_yaml(f, |doc| {
        set_key(
            doc,
            "",
            "exemptions",
            Some(json!([{
                "path": "src/main.rs", "symbol": "main", "layer": "crap",
                "reason": "cli match dispatch table", "owner": "cli", "expires": "2999-01-01"
            }])),
        );
    });
}

#[test]
fn init_normalizes_paths_keeps_max_score_and_refuses_second_init() {
    let f = fixture("init", Layout::Workspace);
    write_reports(
        &f,
        100,
        &[
            ("./crates/demo/src/a.rs", "dup", 9.5),
            ("./crates/demo/src/a.rs", "dup", 14.2),
            ("./crates/demo/src/a.rs", "clean", 6.0),
            ("./crates/demo/src/main.rs", "main", 29.0),
        ],
    );
    seed_crap_exemption(&f);
    assert_eq!(
        tops(&f, &["baseline", "--init"]).0.status.code(),
        Some(2),
        "no gate yet"
    );
    assert_eq!(
        tops(&f, &["gate"]).0.status.code(),
        Some(1),
        "missing baseline fails"
    );
    let (out, text) = tops(&f, &["baseline", "--init"]);
    assert_eq!(out.status.code(), Some(0), "{text}");
    let raw = fs::read_to_string(f.repo.join("rust-tops-baseline.json")).expect("read v2");
    assert!(!raw.contains("42") && !raw.contains("crates/demo"), "{raw}");
    let doc: Value = serde_json::from_str(&raw).expect("json");
    assert_eq!(doc.get("version"), Some(&json!(2)));
    assert_eq!(doc.get("functions"), Some(&json!({"src/a.rs::dup": 14.2})));
    assert_eq!(
        tops(&f, &["gate"]).0.status.code(),
        Some(0),
        "passes after init"
    );
    assert_eq!(
        tops(&f, &["baseline", "--init"]).0.status.code(),
        Some(1),
        "second init refused"
    );
}

#[test]
fn refresh_lowers_and_removes_after_pass() {
    let f = fixture("refresh-ok", Layout::Single);
    write_reports(
        &f,
        100,
        &[("./src/a.rs", "alpha", 12.0), ("./src/a.rs", "beta", 9.0)],
    );
    let _ = tops(&f, &["gate"]);
    assert_eq!(
        tops(&f, &["baseline", "--init"]).0.status.code(),
        Some(0),
        "init"
    );
    let v2_path = f.repo.join("rust-tops-baseline.json");
    let mut seeded: Value =
        serde_json::from_str(&fs::read_to_string(&v2_path).expect("v2")).expect("json");
    set_key(&mut seeded, "", "coverage", Some(json!({"note": "keep"})));
    write_json(&v2_path, &seeded);

    write_reports(
        &f,
        100,
        &[("./src/a.rs", "alpha", 8.0), ("./src/a.rs", "beta", 5.5)],
    );
    assert_eq!(tops(&f, &["gate"]).0.status.code(), Some(0), "pass gate");
    assert_eq!(tops(&f, &["baseline"]).0.status.code(), Some(0), "refresh");
    let refreshed: Value =
        serde_json::from_str(&fs::read_to_string(&v2_path).expect("v2")).expect("json");
    assert_eq!(
        refreshed.get("functions"),
        Some(&json!({"src/a.rs::alpha": 8.0}))
    );
    assert_eq!(refreshed.get("coverage"), Some(&json!({"note": "keep"})));
}

#[test]
fn refresh_refuses_after_failing_gate_or_score_rise() {
    let f = fixture("refresh-no", Layout::Single);
    write_reports(&f, 100, &[("./src/a.rs", "alpha", 12.0)]);
    let _ = tops(&f, &["gate"]);
    assert_eq!(
        tops(&f, &["baseline", "--init"]).0.status.code(),
        Some(0),
        "init"
    );

    write_reports(&f, 40, &[("./src/a.rs", "alpha", 8.0)]);
    assert_eq!(tops(&f, &["gate"]).0.status.code(), Some(1), "fail gate");
    assert_eq!(
        tops(&f, &["baseline"]).0.status.code(),
        Some(1),
        "refuse after fail"
    );

    write_reports(&f, 100, &[("./src/a.rs", "alpha", 8.0)]);
    assert_eq!(tops(&f, &["gate"]).0.status.code(), Some(0), "pass gate");
    let tampered = json!({"version": "0.9.0", "entries": [{"file": "./src/a.rs", "function": "alpha", "line": 42, "cyclomatic": 1.0, "coverage": 100.0, "crap": 15.0}]});
    fs::write(
        f.repo.join("target/rust-tops/crap.json"),
        tampered.to_string(),
    )
    .expect("tamper");
    let (out, text) = tops(&f, &["baseline"]);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("src/a.rs::alpha"), "{text}");
}

#[test]
fn check_rejects_under_cap_and_line_numbers() {
    let f = fixture("check-form", Layout::Single);
    git(&f.repo, &["init", "-q", "-b", "main"]);
    let v2_path = f.repo.join("rust-tops-baseline.json");
    write_json(
        &v2_path,
        &json!({"version": 2, "crate": "demo", "functions": {"src/a.rs::alpha": 10.0}}),
    );
    git(&f.repo, &["add", "-A"]);
    git(&f.repo, &["commit", "-q", "-m", "base"]);
    assert_eq!(
        tops(&f, &["baseline", "--check"]).0.status.code(),
        Some(0),
        "clean"
    );

    write_json(
        &v2_path,
        &json!({"version": 2, "crate": "demo", "functions": {"src/a.rs::at": 6.0}}),
    );
    assert_eq!(
        tops(&f, &["baseline", "--check"]).0.status.code(),
        Some(1),
        "at cap"
    );

    write_json(
        &v2_path,
        &json!({"version": 2, "crate": "demo", "functions": {"src/a.rs:9::alpha": 10.0}}),
    );
    assert_eq!(
        tops(&f, &["baseline", "--check"]).0.status.code(),
        Some(1),
        "line number"
    );
}

#[test]
fn check_rejects_capless_workspace_root_with_nonempty_functions() {
    let ws = fixture_with_class("check-ws", "workspace-root", Layout::Single);
    git(&ws.repo, &["init", "-q", "-b", "main"]);
    write_json(
        &ws.repo.join("rust-tops-baseline.json"),
        &json!({"version": 2, "crate": "demo", "functions": {"src/a.rs::any": 10.0}}),
    );
    assert_eq!(
        tops(&ws, &["baseline", "--check"]).0.status.code(),
        Some(1),
        "capless non-empty"
    );
}

#[test]
fn check_catches_moved_ledger_bypass() {
    let f = fixture("check-moved", Layout::Single);
    git(&f.repo, &["init", "-q", "-b", "main"]);
    let v2_path = f.repo.join("rust-tops-baseline.json");
    write_json(
        &v2_path,
        &json!({"version": 2, "crate": "demo", "functions": {"src/a.rs::alpha": 10.0}}),
    );
    git(&f.repo, &["add", "-A"]);
    git(&f.repo, &["commit", "-q", "-m", "base"]);

    fs::remove_file(&v2_path).expect("rm old");
    write_json(
        &f.repo.join("debt/ledger.json"),
        &json!({"version": 2, "crate": "demo", "functions": {"src/a.rs::alpha": 12.0}}),
    );
    edit_yaml(&f, |doc| {
        set_key(doc, "", "baseline", Some(json!("debt/ledger.json")))
    });
    assert_eq!(
        tops(&f, &["baseline", "--check"]).0.status.code(),
        Some(1),
        "moved bypass refused"
    );
}

#[test]
fn check_requires_exact_regression_records() {
    let f = fixture("check-reg", Layout::Single);
    git(&f.repo, &["init", "-q", "-b", "main"]);
    let v2_path = f.repo.join("rust-tops-baseline.json");
    write_json(
        &v2_path,
        &json!({"version": 2, "crate": "demo", "functions": {"src/a.rs::alpha": 10.0}}),
    );
    git(&f.repo, &["add", "-A"]);
    git(&f.repo, &["commit", "-q", "-m", "base"]);

    write_json(
        &v2_path,
        &json!({
            "version": 2, "crate": "demo", "functions": {"src/a.rs::alpha": 12.0},
            "regressions": [{"kind": "functions", "key": "src/a.rs::alpha", "from": 9.0, "to": 12.0, "reason": "r", "owner": "o", "date": "2026-11-25"}]
        }),
    );
    assert_eq!(
        tops(&f, &["baseline", "--check"]).0.status.code(),
        Some(1),
        "stale from rejected"
    );

    write_json(
        &v2_path,
        &json!({
            "version": 2, "crate": "demo",
            "functions": {"src/a.rs::alpha": 12.0, "src/b.rs::newfn": 9.0},
            "regressions": [
                {"kind": "functions", "key": "src/a.rs::alpha", "from": 10.0, "to": 12.0, "reason": "r", "owner": "o", "date": "2026-11-25"},
                {"kind": "functions", "key": "src/b.rs::newfn", "from": null, "to": 9.0, "reason": "r", "owner": "o", "date": "2026-11-25"}
            ]
        }),
    );
    let (out, text) = tops(&f, &["baseline", "--check"]);
    assert_eq!(out.status.code(), Some(0), "{text}");
}

#[test]
fn check_fails_closed_on_unparseable_base_config() {
    let f = fixture("check-bad-base", Layout::Single);
    let good_yaml = fs::read_to_string(f.repo.join("rust-tops.yaml")).expect("read good yaml");
    git(&f.repo, &["init", "-q", "-b", "main"]);
    fs::write(f.repo.join("rust-tops.yaml"), "[not valid yaml").expect("bad yaml");
    write_json(
        &f.repo.join("rust-tops-baseline.json"),
        &json!({"version": 2, "crate": "demo", "functions": {}}),
    );
    git(&f.repo, &["add", "-A"]);
    git(&f.repo, &["commit", "-q", "-m", "bad-base"]);
    fs::write(f.repo.join("rust-tops.yaml"), good_yaml).expect("restore good yaml");
    assert_ne!(
        tops(&f, &["baseline", "--check"]).0.status.code(),
        Some(0),
        "fail closed"
    );
}

#[test]
fn two_branches_touching_different_functions_merge_without_conflict() {
    let f = fixture("merge", Layout::Single);
    git(&f.repo, &["init", "-q", "-b", "main"]);
    let base = "{\n  \"version\": 2,\n  \"crate\": \"demo\",\n  \"functions\": {\n    \"src/a.rs::alpha\": 12.0,\n    \"src/m.rs::mid\": 11.0,\n    \"src/z.rs::zeta\": 14.0\n  }\n}\n";
    let v2_path = f.repo.join("rust-tops-baseline.json");
    fs::write(&v2_path, base).expect("base v2");
    git(&f.repo, &["add", "-A"]);
    git(&f.repo, &["commit", "-q", "-m", "base"]);

    git(&f.repo, &["checkout", "-q", "-b", "branch-a"]);
    fs::write(&v2_path, base.replace("12.0", "9.5")).expect("a v2");
    git(&f.repo, &["commit", "-am", "lower alpha"]);

    git(&f.repo, &["checkout", "-q", "main"]);
    git(&f.repo, &["checkout", "-q", "-b", "branch-b"]);
    fs::write(&v2_path, base.replace("14.0", "10.0")).expect("b v2");
    git(&f.repo, &["commit", "-am", "lower zeta"]);

    git(&f.repo, &["merge", "-q", "--no-edit", "branch-a"]);
    let (out, text) = tops(&f, &["baseline", "--check", "--base", "main"]);
    assert_eq!(out.status.code(), Some(0), "{text}");
    let merged: Value =
        serde_json::from_str(&fs::read_to_string(&v2_path).expect("merged")).expect("json");
    assert_eq!(
        merged.get("functions"),
        Some(&json!({"src/a.rs::alpha": 9.5, "src/m.rs::mid": 11.0, "src/z.rs::zeta": 10.0}))
    );
}

#[test]
fn crap_baseline_alias_works_with_deprecation_warning() {
    let f = fixture("alias", Layout::Single);
    write_reports(&f, 100, &[("./src/a.rs", "legacy", 12.0)]);
    edit_yaml(&f, |doc| set_key(doc, "", "enforce", None));
    assert_eq!(
        tops(&f, &["gate"]).0.status.code(),
        Some(1),
        "over cap before baseline"
    );
    let (out, text) = tops(&f, &["crap-baseline"]);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("deprecated") && text.contains("cargo tops baseline"),
        "{text}"
    );
}

#[test]
fn check_accepts_first_ledger_when_base_has_no_ledger_or_yaml() {
    let f = fixture("first-ledger", Layout::Single);
    let good_yaml = fs::read_to_string(f.repo.join("rust-tops.yaml")).expect("read yaml");
    fs::remove_file(f.repo.join("rust-tops.yaml")).expect("rm yaml");
    git(&f.repo, &["init", "-q", "-b", "main"]);
    git(&f.repo, &["add", "-A"]);
    git(&f.repo, &["commit", "-q", "-m", "initial"]);

    fs::write(f.repo.join("rust-tops.yaml"), good_yaml).expect("restore yaml");
    write_json(
        &f.repo.join("rust-tops-baseline.json"),
        &json!({"version": 2, "crate": "demo", "functions": {"src/a.rs::alpha": 10.0}}),
    );
    let (out, text) = tops(&f, &["baseline", "--check"]);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("first ledger"), "{text}");
}

#[test]
fn refresh_refuses_when_crap_layer_did_not_pass() {
    let f = fixture("refresh-no-crap", Layout::Single);
    write_reports(&f, 100, &[("./src/a.rs", "alpha", 12.0)]);
    let _ = tops(&f, &["gate"]);
    assert_eq!(
        tops(&f, &["baseline", "--init"]).0.status.code(),
        Some(0),
        "init"
    );

    edit_yaml(&f, |doc| {
        if let Some(layers) = doc.pointer_mut("/layers").and_then(Value::as_array_mut) {
            for layer in layers {
                if layer.get("id").and_then(Value::as_str) == Some("crap") {
                    if let Some(map) = layer.as_object_mut() {
                        map.insert("enabled".into(), json!(false));
                    }
                }
            }
        }
    });
    assert_eq!(
        tops(&f, &["gate"]).0.status.code(),
        Some(0),
        "pass without crap"
    );
    let (out, text) = tops(&f, &["baseline"]);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("crap layer"), "{text}");
}
