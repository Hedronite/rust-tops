//! `rust-tops-style.nu check`: the golden, every probe's firing line and near-miss,
//! byte-identical reruns, the Nushell minor refusal, `--base` and a broken build.
//! Each test runs the template script against a copy of the fixture crate.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn script() -> PathBuf {
    manifest().join("templates/skills/rust-tops-style/rust-tops-style.nu")
}

fn fixture_src() -> PathBuf {
    manifest().join("tests/fixtures/style/crate")
}

fn nu_bin() -> Option<PathBuf> {
    let found = Command::new("sh")
        .args(["-c", "command -v nu"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    if found.is_some() {
        return found;
    }
    let local = PathBuf::from("/tmp/nu/nu");
    local.is_file().then_some(local)
}

/// The nu binary, or `None` to skip. `RUST_TOPS_REQUIRE_NU` makes a missing one fail.
fn nu_or_skip() -> Option<PathBuf> {
    let nu = nu_bin();
    assert!(
        nu.is_some() || std::env::var_os("RUST_TOPS_REQUIRE_NU").is_none(),
        "nu is not installed; RUST_TOPS_REQUIRE_NU is set"
    );
    if nu.is_none() {
        eprintln!("nu is not installed; style skill tests skipped");
    }
    nu
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("mkdir");
    for entry in fs::read_dir(from).expect("read_dir") {
        let entry = entry.expect("entry");
        let dest = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), &dest).expect("copy");
        }
    }
}

/// A fresh copy of the fixture crate in its own temp dir, plus a target dir.
fn fresh(label: &str) -> (PathBuf, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("cargo-tops-style-{label}-{}", std::process::id()));
    drop(fs::remove_dir_all(&root));
    let dir = root.join("crate");
    copy_dir(&fixture_src(), &dir);
    fs::rename(dir.join("fixture-Cargo.toml"), dir.join("Cargo.toml")).expect("Cargo.toml");
    (dir, root.join("target"))
}

/// Run `check` and return its stdout. The script exits 0 whenever it prints JSON.
fn check(nu: &Path, dir: &Path, target: &Path, extra: &[&str]) -> String {
    let out = Command::new(nu)
        .arg(script())
        .args(["check", "--path"])
        .arg(dir)
        .args(extra)
        .env("CARGO_TARGET_DIR", target)
        .output()
        .expect("spawn nu");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8")
}

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text.trim()).expect("the script prints one JSON object")
}

fn findings(doc: &serde_json::Value) -> Vec<serde_json::Value> {
    doc.get("data")
        .and_then(|d| d.get("findings"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn field<'a>(finding: &'a serde_json::Value, key: &str) -> &'a serde_json::Value {
    finding.get(key).unwrap_or(&serde_json::Value::Null)
}

fn catalog() -> Vec<toml::Value> {
    let text = fs::read_to_string(script().with_file_name("rules.toml")).expect("rules.toml");
    let doc: toml::Table = text.parse().expect("rules.toml parses");
    doc.get("rule")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// True when the rule lists `style` in its `skills`.
fn for_style(rule: &toml::Value) -> bool {
    rule.get("skills")
        .and_then(toml::Value::as_array)
        .is_some_and(|all| all.iter().any(|v| v.as_str() == Some("style")))
}

fn rule_str<'a>(rule: &'a toml::Value, key: &str) -> &'a str {
    rule.get(key).and_then(toml::Value::as_str).unwrap_or("")
}

/// One trailing `//@ fire|near ID [col=N]` marker, on `line`.
struct Marker {
    kind: String,
    id: String,
    line: u64,
    col: Option<u64>,
}

fn markers(lib: &str) -> Vec<Marker> {
    let mut found = Vec::new();
    for (index, text) in lib.lines().enumerate() {
        let Some((_, tail)) = text.split_once("//@ ") else {
            continue;
        };
        let mut words = tail.split_whitespace();
        if let (Some(kind), Some(id)) = (words.next(), words.next()) {
            found.push(Marker {
                kind: kind.to_string(),
                id: id.to_string(),
                line: index as u64 + 1,
                col: words
                    .next()
                    .and_then(|w| w.strip_prefix("col="))
                    .and_then(|n| n.parse().ok()),
            });
        }
    }
    found
}

type Finding = serde_json::Value;

fn assert_sorted_and_known(all: &[Finding], rules: &[toml::Value]) {
    let mut keys: Vec<(String, u64, u64, String)> = Vec::new();
    for f in all {
        let id = field(f, "rule").as_str().expect("rule");
        assert!(
            rules.iter().any(|r| rule_str(r, "id") == id),
            "unknown rule {id}"
        );
        keys.push((
            field(f, "file").as_str().expect("file").to_string(),
            field(f, "line").as_u64().expect("line"),
            field(f, "col").as_u64().expect("col"),
            id.to_string(),
        ));
    }
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "findings are not sorted");
}

fn assert_pairs(all: &[Finding], marks: &[Marker]) {
    let at = |m: &Marker| {
        all.iter()
            .find(|f| {
                field(f, "rule").as_str() == Some(m.id.as_str())
                    && field(f, "line").as_u64() == Some(m.line)
            })
            .cloned()
    };
    for m in marks {
        if m.kind == "fire" {
            let hit = at(m);
            assert!(hit.is_some(), "{} did not fire on line {}", m.id, m.line);
            if let (Some(want), Some(hit)) = (m.col, hit) {
                assert_eq!(field(&hit, "col").as_u64(), Some(want), "{} column", m.id);
            }
        } else {
            assert_eq!(m.kind, "near", "unknown marker {}", m.kind);
            assert!(
                at(m).is_none(),
                "{} fired on its near-miss, line {}",
                m.id,
                m.line
            );
        }
    }
}

fn assert_probes_paired(rules: &[toml::Value], marks: &[Marker]) {
    for rule in rules
        .iter()
        .filter(|r| rule_str(r, "kind") == "probe" && for_style(r))
    {
        let id = rule_str(rule, "id");
        for kind in ["fire", "near"] {
            assert!(
                marks.iter().any(|m| m.kind == kind && m.id == id),
                "probe {id} has no {kind} line in the fixture"
            );
        }
    }
}

#[test]
fn findings_match_the_golden_and_every_rule_has_a_pair() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir, target) = fresh("golden");
    let stdout = check(&nu, &dir, &target, &[]);
    let golden = manifest().join("tests/fixtures/style/expected.json");
    if std::env::var_os("RUST_TOPS_BLESS").is_some() {
        fs::write(&golden, &stdout).expect("bless golden");
    }
    assert_eq!(stdout, fs::read_to_string(&golden).expect("golden"));

    let doc = json(&stdout);
    assert_eq!(
        doc.get("ok"),
        Some(&serde_json::Value::Bool(false)),
        "{doc}"
    );
    let rules = catalog();
    let all = findings(&doc);
    assert_sorted_and_known(&all, &rules);
    let lib = fs::read_to_string(dir.join("src/lib.rs")).expect("lib.rs");
    let marks = markers(&lib);
    assert_pairs(&all, &marks);
    assert_probes_paired(&rules, &marks);
}

#[test]
fn two_runs_in_different_dirs_are_byte_identical() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir_a, target_a) = fresh("run-a");
    let (dir_b, target_b) = fresh("run-b");
    let first = check(&nu, &dir_a, &target_a, &[]);
    let second = check(&nu, &dir_b, &target_b, &[]);
    assert_eq!(first, second);
    for word in [
        "cargo-tops-style",
        "/tmp",
        "/var/folders",
        "/Users",
        "/home",
    ] {
        assert!(!first.contains(word), "output names a path: {word}");
    }
}

#[test]
fn another_nushell_minor_is_refused() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir, target) = fresh("minor");
    let version = Command::new(&nu)
        .arg("--version")
        .output()
        .expect("nu --version");
    let found = String::from_utf8(version.stdout)
        .expect("utf-8")
        .trim()
        .to_string();
    let minor: u32 = found
        .split('.')
        .nth(1)
        .expect("minor")
        .parse()
        .expect("number");
    let wrong = format!("0.{}.0", if minor == 0 { 1 } else { minor - 1 });
    fs::write(
        dir.join("rust-tops.lock"),
        format!("[tools]\nnushell = \"{wrong}\"\n"),
    )
    .expect("lock");
    let doc = json(&check(&nu, &dir, &target, &[]));
    assert_eq!(
        doc.get("ok"),
        Some(&serde_json::Value::Bool(false)),
        "{doc}"
    );
    assert_eq!(
        doc.get("error").and_then(serde_json::Value::as_str),
        Some(format!("nu {wrong} required, found {found}").as_str())
    );
    assert!(doc.get("data").is_none(), "{doc}");
}

#[test]
fn a_broken_build_is_not_ok() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir, target) = fresh("broken");
    let lib = dir.join("src/lib.rs");
    let text = fs::read_to_string(&lib).expect("lib.rs");
    fs::write(&lib, format!("{text}\npub fn (\n")).expect("break");
    let doc = json(&check(&nu, &dir, &target, &[]));
    assert_eq!(
        doc.get("ok"),
        Some(&serde_json::Value::Bool(false)),
        "{doc}"
    );
    let error = doc
        .get("error")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    assert!(error.starts_with("build failed: "), "{doc}");
    assert!(doc.get("data").is_none(), "{doc}");
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

#[test]
fn base_keeps_only_findings_on_changed_lines() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir, target) = fresh("base");
    fs::write(dir.join(".gitignore"), "target\n").expect("gitignore");
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "base"]);
    let lib = dir.join("src/lib.rs");
    let text = fs::read_to_string(&lib).expect("lib.rs");
    let line = text.lines().count() as u64 + 2;
    fs::write(
        &lib,
        format!("{text}\npub fn added(v: &[i32]) -> i32 {{ *v.first().unwrap() }}\n"),
    )
    .expect("edit");
    git(&dir, &["commit", "-aqm", "change"]);

    let all = findings(&json(&check(&nu, &dir, &target, &[])));
    assert!(all.len() > 1, "the fixture should have several findings");
    let doc = json(&check(&nu, &dir, &target, &["--base", "HEAD~1"]));
    let kept = findings(&doc);
    assert!(!kept.is_empty(), "{doc}");
    assert!(
        kept.iter().all(|f| field(f, "line").as_u64() == Some(line)),
        "{doc}"
    );
    let unwrap_id = catalog()
        .iter()
        .find(|r| rule_str(r, "check") == "clippy::unwrap_used")
        .map(|r| rule_str(r, "id").to_string())
        .expect("the catalog has a clippy::unwrap_used rule");
    assert!(
        kept.iter()
            .any(|f| field(f, "rule").as_str() == Some(unwrap_id.as_str())),
        "{doc}"
    );
}

#[test]
fn the_script_uses_builtins_for_file_work() {
    let text = fs::read_to_string(script()).expect("script");
    assert!(text.starts_with("#!/usr/bin/env nu\n"));
    for tool in ["^grep", "^sed", "^find", "^awk", "^rm"] {
        assert!(!text.contains(tool), "style script calls {tool}");
    }
}

#[test]
fn a_member_crate_path_gives_the_same_findings_and_drops_the_rest() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir, target) = fresh("member");
    let ws = dir.with_file_name("ws");
    let member = ws.join("member");
    fs::create_dir_all(ws.join("other/src")).expect("mkdir");
    fs::rename(&dir, &member).expect("move the fixture into a workspace");
    fs::write(
        ws.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"member\", \"other\"]\n",
    )
    .expect("workspace manifest");
    fs::write(
        ws.join("other/Cargo.toml"),
        "[package]\nname = \"other\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .expect("other manifest");
    fs::write(
        ws.join("other/src/lib.rs"),
        "pub fn out(v: &[i32]) -> i32 { *v.first().unwrap() }\n",
    )
    .expect("other lib");
    let from_member = check(&nu, &member, &target, &[]);
    let golden = manifest().join("tests/fixtures/style/expected.json");
    assert_eq!(from_member, fs::read_to_string(golden).expect("golden"));
    let from_root = json(&check(&nu, &ws, &target, &[]));
    let in_other = findings(&from_root)
        .iter()
        .any(|f| field(f, "file").as_str() == Some("other/src/lib.rs"));
    assert!(
        in_other,
        "a run at the workspace root sees the other member"
    );
}

#[test]
fn a_denied_warnings_group_still_reports_findings() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir, target) = fresh("deny");
    fs::write(
        dir.join("src/lib.rs"),
        "#![deny(warnings)]\n\n/// Returns the first element.\npub fn first(v: &[i32]) -> i32 {\n    *v.first().unwrap()\n}\n",
    )
    .expect("lib.rs");
    let doc = json(&check(&nu, &dir, &target, &[]));
    assert_eq!(
        doc.get("ok"),
        Some(&serde_json::Value::Bool(false)),
        "{doc}"
    );
    assert!(doc.get("error").is_none(), "no build error: {doc}");
    let ids: Vec<String> = findings(&doc)
        .iter()
        .map(|f| field(f, "rule").as_str().unwrap_or("").to_string())
        .collect();
    let unwrap_id = catalog()
        .iter()
        .find(|r| rule_str(r, "check") == "clippy::unwrap_used")
        .map(|r| rule_str(r, "id").to_string())
        .expect("the catalog has a clippy::unwrap_used rule");
    assert!(ids.contains(&"IDIOM-1".to_string()), "{ids:?}");
    assert!(ids.contains(&unwrap_id), "{ids:?}");
}
