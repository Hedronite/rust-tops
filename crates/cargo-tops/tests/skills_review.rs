//! `rust-tops-review.nu`: `files` and `diff` stay byte-identical, `style` limits
//! the style findings to the review range, and `answers` checks the judgment IDs.
//! Each test builds its own repo in a temp dir and runs the template scripts.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

type Json = serde_json::Value;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn templates() -> PathBuf {
    manifest().join("templates/skills")
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
        eprintln!("nu is not installed; review skill tests skipped");
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

fn temp(label: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("cargo-tops-review-{label}-{}", std::process::id()));
    drop(fs::remove_dir_all(&root));
    fs::create_dir_all(&root).expect("mkdir");
    root
}

/// The two skill dirs side by side, as `init --skills` writes them.
fn skills_tree(root: &Path) -> PathBuf {
    let skills = root.join("skills");
    for name in ["rust-tops-review", "rust-tops-style"] {
        copy_dir(&templates().join(name), &skills.join(name));
    }
    skills.join("rust-tops-review/rust-tops-review.nu")
}

fn run(nu: &Path, script: &Path, dir: &Path, args: &[&str], target: &Path) -> String {
    let out = Command::new(nu)
        .arg(script)
        .args(args)
        .current_dir(dir)
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

fn json(text: &str) -> Json {
    serde_json::from_str(text.trim()).expect("the script prints one JSON object")
}

/// `doc` at a key path, or null.
fn at<'a>(doc: &'a Json, path: &[&str]) -> &'a Json {
    path.iter()
        .try_fold(doc, |node, key| node.get(*key))
        .unwrap_or(&Json::Null)
}

fn ok(doc: &Json) -> Option<bool> {
    doc.get("ok").and_then(Json::as_bool)
}

// --- byte identity of `files` and `diff` ----------------------------------

/// A repo with a base branch, one commit on top, a tracked edit and an untracked file.
fn plain_repo(root: &Path) -> PathBuf {
    let repo = root.join("repo");
    fs::create_dir_all(&repo).expect("mkdir");
    git(&repo, &["init", "-q", "-b", "main"]);
    fs::write(repo.join("a.rs"), "fn a() {}\n").expect("a.rs");
    fs::write(repo.join("b.txt"), "one\ntwo\n").expect("b.txt");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    git(&repo, &["branch", "base"]);
    fs::write(repo.join("a.rs"), "fn a() {}\nfn b() {}\n").expect("a.rs");
    fs::write(repo.join("c.rs"), "fn c() {}\n").expect("c.rs");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "change"]);
    fs::write(repo.join("b.txt"), "one\ntwo\nthree\n").expect("b.txt");
    fs::write(repo.join("u.rs"), "fn u() {}\n").expect("u.rs");
    repo
}

#[test]
fn files_and_diff_output_is_byte_identical_to_main() {
    let Some(nu) = nu_or_skip() else { return };
    let root = temp("identity");
    let repo = plain_repo(&root);
    let script = templates().join("rust-tops-review/rust-tops-review.nu");
    let fixtures = manifest().join("tests/fixtures/review");
    for command in ["files", "diff"] {
        for (suffix, extra) in [("", None), ("-committed", Some("--committed-only"))] {
            let mut args = vec![command, "--base", "base", "--path", "."];
            args.extend(extra);
            let stdout = run(&nu, &script, &repo, &args, &root.join("target"));
            let golden = fixtures.join(format!("{command}{suffix}.golden.json"));
            if std::env::var_os("RUST_TOPS_BLESS").is_some() {
                fs::create_dir_all(&fixtures).expect("mkdir fixtures");
                fs::write(&golden, &stdout).expect("bless");
            }
            let want = fs::read_to_string(&golden).expect("golden");
            assert_eq!(stdout, want, "{command}{suffix}");
        }
    }
}

// --- `style` ------------------------------------------------------------------

fn catalog() -> Vec<toml::Value> {
    let path = templates().join("rust-tops-style/rules.toml");
    let doc: toml::Table = fs::read_to_string(path)
        .expect("rules.toml")
        .parse()
        .expect("rules.toml parses");
    doc.get("rule")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn text<'a>(rule: &'a toml::Value, key: &str) -> &'a str {
    rule.get(key).and_then(toml::Value::as_str).unwrap_or("")
}

fn review_judgment_ids() -> Vec<String> {
    let mut ids: Vec<String> = catalog()
        .iter()
        .filter(|r| text(r, "kind") == "judgment")
        .filter(|r| {
            r.get("skills")
                .and_then(toml::Value::as_array)
                .is_some_and(|s| s.iter().any(|v| v.as_str() == Some("review")))
        })
        .map(|r| text(r, "id").to_string())
        .collect();
    ids.sort();
    ids
}

fn unwrap_rule_id() -> String {
    catalog()
        .iter()
        .find(|r| text(r, "check") == "clippy::unwrap_used")
        .map(|r| text(r, "id").to_string())
        .expect("the catalog has a clippy::unwrap_used rule")
}

/// The style fixture crate, committed; then one commit adds an unwrap, and the
/// working tree adds another plus an untracked module with a third.
fn style_repo(root: &Path) -> PathBuf {
    let repo = root.join("repo");
    copy_dir(&manifest().join("tests/fixtures/style/crate"), &repo);
    fs::rename(repo.join("fixture-Cargo.toml"), repo.join("Cargo.toml")).expect("Cargo.toml");
    fs::write(repo.join(".gitignore"), "target\n").expect("gitignore");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    git(&repo, &["branch", "base"]);
    let lib = repo.join("src/lib.rs");
    let body = fs::read_to_string(&lib).expect("lib.rs");
    let committed =
        format!("{body}\npub fn committed(v: &[i32]) -> i32 {{ *v.first().unwrap() }}\n");
    fs::write(&lib, &committed).expect("lib.rs");
    git(&repo, &["commit", "-aqm", "change"]);
    let working = format!(
        "{committed}\nmod extra;\npub fn working(v: &[i32]) -> i32 {{ *v.last().unwrap() }}\n"
    );
    fs::write(&lib, working).expect("lib.rs");
    fs::write(
        repo.join("src/extra.rs"),
        "pub fn extra(v: &[i32]) -> i32 {\n    *v.first().unwrap()\n}\n",
    )
    .expect("extra.rs");
    repo
}

fn findings(doc: &Json) -> Vec<(String, u64)> {
    doc.get("data")
        .and_then(|d| d.get("findings"))
        .and_then(Json::as_array)
        .map(|all| {
            all.iter()
                .map(|f| {
                    (
                        f.get("file")
                            .and_then(Json::as_str)
                            .unwrap_or("")
                            .to_string(),
                        f.get("line").and_then(Json::as_u64).unwrap_or(0),
                    )
                })
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn style_reports_only_findings_inside_the_review_range() {
    let Some(nu) = nu_or_skip() else { return };
    let root = temp("style");
    let repo = style_repo(&root);
    let script = skills_tree(&root);
    let lines = fs::read_to_string(repo.join("src/lib.rs"))
        .expect("lib.rs")
        .lines()
        .count() as u64;
    let target = root.join("target");
    let args = ["style", "--base", "base", "--path", "."];

    let all = json(&run(&nu, &script, &repo, &args, &target));
    assert_eq!(ok(&all), Some(false), "{all}");
    let want = vec![
        ("src/extra.rs".to_string(), 2),
        ("src/lib.rs".to_string(), lines - 3),
        ("src/lib.rs".to_string(), lines),
    ];
    assert_eq!(findings(&all), want, "{all}");
    let rule = unwrap_rule_id();
    assert!(all.to_string().contains(&rule));

    let committed = json(&run(
        &nu,
        &script,
        &repo,
        &[&args[..], &["--committed-only"]].concat(),
        &target,
    ));
    assert_eq!(
        findings(&committed),
        vec![("src/lib.rs".to_string(), lines - 3)],
        "{committed}"
    );

    let ids: Vec<&str> = at(&all, &["data", "judgment"])
        .as_array()
        .expect("judgment")
        .iter()
        .filter_map(|j| j.get("id").and_then(Json::as_str))
        .collect();
    assert_eq!(ids, review_judgment_ids());
}

#[test]
fn style_runs_in_different_dirs_are_byte_identical() {
    let Some(nu) = nu_or_skip() else { return };
    let (root_a, root_b) = (temp("style-a"), temp("style-b"));
    let (repo_a, repo_b) = (style_repo(&root_a), style_repo(&root_b));
    let args = ["style", "--base", "base", "--path", "."];
    let first = run(
        &nu,
        &skills_tree(&root_a),
        &repo_a,
        &args,
        &root_a.join("target"),
    );
    let second = run(
        &nu,
        &skills_tree(&root_b),
        &repo_b,
        &args,
        &root_b.join("target"),
    );
    assert_eq!(first, second);
    for word in [
        "cargo-tops-review",
        "/tmp",
        "/var/folders",
        "/Users",
        "/home",
    ] {
        assert!(!first.contains(word), "output names a path: {word}");
    }
}

#[test]
fn style_refuses_another_nushell_minor() {
    let Some(nu) = nu_or_skip() else { return };
    let root = temp("style-minor");
    let repo = style_repo(&root);
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
        repo.join("rust-tops.lock"),
        format!("[tools]\nnushell = \"{wrong}\"\n"),
    )
    .expect("lock");
    let args = ["style", "--base", "base", "--path", "."];
    let refused = json(&run(
        &nu,
        &skills_tree(&root),
        &repo,
        &args,
        &root.join("target"),
    ));
    assert_eq!(ok(&refused), Some(false), "{refused}");
    assert_eq!(
        refused.get("error").and_then(Json::as_str),
        Some(format!("nu {wrong} required, found {found}").as_str())
    );
}

// --- `answers` ----------------------------------------------------------------

fn answers(nu: &Path, label: &str, body: &str) -> (String, Json) {
    let root = temp(label);
    let script = skills_tree(&root);
    fs::write(root.join("answers.toml"), body).expect("answers");
    let stdout = run(
        nu,
        &script,
        &root,
        &["answers", "answers.toml"],
        &root.join("target"),
    );
    let doc = json(&stdout);
    (stdout, doc)
}

fn entries(ids: &[String], status: &str, note: &str) -> String {
    ids.iter()
        .map(|id| format!("\"{id}\" = {{ status = \"{status}\", note = \"{note}\" }}\n"))
        .collect()
}

fn problems(doc: &Json) -> Vec<(String, String)> {
    at(doc, &["data", "problems"])
        .as_array()
        .map(|all| {
            all.iter()
                .map(|p| {
                    let get = |k: &str| p.get(k).and_then(Json::as_str).unwrap_or("").to_string();
                    (get("id"), get("problem"))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn answered(nu: &Path, label: &str, body: &str) -> Json {
    answers(nu, label, &format!("[answers]\n{body}")).1
}

#[test]
fn answers_accepts_a_complete_file_and_prints_no_verdict() {
    let Some(nu) = nu_or_skip() else { return };
    let ids = review_judgment_ids();
    assert!(!ids.is_empty(), "the catalog has review judgment IDs");
    let (stdout, doc) = answers(
        &nu,
        "answers-ok",
        &format!("[answers]\n{}", entries(&ids, "ok", "")),
    );
    assert_eq!(ok(&doc), Some(true), "{doc}");
    assert!(!stdout.to_lowercase().contains("verdict"), "{stdout}");
    let noted = answered(
        &nu,
        "answers-noted",
        &entries(&ids, "finding", "returns early"),
    );
    assert_eq!(ok(&noted), Some(true), "{noted}");
}

#[test]
fn answers_rejects_a_missing_id() {
    let Some(nu) = nu_or_skip() else { return };
    let ids = review_judgment_ids();
    let first = ids.first().expect("an id");
    let doc = answered(
        &nu,
        "answers-missing",
        &entries(ids.get(1..).unwrap_or_default(), "ok", ""),
    );
    assert_eq!(ok(&doc), Some(false), "{doc}");
    assert_eq!(problems(&doc), vec![(first.clone(), "missing".to_string())]);
}

#[test]
fn answers_rejects_an_unknown_id() {
    let Some(nu) = nu_or_skip() else { return };
    let ids = review_judgment_ids();
    let body = format!(
        "{}\"ZZZ-9\" = {{ status = \"ok\", note = \"\" }}\n",
        entries(&ids, "ok", "")
    );
    let doc = answered(&nu, "answers-unknown", &body);
    assert_eq!(ok(&doc), Some(false), "{doc}");
    assert_eq!(
        problems(&doc),
        vec![("ZZZ-9".to_string(), "unknown".to_string())]
    );
}

#[test]
fn answers_rejects_a_finding_with_an_empty_note_and_a_bad_status() {
    let Some(nu) = nu_or_skip() else { return };
    let ids = review_judgment_ids();
    let empty = answered(&nu, "answers-empty", &entries(&ids, "finding", "  "));
    assert_eq!(ok(&empty), Some(false), "{empty}");
    assert_eq!(problems(&empty).len(), ids.len(), "{empty}");
    let bad = answered(&nu, "answers-status", &entries(&ids, "maybe", ""));
    assert_eq!(ok(&bad), Some(false), "{bad}");
}
