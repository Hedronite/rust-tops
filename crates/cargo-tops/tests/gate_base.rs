//! The mutation layer diffs from a named base: `--base <ref>`, then
//! `gates.base_ref`, then `origin/HEAD`. A base that is named and does not
//! resolve fails and names the ref. With no base at all the layer is `fail`
//! under `CI` (naming `fetch-depth: 0` and `--base`) and `vacuous`
//! otherwise, listing each source tried. The report names the base and
//! where it came from.
//!
//! Each case is a real git repository. Cargo and its plugins are stubs that
//! log what they are asked, so a test sees whether `cargo mutants` ran and
//! on which diff. Every report is validated against the schema.
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
/// `cargo mutants` does what 27.1.0 does for a diff with nothing to mutate;
/// passes everything else.
const CARGO_STUB: &str = r#"#!/bin/sh
if [ -n "${CARGO_STUB_LOG:-}" ]; then echo "$*" >> "$CARGO_STUB_LOG"; fi
if [ "$1 $2" = "crap --version" ]; then echo "cargo-crap 0.9.0"; exit 0; fi
case "$*" in *--version) echo "stub-tool 1.2.3"; exit 0 ;; esac
# 27.1.0 finds nothing to mutate in a diff of comments, tests, docs or CI:
# it says so on stderr, exits 0 and writes no `mutants.out`.
if [ "$1" = "mutants" ]; then
  echo ' INFO No mutants to filter' >&2
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
fn clone_shape(fixture: &Fixture, branch: &str) {
    let bare = fixture.root.join("origin.git");
    git(
        fixture,
        &fixture.root,
        &["init", "-q", "--bare", "origin.git"],
    );
    let url = bare.to_string_lossy().into_owned();
    git(fixture, &fixture.repo, &["remote", "add", "origin", &url]);
    git(fixture, &fixture.repo, &["push", "-q", "origin", branch]);
    git(
        fixture,
        &fixture.repo,
        &["remote", "set-head", "origin", branch],
    );
}

/// `refs/remotes/origin/<name>` at `commit`.
fn remote_ref(fixture: &Fixture, name: &str, commit: &str) {
    let full = format!("refs/remotes/origin/{name}");
    git(fixture, &fixture.repo, &["update-ref", &full, commit]);
}

fn set_base_ref(fixture: &Fixture, base: &str) {
    edit(fixture, |doc| {
        let gates = doc.pointer_mut("/gates").expect("gates");
        set(gates, "base_ref", json!(base));
    });
}

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

fn cargo_log(fixture: &Fixture) -> String {
    fs::read_to_string(fixture.root.join("cargo.log")).unwrap_or_default()
}

/// Whether `cargo mutants` ran.
fn mutants_ran(fixture: &Fixture) -> bool {
    cargo_log(fixture)
        .lines()
        .any(|line| line.starts_with("mutants --in-diff "))
}

fn diff_text(fixture: &Fixture) -> String {
    fs::read_to_string(fixture.repo.join("target/rust-tops/rust-tops.diff")).unwrap_or_default()
}

/// A branch `feature` two commits ahead of `base`: `src/b.rs`, then
/// `src/c.rs`. Returns the commits `(base, after_b, after_c)`.
fn feature_branch(fixture: &Fixture) -> (String, String, String) {
    let base = git(fixture, &fixture.repo, &["rev-parse", "HEAD"]);
    git(fixture, &fixture.repo, &["checkout", "-q", "-b", "feature"]);
    let after_b = commit_file(fixture, "b.rs");
    let after_c = commit_file(fixture, "c.rs");
    (base, after_b, after_c)
}

#[test]
fn a_default_branch_called_trunk_runs_mutants() {
    let fixture = fixture("trunk", "trunk");
    clone_shape(&fixture, "trunk");
    feature_branch(&fixture);
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(mutants_ran(&fixture), "{}", cargo_log(&fixture));
    let diff = diff_text(&fixture);
    assert!(
        diff.contains("src/b.rs") && diff.contains("src/c.rs"),
        "{diff}"
    );
    let report = report(&fixture);
    let row = mutation(&report);
    assert_eq!(at(row, "/verdict"), "pass", "{row}");
    assert_eq!(
        at(row, "/judged/base"),
        &json!({ "ref": "origin/trunk", "source": "origin/HEAD" }),
        "{row}"
    );
    assert!(at(row, "/detail").is_null(), "{row}");
    assert!(!text.contains("origin/main"), "{text}");
}

/// Three remote refs on one history: `origin/main` before the two commits,
/// `origin/one` after the first, `origin/two` at `HEAD`. `origin/HEAD` is
/// `origin/main`.
fn three_bases(name: &str) -> Fixture {
    let fixture = fixture(name, "main");
    clone_shape(&fixture, "main");
    let (_, after_b, after_c) = feature_branch(&fixture);
    remote_ref(&fixture, "one", &after_b);
    remote_ref(&fixture, "two", &after_c);
    fixture
}

fn touched(diff: &str) -> (bool, bool) {
    (diff.contains("src/b.rs"), diff.contains("src/c.rs"))
}

#[test]
fn origin_head_is_the_base_when_nothing_else_names_one() {
    let fixture = three_bases("from-head");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert_eq!(touched(&diff_text(&fixture)), (true, true));
    let report = report(&fixture);
    assert_eq!(
        at(mutation(&report), "/judged/base"),
        &json!({ "ref": "origin/main", "source": "origin/HEAD" })
    );
}

#[test]
fn the_key_beats_origin_head() {
    let fixture = three_bases("key-over-head");
    set_base_ref(&fixture, "origin/one");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert_eq!(touched(&diff_text(&fixture)), (false, true), "{text}");
    let report = report(&fixture);
    assert_eq!(
        at(mutation(&report), "/judged/base"),
        &json!({ "ref": "origin/one", "source": "key" })
    );
}

#[test]
fn the_flag_beats_the_key_and_origin_head() {
    let fixture = three_bases("flag-over-key");
    // The key alone would leave an empty diff; the flag gives a real one.
    set_base_ref(&fixture, "origin/two");
    let (out, text) = gate(&fixture, &["--base", "origin/one"], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(mutants_ran(&fixture), "{}", cargo_log(&fixture));
    assert_eq!(touched(&diff_text(&fixture)), (false, true), "{text}");
    let report = report(&fixture);
    assert_eq!(
        at(mutation(&report), "/judged/base"),
        &json!({ "ref": "origin/one", "source": "flag" })
    );
}

#[test]
fn a_base_at_head_is_an_empty_diff_and_vacuous() {
    let fixture = three_bases("empty");
    let (out, text) = gate(&fixture, &["--base", "origin/two"], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("Vacuous: the diff from `git merge-base origin/two HEAD` is empty"),
        "{text}"
    );
    assert!(!mutants_ran(&fixture));
    let report = report(&fixture);
    assert_eq!(at(mutation(&report), "/verdict"), "vacuous");
}

#[test]
fn a_tag_or_a_commit_is_a_base() {
    let fixture = three_bases("tag-and-sha");
    let one = git(&fixture, &fixture.repo, &["rev-parse", "origin/one"]);
    git(&fixture, &fixture.repo, &["tag", "v0", &one]);
    for named in ["v0", one.as_str()] {
        let (out, text) = gate(&fixture, &["--base", named], None);
        assert_eq!(out.status.code(), Some(0), "{named}: {text}");
        assert_eq!(touched(&diff_text(&fixture)), (false, true), "{named}");
        let report = report(&fixture);
        assert_eq!(at(mutation(&report), "/judged/base/ref"), named);
    }
}

fn expect_unresolved(fixture: &Fixture, text: &str, out: &Output, name: &str, source: &str) {
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains(name), "{text}");
    assert!(
        text.lines()
            .any(|line| line.starts_with("Failed:") && line.contains(name)),
        "{text}"
    );
    assert!(!mutants_ran(fixture), "{}", cargo_log(fixture));
    let report = report(fixture);
    let row = mutation(&report);
    assert_eq!(at(row, "/verdict"), "fail", "{row}");
    assert!(
        at(row, "/detail")
            .as_str()
            .is_some_and(|d| d.contains(name)),
        "{row}"
    );
    assert_eq!(
        at(row, "/judged/base"),
        &json!({ "ref": name, "source": source, "resolved": false }),
        "{row}"
    );
    // Git never ran: the problem says what happened, and no command is
    // reported as failed or counted as run.
    let problems = mutation_problems(&report);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems
            .iter()
            .all(|p| p.contains(name) && !p.contains("failed") && !p.contains("merge-base")),
        "{problems:?}"
    );
    assert!(
        at(&report, "/count")
            .as_str()
            .is_some_and(|c| c.contains("none ran for: mutation")),
        "{report}"
    );
}

#[test]
fn a_named_base_that_does_not_resolve_fails_naming_it() {
    let fixture = three_bases("nope-flag");
    let (out, text) = gate(&fixture, &["--base", "nope"], None);
    expect_unresolved(&fixture, &text, &out, "nope", "flag");
    assert!(text.contains("--base names `nope`"), "{text}");
}

#[test]
fn a_named_key_that_does_not_resolve_fails_naming_it() {
    let fixture = three_bases("nope-key");
    set_base_ref(&fixture, "origin/gone");
    let (out, text) = gate(&fixture, &[], None);
    expect_unresolved(&fixture, &text, &out, "origin/gone", "key");
    assert!(
        text.contains("gates.base_ref names `origin/gone`"),
        "{text}"
    );
}

#[test]
fn a_named_base_is_never_replaced_by_a_later_source() {
    // The key and `origin/HEAD` would both resolve. The flag does not.
    let fixture = three_bases("nope-over-key");
    set_base_ref(&fixture, "origin/one");
    let (out, text) = gate(&fixture, &["--base", "nope"], None);
    expect_unresolved(&fixture, &text, &out, "nope", "flag");
}

#[test]
fn a_base_that_starts_with_a_dash_is_not_a_ref() {
    let fixture = three_bases("dash");
    let (out, text) = gate(&fixture, &["--base=-x"], None);
    expect_unresolved(&fixture, &text, &out, "-x", "flag");
    let (out, text) = gate(&fixture, &["--base="], None);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("--base names ``"), "{text}");
}

#[test]
fn no_base_under_ci_fails_naming_fetch_depth_and_the_flag() {
    for ci in ["true", "1", "yes", "TRUE"] {
        let fixture = fixture("ci-none", "main");
        let (out, text) = gate(&fixture, &[], Some(ci));
        assert_eq!(out.status.code(), Some(1), "CI={ci}: {text}");
        assert!(
            text.lines().any(|line| line.starts_with("Failed:")
                && line.contains("fetch-depth: 0")
                && line.contains("--base")),
            "CI={ci}: {text}"
        );
        assert!(!text.contains("Vacuous"), "CI={ci}: {text}");
        assert!(!mutants_ran(&fixture));
        let report = report(&fixture);
        let row = mutation(&report);
        assert_eq!(at(row, "/verdict"), "fail", "{row}");
        assert_eq!(at(&report, "/ok"), &json!(false));
        // The report says what stdout says: the remedy, and no command that
        // failed, because none did.
        let detail = at(row, "/detail").as_str().unwrap_or_default();
        assert!(
            detail.contains("fetch-depth: 0") && detail.contains("--base"),
            "{row}"
        );
        let problems = mutation_problems(&report);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems.iter().all(|p| p.contains("fetch-depth: 0")
                && p.contains("--base")
                && !p.contains("failed")),
            "{problems:?}"
        );
    }
}

#[test]
fn no_base_without_ci_is_vacuous_and_lists_every_source_tried() {
    // Unset, empty, `false`, `0` and any case of `false` are not CI.
    for ci in [None, Some(""), Some("false"), Some("False"), Some("0")] {
        let fixture = fixture("ci-off", "main");
        let (out, text) = gate(&fixture, &[], ci);
        assert_eq!(out.status.code(), Some(0), "CI={ci:?}: {text}");
        for source in [
            "--base: not given",
            "gates.base_ref: not set",
            "origin/HEAD: not set",
        ] {
            assert!(text.contains(source), "CI={ci:?}: {source}: {text}");
        }
        assert!(text.contains("Vacuous: no mutation base"), "{text}");
        assert!(!mutants_ran(&fixture));
        let report = report(&fixture);
        let row = mutation(&report);
        assert_eq!(at(row, "/verdict"), "vacuous", "{row}");
        let detail = at(row, "/detail").as_str().unwrap_or_default();
        for source in [
            "--base: not given",
            "gates.base_ref: not set",
            "origin/HEAD: not set",
        ] {
            assert!(detail.contains(source), "{row}");
        }
        assert_eq!(
            at(row, "/judged/base"),
            &json!({
                "ref": null,
                "source": null,
                "tried": [
                    "--base: not given",
                    "gates.base_ref: not set",
                    "origin/HEAD: not set"
                ]
            }),
            "{row}"
        );
    }
}

#[test]
fn an_origin_head_that_points_nowhere_is_no_base() {
    let fixture = fixture("dangling", "main");
    git(
        &fixture,
        &fixture.repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/gone",
        ],
    );
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("origin/HEAD: points at origin/gone, which is not a commit"),
        "{text}"
    );
    let (out, text) = gate(&fixture, &[], Some("true"));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("fetch-depth: 0"), "{text}");
}

#[test]
fn a_flag_or_key_is_a_base_under_ci_too() {
    let fixture = three_bases("ci-named");
    let (out, text) = gate(&fixture, &["--base", "origin/one"], Some("true"));
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(mutants_ran(&fixture));
}

#[test]
fn a_failed_diff_is_failed_whatever_the_ci_setting() {
    // Not a git work tree: the diff command fails, with or without CI.
    let fixture = fixture("not-git", "main");
    fs::remove_dir_all(fixture.repo.join(".git")).expect("remove .git");
    for ci in [None, Some("true")] {
        let (out, text) = gate(&fixture, &[], ci);
        assert_eq!(out.status.code(), Some(1), "CI={ci:?}: {text}");
        assert!(text.contains("a failed diff is never waived"), "{text}");
        assert!(!text.contains("fetch-depth"), "{text}");
        assert!(!mutants_ran(&fixture));
    }
}

#[test]
fn a_dry_run_lists_the_diff_from_the_named_base() {
    let fixture = three_bases("dry");
    let (out, text) = gate(&fixture, &["--dry-run", "--base", "origin/one"], None);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("git merge-base 'origin/one' HEAD"), "{text}");
    assert!(!mutants_ran(&fixture));
    let report = report(&fixture);
    assert_eq!(
        at(mutation(&report), "/judged/base"),
        &json!({ "ref": "origin/one", "source": "flag" })
    );
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

#[test]
fn the_schema_accepts_a_base_ref_and_refuses_an_empty_one() {
    let fixture = fixture("schema", "main");
    set_base_ref(&fixture, "origin/main");
    let (out, text) = check(&fixture);
    assert_eq!(out.status.code(), Some(0), "{text}");
    set_base_ref(&fixture, "");
    let (out, text) = check(&fixture);
    assert_ne!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("base_ref"), "{text}");
}

#[test]
fn the_flag_is_in_the_gate_help() {
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(["gate", "--help"])
        .output()
        .expect("spawn help");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("--base <REF>"), "{text}");
    assert!(text.contains("fetch-depth: 0"), "{text}");
}

#[test]
fn a_dry_run_with_no_base_gives_the_same_remedy_in_the_report() {
    let fixture = fixture("dry-ci", "main");
    let (out, text) = gate(&fixture, &["--dry-run"], Some("true"));
    assert_eq!(out.status.code(), Some(1), "{text}");
    let doc = report(&fixture);
    let detail = at(mutation(&doc), "/detail").as_str().unwrap_or_default();
    assert!(
        detail.contains("fetch-depth: 0") && detail.contains("--base"),
        "{detail}"
    );
    let (_, _) = gate(&fixture, &["--dry-run"], None);
    let doc = report(&fixture);
    let detail = at(mutation(&doc), "/detail").as_str().unwrap_or_default();
    assert!(
        detail.contains("no mutation base") && !detail.contains("fetch-depth"),
        "{detail}"
    );
}

#[test]
fn an_origin_head_that_is_not_a_symbolic_ref_is_still_a_base() {
    let fixture = fixture("direct-head", "main");
    let base = git(&fixture, &fixture.repo, &["rev-parse", "HEAD"]);
    commit_file(&fixture, "b.rs");
    git(
        &fixture,
        &fixture.repo,
        &[
            "update-ref",
            "--no-deref",
            "refs/remotes/origin/HEAD",
            &base,
        ],
    );
    let (out, text) = gate(&fixture, &[], Some("true"));
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(mutants_ran(&fixture), "{}", cargo_log(&fixture));
    assert!(!text.contains("not set"), "{text}");
    let report = report(&fixture);
    assert_eq!(
        at(mutation(&report), "/judged/base"),
        &json!({ "ref": "origin/HEAD", "source": "origin/HEAD" })
    );
}

#[test]
fn a_base_with_no_common_history_says_why_and_names_fetch_depth() {
    // What a shallow checkout leaves: the base is a commit, and
    // `git merge-base` finds nothing in common with HEAD.
    let fixture = fixture("no-merge-base", "main");
    let orphan = git(
        &fixture,
        &fixture.repo,
        &[
            "commit-tree",
            "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
            "-m",
            "unrelated",
        ],
    );
    remote_ref(&fixture, "orph", &orphan);
    let (out, text) = gate(&fixture, &["--base", "origin/orph"], None);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("a failed diff is never waived"), "{text}");
    assert!(
        text.contains("fetch-depth: 0") && text.contains("origin/orph"),
        "{text}"
    );
    assert!(!mutants_ran(&fixture));
    let report = report(&fixture);
    let detail = at(mutation(&report), "/detail")
        .as_str()
        .unwrap_or_default();
    assert!(
        detail.contains("fetch-depth: 0") && detail.contains("origin/orph"),
        "{detail}"
    );
    let problems = mutation_problems(&report);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems.iter().all(|p| p.contains("fetch-depth: 0")),
        "{problems:?}"
    );
}

/// The number that follows `after` in `text`.
fn number_after(text: &str, after: &str) -> usize {
    let tail = text.split(after).nth(1).unwrap_or_default();
    let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(usize::MAX)
}

#[test]
fn an_origin_head_that_names_a_tree_reads_not_a_commit() {
    let fixture = fixture("tree-head", "main");
    let tree = git(&fixture, &fixture.repo, &["rev-parse", "HEAD^{tree}"]);
    git(
        &fixture,
        &fixture.repo,
        &[
            "update-ref",
            "--no-deref",
            "refs/remotes/origin/HEAD",
            &tree,
        ],
    );
    let (_, text) = gate(&fixture, &[], None);
    assert!(text.contains("origin/HEAD: not a commit"), "{text}");
    assert!(!text.contains("origin/HEAD: not set"), "{text}");
}

#[test]
fn a_mutation_row_that_stopped_on_a_missing_tool_does_not_say_the_diff_is_empty() {
    let fixture = fixture("tool-missing-words", "main");
    fs::remove_file(fixture.bin.join("cargo-mutants")).expect("remove the stub");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(3), "{text}");
    let doc = report(&fixture);
    assert_eq!(at(mutation(&doc), "/verdict"), "tool-missing", "{doc}");
    let detail = at(mutation(&doc), "/detail").as_str().unwrap_or_default();
    assert!(detail.contains("no mutation base"), "{detail}");
    assert!(
        !detail.contains("is empty") && !detail.contains("did not run"),
        "{detail}"
    );
}

#[test]
fn a_dry_run_row_words_the_base_without_an_outcome() {
    let fixture = fixture("dry-words", "main");
    let (_, _) = gate(&fixture, &["--dry-run"], None);
    let doc = report(&fixture);
    let detail = at(mutation(&doc), "/detail").as_str().unwrap_or_default();
    assert!(detail.contains("no mutation base"), "{detail}");
    assert!(
        !detail.contains("is empty") && !detail.contains("did not run"),
        "{detail}"
    );
}

#[test]
fn the_summary_counts_commands_that_ran_as_the_count_line_does() {
    // A missing tool ran no command: it is in neither number.
    let fixture = fixture("count-words", "main");
    fs::remove_file(fixture.bin.join("cargo-deny")).expect("remove the stub");
    let (out, text) = gate(&fixture, &[], None);
    assert_eq!(out.status.code(), Some(3), "{text}");
    let said = number_after(&text, "count: ");
    let closing = number_after(&text, "problem(s), ");
    assert!(said != usize::MAX && closing != usize::MAX, "{text}");
    assert_eq!(said, closing, "{text}");
}

#[test]
fn a_report_that_cannot_be_written_ends_the_output_with_fail() {
    let fixture = fixture("unwritable", "main");
    fs::create_dir_all(fixture.repo.join("target/rust-tops/report.json")).expect("mkdir");
    // At HEAD the diff is empty: without the write error this is a pass.
    let (out, text) = gate(&fixture, &["--base", "HEAD"], None);
    assert_eq!(out.status.code(), Some(3), "{text}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let last = stdout.lines().rev().find(|line| !line.trim().is_empty());
    assert!(
        last.is_some_and(|line| line.starts_with("fail:") && line.contains("report.json")),
        "{stdout}"
    );
    assert!(
        !stdout.lines().any(|line| line.starts_with("ok:")),
        "{stdout}"
    );
}

#[test]
fn the_help_and_the_schema_say_what_exit_2_covers() {
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(["gate", "--help"])
        .output()
        .expect("spawn help");
    let help = String::from_utf8_lossy(&out.stdout).replace("\n  ", " ");
    let schema: Value = serde_json::from_str(REPORT_SCHEMA).expect("schema is JSON");
    let described = at(&schema, "/properties/exit_code/description")
        .as_str()
        .unwrap_or_default();
    for (name, text) in [("help", help.as_str()), ("schema", described)] {
        for phrase in [
            "rust-tops.yaml",
            "rust-tops.lock",
            "no enabled layer at or below --max-tier",
            "usage error",
            "unreadable when a layer in range needs a pin",
        ] {
            assert!(text.contains(phrase), "{name} lacks `{phrase}`: {text}");
        }
    }
}
