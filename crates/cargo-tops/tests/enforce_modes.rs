//! Enforcement modes (`hard`, `ratchet`, `advisory`, omitted 1.4.0 compat),
//! advisory expiry/validation, and unmasked `report.json` (PROTOCOL.md §2.3).
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
  printf '{"packages":[{"name":"fixture","manifest_path":"%s/Cargo.toml"}]}' "$PWD"
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

fn fixture(name: &str) -> Fixture {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "cargo-tops-enforce-{name}-{}-{seq}",
        std::process::id()
    ));
    fs::remove_dir_all(&root).ok();
    let f = Fixture {
        repo: root.join("repo"),
        bin: root.join("bin"),
        reports: root.join("reports"),
        root,
    };
    init_fixture(&f);
    edit_yaml(&f, |doc| {
        keep_layers(doc, &["coverage", "crap", "security-advisory", "property"])
    });
    write_reports(&f, 100, &[("./src/lib.rs", "legacy", 12.0)]);
    f
}

fn init_fixture(f: &Fixture) {
    fs::create_dir_all(&f.bin).expect("mkdir bin");
    fs::create_dir_all(&f.reports).expect("mkdir reports");
    let init = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args([
            "init",
            "--class",
            "library-core",
            "--name",
            "fixture",
            "--path",
        ])
        .arg(&f.repo)
        .output()
        .expect("init");
    assert!(init.status.success(), "init failed");
    fs::write(
        f.repo.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .expect("manifest");
    stub(&f.bin, "cargo", CARGO_STUB);
    support::stub_rustc(&f.bin);
    for plugin in [
        "cargo-llvm-cov",
        "cargo-crap",
        "cargo-nextest",
        "cargo-deny",
    ] {
        stub(&f.bin, plugin, "#!/bin/sh\nexit 0\n");
    }
}

fn keep_layers(doc: &mut Value, on: &[&str]) {
    if let Some(layers) = doc.pointer_mut("/layers").and_then(Value::as_array_mut) {
        for layer in layers {
            let enabled = layer
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| on.contains(&id));
            if let Some(map) = layer.as_object_mut() {
                map.insert("enabled".into(), json!(enabled));
            }
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

fn write_reports(f: &Fixture, line_cov: u64, crap_entries: &[(&str, &str, f64)]) {
    let cov = json!({"type": "llvm.coverage.json.export", "data": [{"files": [
        {"filename": "src/lib.rs", "summary": {"lines": {"covered": line_cov, "count": 100}, "regions": {"covered": line_cov, "count": 100}}}
    ]}]});
    fs::write(f.reports.join("coverage.json"), cov.to_string()).expect("write coverage");
    let entries: Vec<Value> = crap_entries
        .iter()
        .map(|&(file, func, score)| {
            json!({"file": file, "function": func, "line": 1, "cyclomatic": 1.0, "coverage": 100.0, "crap": score})
        })
        .collect();
    fs::write(
        f.reports.join("crap.json"),
        json!({"version": "0.9.0", "entries": entries}).to_string(),
    )
    .expect("write crap");
}

fn write_v2(f: &Fixture, entries: &[(&str, f64)]) {
    let mut map = serde_json::Map::new();
    for &(k, v) in entries {
        map.insert(k.to_string(), json!(v));
    }
    let doc = json!({"version": 2, "crate": "fixture", "functions": map});
    fs::write(
        f.repo.join("rust-tops-baseline.json"),
        format!("{}\n", serde_json::to_string_pretty(&doc).expect("json")),
    )
    .expect("write v2");
}

fn run(f: &Fixture, args: &[&str]) -> (Output, String) {
    let path = std::env::join_paths([f.bin.as_path(), Path::new("/usr/bin"), Path::new("/bin")])
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

fn read_report(f: &Fixture) -> Value {
    let raw = fs::read_to_string(f.repo.join("target/rust-tops/report.json")).expect("report.json");
    serde_json::from_str(&raw).expect("report json")
}

#[test]
fn ratchet_requires_baseline_and_allows_listed_within_epsilon_only() {
    let f = fixture("ratchet");
    let (out, text) = run(&f, &["gate"]);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("cargo tops baseline --init"), "{text}");

    write_v2(&f, &[("src/lib.rs::legacy", 12.0)]);
    write_reports(&f, 100, &[("./src/lib.rs", "legacy", 12.01)]);
    assert_eq!(
        run(&f, &["gate"]).0.status.code(),
        Some(0),
        "within epsilon"
    );

    write_reports(&f, 100, &[("./src/lib.rs", "legacy", 12.02)]);
    assert_eq!(run(&f, &["gate"]).0.status.code(), Some(1), "above epsilon");

    write_reports(
        &f,
        100,
        &[
            ("./src/lib.rs", "legacy", 12.0),
            ("./src/lib.rs", "fresh", 7.0),
        ],
    );
    assert_eq!(
        run(&f, &["gate"]).0.status.code(),
        Some(1),
        "unlisted over cap"
    );
}

#[test]
fn hard_ignores_baseline_and_only_allows_live_exemption() {
    let f = fixture("hard");
    write_v2(&f, &[("src/lib.rs::legacy", 12.0)]);
    edit_yaml(&f, |doc| set_key(doc, "", "enforce", Some(json!("hard"))));
    let (out, text) = run(&f, &["gate"]);
    assert_eq!(out.status.code(), Some(1), "{text}");
    let rep = read_report(&f);
    assert_eq!(rep.get("enforce"), Some(&json!("hard")));
    assert_eq!(rep.get("enforced"), Some(&json!(true)));

    edit_yaml(&f, |doc| {
        set_key(
            doc,
            "",
            "exemptions",
            Some(json!([{
                "path": "src/lib.rs", "symbol": "legacy", "layer": "crap",
                "reason": "covered by integration oracle", "owner": "cli", "expires": "2999-01-01"
            }])),
        );
    });
    assert_eq!(
        run(&f, &["gate"]).0.status.code(),
        Some(0),
        "live exemption passes"
    );
}

fn set_advisory(f: &Fixture, expires: &str, reason: &str) {
    edit_yaml(f, |doc| {
        set_key(doc, "", "enforce", Some(json!("advisory")));
        set_key(
            doc,
            "",
            "advisory",
            Some(json!({"expires": expires, "reason": reason})),
        );
    });
}

#[test]
fn advisory_missing_and_malformed_exit_two() {
    let f = fixture("adv-bad");
    edit_yaml(&f, |doc| {
        set_key(doc, "", "enforce", Some(json!("advisory")))
    });
    assert_eq!(
        run(&f, &["check"]).0.status.code(),
        Some(2),
        "missing check"
    );
    assert_eq!(run(&f, &["gate"]).0.status.code(), Some(2), "missing gate");

    set_advisory(&f, "2026-02-30", "valid four word reason");
    assert_eq!(
        run(&f, &["check"]).0.status.code(),
        Some(2),
        "bad date check"
    );
    assert_eq!(run(&f, &["gate"]).0.status.code(), Some(2), "bad date gate");

    set_advisory(&f, "2099-01-01", "two words");
    assert_eq!(
        run(&f, &["check"]).0.status.code(),
        Some(2),
        "short reason check"
    );
    assert_eq!(
        run(&f, &["gate"]).0.status.code(),
        Some(2),
        "short reason gate"
    );
}

#[test]
fn advisory_expired_warns_check_and_fails_gate_one() {
    let f = fixture("adv-exp");
    set_advisory(&f, "2020-01-01", "three whole words");
    let (chk, chk_txt) = run(&f, &["check"]);
    assert_eq!(chk.status.code(), Some(0), "{chk_txt}");
    assert!(chk_txt.contains("warning: advisory:"), "{chk_txt}");
    let (g, g_txt) = run(&f, &["gate"]);
    assert_eq!(g.status.code(), Some(1), "{g_txt}");
    let rep = read_report(&f);
    assert_eq!(rep.get("enforced"), Some(&json!(true)));
    assert_eq!(rep.get("ok"), Some(&json!(false)));
}

#[test]
fn live_advisory_exits_zero_with_unmasked_report() {
    let f = fixture("adv-live-rep");
    write_reports(&f, 40, &[("./src/lib.rs", "legacy", 12.0)]);
    set_advisory(&f, "2999-01-01", "adopting on legacy crate");
    let (out, text) = run(&f, &["gate"]);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("NOT ENFORCED (advisory, expires 2999-01-01)"),
        "{text}"
    );
    let rep = read_report(&f);
    assert_eq!(rep.get("exit_code"), Some(&json!(0)));
    assert_eq!(rep.get("ok"), Some(&json!(false)));
    assert_eq!(rep.get("kind"), Some(&json!("fail")));
    assert_eq!(rep.get("enforce"), Some(&json!("advisory")));
    assert_eq!(rep.get("enforced"), Some(&json!(false)));
    assert_eq!(rep.get("advisory_expires"), Some(&json!("2999-01-01")));
    assert!(
        rep.get("problems")
            .and_then(Value::as_array)
            .is_some_and(|p| !p.is_empty()),
        "{rep}"
    );
}

#[test]
fn live_advisory_keeps_dry_run_pre_fault_and_refuses_baseline() {
    let f = fixture("adv-live-ctl");
    write_reports(&f, 40, &[("./src/lib.rs", "legacy", 12.0)]);
    set_advisory(&f, "2999-01-01", "adopting on legacy crate");
    assert_eq!(
        run(&f, &["gate", "--dry-run"]).0.status.code(),
        Some(1),
        "dry run"
    );
    assert_eq!(
        run(&f, &["baseline", "--init"]).0.status.code(),
        Some(1),
        "refuse init"
    );
    assert_eq!(
        run(&f, &["baseline"]).0.status.code(),
        Some(1),
        "refuse refresh"
    );
    fs::write(
        f.repo.join("rust-tops.lock"),
        "[tools
",
    )
    .expect("bad lock");
    assert_eq!(
        run(&f, &["gate"]).0.status.code(),
        Some(2),
        "pre-run exit 2"
    );
}

#[test]
fn omitted_enforce_preserves_legacy_v1_and_check_warns_when_both_set() {
    let f = fixture("legacy");
    let v1 = json!({"entries": [{"file": "./src/lib.rs", "function": "legacy", "line": 1, "crap": 12.0}]});
    fs::write(f.repo.join("legacy-v1.json"), v1.to_string()).expect("write v1");
    edit_yaml(&f, |doc| {
        set_key(doc, "", "enforce", None);
        set_key(
            doc,
            "/gates/crap",
            "baseline",
            Some(json!("legacy-v1.json")),
        );
    });
    assert_eq!(
        run(&f, &["gate"]).0.status.code(),
        Some(0),
        "v1 compat pass"
    );
    assert!(
        read_report(&f).get("enforce").is_none(),
        "omitted enforce absent"
    );

    edit_yaml(&f, |doc| {
        set_key(doc, "", "enforce", Some(json!("ratchet")))
    });
    let (chk, chk_txt) = run(&f, &["check"]);
    assert_eq!(chk.status.code(), Some(0), "{chk_txt}");
    assert!(
        chk_txt.contains("gates.crap.baseline is ignored"),
        "{chk_txt}"
    );
    assert_eq!(
        run(&f, &["gate"]).0.status.code(),
        Some(1),
        "ratchet ignores v1"
    );
}

#[test]
fn live_advisory_never_masks_environment_exit_three() {
    let mismatch = fixture("adv-pin");
    write_reports(&mismatch, 100, &[("./src/lib.rs", "ok", 1.0)]);
    set_advisory(&mismatch, "2999-01-01", "adopting on legacy crate");
    let lock = fs::read_to_string(mismatch.repo.join("rust-tops.lock")).expect("read lock");
    fs::write(
        mismatch.repo.join("rust-tops.lock"),
        lock.replacen("0.9.0", "0.0.1", 1),
    )
    .expect("write lock");
    let (out, text) = run(&mismatch, &["gate"]);
    assert_eq!(out.status.code(), Some(3), "{text}");
    assert!(!text.contains("NOT ENFORCED"), "{text}");
    let rep = read_report(&mismatch);
    assert_eq!(rep.get("kind"), Some(&json!("environment")));
    assert_eq!(rep.get("exit_code"), Some(&json!(3)));
    assert_eq!(rep.get("enforced"), Some(&json!(true)));

    let missing = fixture("adv-missing");
    write_reports(&missing, 100, &[("./src/lib.rs", "ok", 1.0)]);
    set_advisory(&missing, "2999-01-01", "adopting on legacy crate");
    fs::remove_file(missing.bin.join("cargo-deny")).expect("rm cargo-deny");
    let (mout, mtext) = run(&missing, &["gate"]);
    assert_eq!(mout.status.code(), Some(3), "{mtext}");
    let mrep = read_report(&missing);
    assert_eq!(mrep.get("kind"), Some(&json!("environment")));
    assert_eq!(mrep.get("exit_code"), Some(&json!(3)));
    assert_eq!(mrep.get("enforced"), Some(&json!(true)));
}

#[test]
fn clean_live_advisory_passes_with_unenforced_banner() {
    let f = fixture("adv-live-pass");
    write_reports(&f, 100, &[("./src/lib.rs", "ok", 1.0)]);
    set_advisory(&f, "2999-01-01", "adopting on legacy crate");
    let (out, text) = run(&f, &["gate"]);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("NOT ENFORCED (advisory, expires 2999-01-01)"),
        "{text}"
    );
    let rep = read_report(&f);
    assert_eq!(rep.get("exit_code"), Some(&json!(0)));
    assert_eq!(rep.get("ok"), Some(&json!(true)));
    assert_eq!(rep.get("kind"), Some(&json!("pass")));
    assert_eq!(rep.get("enforce"), Some(&json!("advisory")));
    assert_eq!(rep.get("enforced"), Some(&json!(false)));
    assert_eq!(rep.get("advisory_expires"), Some(&json!("2999-01-01")));
    assert_eq!(rep.get("problems"), Some(&json!([])));
}
