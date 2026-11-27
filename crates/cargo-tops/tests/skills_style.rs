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
    Command::new("sh")
        .args(["-c", "command -v nu"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
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

/// The running nu's version, as `(major, minor, text)`.
fn nu_version(nu: &Path) -> (u32, u32, String) {
    let out = Command::new(nu)
        .arg("--version")
        .output()
        .expect("nu --version");
    let text = String::from_utf8(out.stdout)
        .expect("utf-8")
        .trim()
        .to_string();
    let mut parts = text.split('.').map(|p| p.parse().expect("number"));
    let (major, minor) = (parts.next().expect("major"), parts.next().expect("minor"));
    (major, minor, text)
}

/// `check` on a copy of the fixture whose lock asks for `required`.
fn check_with_lock(nu: &Path, label: &str, required: &str) -> serde_json::Value {
    let (dir, target) = fresh(label);
    let lock = format!("[tools]\nnushell = \"{required}\"\n");
    fs::write(dir.join("rust-tops.lock"), lock).expect("lock");
    json(&check(nu, &dir, &target, &[]))
}

#[test]
fn an_older_minor_or_another_major_of_nushell_is_refused() {
    let Some(nu) = nu_or_skip() else { return };
    let (major, minor, found) = nu_version(&nu);
    let newer_minor = format!("{major}.{}.0", minor + 1);
    let next_major = format!("{}.0.0", major + 1);
    for (label, required) in [("older", newer_minor), ("major", next_major)] {
        let doc = check_with_lock(&nu, label, &required);
        assert_eq!(doc.get("ok"), Some(&serde_json::Value::Bool(false)));
        assert_eq!(
            doc.get("error").and_then(serde_json::Value::as_str),
            Some(format!("nu {required} required, found {found}").as_str())
        );
        assert!(doc.get("data").is_none(), "{doc}");
    }
}

#[test]
fn a_newer_minor_of_the_same_major_runs() {
    let Some(nu) = nu_or_skip() else { return };
    let (major, minor, _) = nu_version(&nu);
    let older = format!("{major}.{}.0", minor.saturating_sub(1));
    let doc = check_with_lock(&nu, "newer", &older);
    assert!(doc.get("error").is_none(), "{doc}");
    assert!(!findings(&doc).is_empty(), "{doc}");
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
    opt_in(&dir);
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

/// The `- `ID`: text` lines of the SKILL.md judgment list.
fn skill_judgment_lines(skill: &str) -> Vec<(String, String)> {
    skill
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("- `")?;
            let (id, text) = rest.split_once("`: ")?;
            let looks_like_id = id.split_once('-').is_some_and(|(head, tail)| {
                !head.is_empty()
                    && head.chars().all(|ch| ch.is_ascii_uppercase())
                    && tail.chars().all(|ch| ch.is_ascii_digit())
                    && !tail.is_empty()
            });
            looks_like_id.then(|| (id.to_owned(), text.to_owned()))
        })
        .collect()
}

/// Every way the SKILL.md judgment list disagrees with the style judgment
/// rules of the catalog: an ID or text the catalog lacks, or an ID it misses.
fn judgment_drift(skill: &str, rules: &[toml::Value]) -> Vec<String> {
    let lines = skill_judgment_lines(skill);
    let judgment: Vec<(&str, &str)> = rules
        .iter()
        .filter(|rule| rule_str(rule, "kind") == "judgment" && for_style(rule))
        .map(|rule| (rule_str(rule, "id"), rule_str(rule, "rule")))
        .collect();
    let mut problems = Vec::new();
    for (id, text) in &lines {
        match judgment.iter().find(|(known, _)| known == id) {
            None => problems.push(format!("SKILL.md names {id}, which the catalog lacks")),
            Some((_, rule)) if rule != text => {
                problems.push(format!("SKILL.md text for {id} differs from the catalog"));
            }
            Some(_) => {}
        }
    }
    for (id, _) in &judgment {
        if !lines.iter().any(|(listed, _)| listed == id) {
            problems.push(format!("SKILL.md misses the style judgment {id}"));
        }
    }
    problems
}

fn style_skill() -> String {
    let path = manifest().join("templates/skills/rust-tops-style/SKILL.md");
    fs::read_to_string(path).expect("style SKILL.md")
}

#[test]
fn the_style_skill_judgment_list_matches_the_catalog() {
    let skill = style_skill();
    assert!(
        skill_judgment_lines(&skill).len() >= 10,
        "the judgment list was not found in SKILL.md"
    );
    assert_eq!(judgment_drift(&skill, &catalog()), Vec::<String>::new());
}

#[test]
fn a_planted_judgment_drift_is_reported() {
    let skill = style_skill();
    let rules = catalog();
    let (id, text) = skill_judgment_lines(&skill).remove(0);
    let renamed = skill.replace(
        &format!("- `{id}`: {text}"),
        "- `ZZZ-9`: invented rule text",
    );
    let drift = judgment_drift(&renamed, &rules);
    assert!(drift.iter().any(|p| p.contains("ZZZ-9")), "{drift:?}");
    assert!(
        drift
            .iter()
            .any(|p| p.contains(&format!("misses the style judgment {id}"))),
        "{drift:?}"
    );
    let reworded = skill.replace(&text, &format!("{text} Changed."));
    let drift = judgment_drift(&reworded, &rules);
    assert!(
        drift
            .iter()
            .any(|p| p.contains(&format!("text for {id} differs"))),
        "{drift:?}"
    );
    let dropped = skill.replace(&format!("- `{id}`: {text}\n"), "");
    let drift = judgment_drift(&dropped, &rules);
    assert!(
        drift
            .iter()
            .any(|p| p.contains(&format!("misses the style judgment {id}"))),
        "{drift:?}"
    );
}

// --- writer mode (`--base`) --------------------------------------------------

/// Turn `unwrap_used` and `indexing_slicing` on in the crate's own lint table.
fn opt_in(dir: &Path) {
    let manifest = dir.join("Cargo.toml");
    let text = fs::read_to_string(&manifest).expect("Cargo.toml");
    let table = "unwrap_used = \"warn\"\nindexing_slicing = \"warn\"\n";
    let text = if text.contains("[lints.clippy]") {
        text.replace("[lints.clippy]\n", &format!("[lints.clippy]\n{table}"))
    } else {
        format!("{text}\n[lints.clippy]\n{table}")
    };
    fs::write(manifest, text).expect("Cargo.toml");
}

fn put(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

const SMALL_LIB: &str = "//! Fixture.\n\n/// First.\npub fn first(v: &[i32]) -> i32 {\n    v.first().copied().unwrap_or(0)\n}\n\n/// Gone.\npub fn gone() {}\n";

const SMALL_EDIT: &str =
    "//! Fixture.\n\n/// First.\npub fn first(v: &[i32]) -> i32 {\n    *v.first().unwrap()\n}\n";

/// A small committed library in its own temp dir, plus a target dir.
fn small(label: &str) -> (PathBuf, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("cargo-tops-style-{label}-{}", std::process::id()));
    drop(fs::remove_dir_all(&root));
    let dir = root.join("crate");
    let manifest = "[package]\nname = \"small\"\nversion = \"0.0.0\"\nedition = \"2021\"\n";
    put(&dir, "Cargo.toml", manifest);
    put(&dir, ".gitignore", "target\nCargo.lock\n");
    put(&dir, "src/lib.rs", SMALL_LIB);
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "base"]);
    (dir, root.join("target"))
}

/// `(rule, file, line)` of every finding.
fn places(doc: &serde_json::Value) -> Vec<(String, String, u64)> {
    assert!(doc.get("error").is_none(), "the check did not run: {doc}");
    findings(doc)
        .iter()
        .map(|f| {
            let text = |key| field(f, key).as_str().unwrap_or("").to_string();
            let line = field(f, "line").as_u64().unwrap_or(0);
            (text("rule"), text("file"), line)
        })
        .collect()
}

fn place(rule: &str, file: &str, line: u64) -> (String, String, u64) {
    (rule.to_string(), file.to_string(), line)
}

fn judgment_ids(doc: &serde_json::Value) -> Vec<String> {
    doc.pointer("/data/judgment")
        .and_then(serde_json::Value::as_array)
        .expect("judgment")
        .iter()
        .filter_map(|j| j.get("id").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect()
}

fn command(doc: &serde_json::Value) -> &str {
    doc.get("command")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

/// The hunk headers `git diff -U0 HEAD` prints.
fn hunks(dir: &Path) -> Vec<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["diff", "-U0", "HEAD"])
        .output()
        .expect("git diff");
    String::from_utf8(out.stdout)
        .expect("utf-8")
        .lines()
        .filter(|l| l.starts_with("@@ "))
        .map(|l| l.split(" @@").next().unwrap_or(l).to_string())
        .collect()
}

#[test]
fn count_less_and_deletion_hunks_give_json_and_exact_findings() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir, target) = small("hunks");
    opt_in(&dir);
    git(&dir, &["commit", "-aqm", "opt in"]);
    put(&dir, "src/lib.rs", SMALL_EDIT);
    put(&dir, "src/bin/tool.rs", "fn main() {}\n");
    git(&dir, &["add", "."]);
    assert_eq!(
        hunks(&dir),
        ["@@ -0,0 +1", "@@ -5 +5", "@@ -7,3 +6,0"],
        "one added line, one edited line, a pure deletion"
    );
    let want = vec![
        place("DOC-1", "src/bin/tool.rs", 1),
        place("PANIC-1", "src/lib.rs", 5),
    ];
    let doc = json(&check(&nu, &dir, &target, &["--base", "HEAD"]));
    assert_eq!(places(&doc), want, "{doc}");
    git(&dir, &["commit", "-qm", "change"]);
    let args = ["--base", "HEAD~1", "--committed-only"];
    let doc = json(&check(&nu, &dir, &target, &args));
    assert_eq!(places(&doc), want, "{doc}");
}

#[test]
fn base_sees_unstaged_and_untracked_lines_unless_committed_only() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir, target) = small("worktree");
    opt_in(&dir);
    git(&dir, &["commit", "-aqm", "opt in"]);
    // A user's diff settings must not hide the paths.
    git(&dir, &["config", "diff.mnemonicPrefix", "true"]);
    put(&dir, "src/lib.rs", SMALL_EDIT);
    put(
        &dir,
        "src/bin/extra.rs",
        "//! Extra.\nfn main() {\n    let v = [1];\n    let _ = v.first().unwrap();\n}\n",
    );
    let doc = json(&check(&nu, &dir, &target, &["--base", "HEAD"]));
    let want = vec![
        place("PANIC-1", "src/bin/extra.rs", 4),
        place("PANIC-1", "src/lib.rs", 5),
    ];
    assert_eq!(places(&doc), want, "{doc}");
    let args = ["--base", "HEAD", "--committed-only"];
    let doc = json(&check(&nu, &dir, &target, &args));
    assert_eq!(places(&doc), Vec::new(), "{doc}");
    assert_eq!(judgment_ids(&doc), Vec::<String>::new(), "{doc}");
}

const WITH_TESTS: &str = "//! Fixture.\n\n/// First.\npub fn first(v: &[i32]) -> i32 {\n    v.first().copied().unwrap_or(0)\n}\n\n/// Gone.\npub fn gone() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn first_of_one() {\n        let v = vec![1];\n        assert_eq!(*v.first().unwrap(), v[0]);\n    }\n}\n";

const TEST_FILE: &str = "//! Journey.\n#[test]\nfn journey() {\n    let v = vec![1];\n    assert_eq!(*v.first().unwrap(), v[0]);\n}\n";

#[test]
fn a_tests_only_change_is_ok_and_asks_nothing() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir, target) = small("tests-only");
    opt_in(&dir);
    // A hit that was there before the change does not count against it.
    put(
        &dir,
        "src/lib.rs",
        &SMALL_LIB.replace("v.first().copied().unwrap_or(0)", "*v.first().unwrap()"),
    );
    git(&dir, &["commit", "-aqm", "an old unwrap"]);
    let old = fs::read_to_string(dir.join("src/lib.rs")).expect("lib.rs");
    let tests = WITH_TESTS.split_once("\n#[cfg(test)]").expect("tests").1;
    put(&dir, "src/lib.rs", &format!("{old}\n#[cfg(test)]{tests}"));
    put(&dir, "tests/journey.rs", TEST_FILE);
    let doc = json(&check(&nu, &dir, &target, &["--base", "HEAD"]));
    assert_eq!(doc.get("ok"), Some(&serde_json::Value::Bool(true)), "{doc}");
    assert_eq!(places(&doc), Vec::new(), "{doc}");
    assert_eq!(judgment_ids(&doc), Vec::<String>::new(), "{doc}");
}

#[test]
fn writer_mode_keeps_production_hits_and_audit_keeps_all() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir, target) = small("exempt");
    opt_in(&dir);
    git(&dir, &["commit", "-aqm", "opt in"]);
    put(
        &dir,
        "src/lib.rs",
        &WITH_TESTS.replace("v.first().copied().unwrap_or(0)", "v[0]"),
    );
    put(&dir, "tests/journey.rs", TEST_FILE);
    let panics = |extra: &[&str]| {
        let mut all = places(&json(&check(&nu, &dir, &target, extra)));
        all.retain(|(rule, _, _)| rule.starts_with("PANIC-"));
        all
    };
    assert_eq!(
        panics(&["--base", "HEAD"]),
        vec![place("PANIC-2", "src/lib.rs", 5)]
    );
    let audit = panics(&["--base", "HEAD", "--audit"]);
    assert!(
        audit.iter().any(|(_, file, _)| file == "tests/journey.rs"),
        "{audit:?}"
    );
    assert!(
        audit
            .iter()
            .any(|(_, file, line)| file == "src/lib.rs" && *line == 16),
        "{audit:?}"
    );
}

#[test]
fn writer_mode_follows_the_crates_lints() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir, target) = small("all-warn");
    let manifest = fs::read_to_string(dir.join("Cargo.toml")).expect("Cargo.toml");
    let lints =
        "[workspace]\n[workspace.lints.clippy]\nall = \"warn\"\n[lints]\nworkspace = true\n";
    put(&dir, "Cargo.toml", &format!("{manifest}{lints}"));
    git(&dir, &["commit", "-aqm", "all = warn"]);
    put(&dir, "src/lib.rs", SMALL_EDIT);
    let writer = json(&check(&nu, &dir, &target, &["--base", "HEAD"]));
    assert_eq!(places(&writer), Vec::new(), "{writer}");
    assert!(!command(&writer).contains("unwrap_used"), "{writer}");
    assert!(command(&writer).contains("-W clippy::ptr_arg"), "{writer}");
    let audit = json(&check(&nu, &dir, &target, &["--audit"]));
    assert!(
        command(&audit).contains("-W clippy::unwrap_used"),
        "{audit}"
    );
    assert!(
        places(&audit).contains(&place("PANIC-1", "src/lib.rs", 5)),
        "{audit}"
    );
    // The instance's deny list opts in too.
    let yaml = "gates:\n  lint:\n    clippy_deny: [\"clippy::unwrap_used\"]\n";
    put(&dir, "rust-tops.yaml", yaml);
    let opted = json(&check(&nu, &dir, &target, &["--base", "HEAD"]));
    assert_eq!(
        places(&opted),
        vec![place("PANIC-1", "src/lib.rs", 5)],
        "{opted}"
    );
}

#[test]
fn writer_judgment_follows_the_changed_lines() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir, target) = small("judgment");
    let added = "\n/// Copy.\npub fn copy(v: &[i32]) -> Vec<i32> {\n    v.to_vec().clone()\n}\n";
    put(&dir, "src/lib.rs", &format!("{SMALL_LIB}{added}"));
    let ids = judgment_ids(&json(&check(&nu, &dir, &target, &["--base", "HEAD"])));
    for want in ["API-4", "API-6", "OWN-2", "DOC-5"] {
        assert!(ids.iter().any(|id| id == want), "{want}: {ids:?}");
    }
    for unwanted in ["UNSAFE-5", "PANIC-7", "TRAIT-3", "CONC-3"] {
        assert!(!ids.iter().any(|id| id == unwanted), "{unwanted}: {ids:?}");
    }
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn every_judgment_rule_has_a_trigger() {
    let source = fs::read_to_string(script()).expect("script");
    let block = source.split_once("const TRIGGERS = [").expect("TRIGGERS").1;
    let block = block.split_once("\n]").expect("end").0;
    let mut have: Vec<&str> = block
        .lines()
        .filter_map(|l| l.trim().strip_prefix("{id: \""))
        .filter_map(|l| l.split('"').next())
        .collect();
    have.sort_unstable();
    let rules = catalog();
    let mut want: Vec<&str> = rules
        .iter()
        .filter(|r| rule_str(r, "kind") == "judgment" && for_style(r))
        .map(|r| rule_str(r, "id"))
        .collect();
    want.sort_unstable();
    assert_eq!(have, want);
}

#[test]
fn writer_output_is_byte_identical_and_vendor_is_skipped() {
    let Some(nu) = nu_or_skip() else { return };
    let run = |label: &str| {
        let (dir, target) = small(label);
        put(&dir, "src/lib.rs", SMALL_EDIT);
        put(&dir, "vendor/dep/src/lib.rs", "pub fn get_x(&self) {}\n");
        let writer = check(&nu, &dir, &target, &["--base", "HEAD"]);
        let audit = check(&nu, &dir, &target, &[]);
        assert!(!writer.contains("vendor/"), "{writer}");
        assert!(!audit.contains("vendor/"), "{audit}");
        writer
    };
    let first = run("same-a");
    assert_eq!(first, run("same-b"));
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
fn committed_only_needs_a_base_and_a_bad_base_is_not_ok() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir, target) = small("flags");
    for extra in [&["--committed-only"][..], &["--base", "no-such-ref"][..]] {
        let doc = json(&check(&nu, &dir, &target, extra));
        assert_eq!(
            doc.get("ok"),
            Some(&serde_json::Value::Bool(false)),
            "{doc}"
        );
        assert!(doc.get("error").is_some(), "{doc}");
        assert!(doc.get("data").is_none(), "{doc}");
    }
}

#[test]
fn a_changed_last_line_of_a_chain_is_found() {
    let Some(nu) = nu_or_skip() else { return };
    let (dir, target) = small("chain");
    opt_in(&dir);
    let chain = |last: &str| {
        let body = format!("    v.iter()\n        .copied()\n        .next()\n        {last}\n");
        SMALL_LIB.replace("    v.first().copied().unwrap_or(0)\n", &body)
    };
    put(&dir, "src/lib.rs", &chain(".unwrap_or(0)"));
    git(&dir, &["commit", "-aqm", "chain"]);
    put(&dir, "src/lib.rs", &chain(".unwrap()"));
    let doc = json(&check(&nu, &dir, &target, &["--base", "HEAD"]));
    assert_eq!(
        places(&doc),
        vec![place("PANIC-1", "src/lib.rs", 5)],
        "{doc}"
    );
    let found = findings(&doc);
    assert!(found.iter().all(|f| f.get("end").is_none()), "{doc}");
}
