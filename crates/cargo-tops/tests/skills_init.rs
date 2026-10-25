//! `cargo tops init --skills` writes the same skill bytes under every harness
//! path, as ordinary files. Default `init` does not. The drift check fails
//! when any copy differs.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const NAMES: &[&str] = &["rust-tops-adopt", "rust-tops-gate", "rust-tops-review"];

/// Every harness tree `init --skills` writes. Each file is a regular file.
const COPIES: &[&str] = &[
    ".agents/skills",
    ".claude/skills",
    ".cursor/skills",
    ".grok/skills",
];

const PHRASES: &[&str] = &[
    "add a test for the specified missing branch",
    "reduce real branching or cover the risky arm",
    "assert the behavior the mutant broke",
    "profile, then algorithm",
    "the recorder never ran",
    "honor CARGO_BIN_EXE_*",
    "read that law in rust-tops/cases.json and fix the fact it read",
    "the law was not checked. It is not a pass.",
    "does not vendor engines",
];

const HARNESS_NAMES: &[&str] = &["Cursor", "Claude", "Codex", "omp", "Grok", "Halo"];

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workspace() -> PathBuf {
    manifest().join("../..")
}

fn run_init(label: &str, extra: &[&str]) -> (PathBuf, String, String, bool) {
    let dir =
        std::env::temp_dir().join(format!("cargo-tops-skills-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args([
            "init",
            "--class",
            "library-core",
            "--name",
            "golden-crate",
            "--path",
        ])
        .arg(&dir)
        .args(extra)
        .output()
        .expect("spawn cargo-tops init");
    let stdout = String::from_utf8(output.stdout).expect("stdout utf-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf-8");
    (dir, stdout, stderr, output.status.success())
}

#[test]
fn default_init_does_not_write_skills() {
    let (dir, stdout, stderr, ok) = run_init("default", &[]);
    assert!(ok, "{stderr}");
    assert!(stderr.is_empty());
    assert!(!stdout.contains(".agents/skills"));
    assert!(!dir.join(".agents").exists());
    assert!(!dir.join(".claude").exists());
    assert!(!dir.join(".cursor").exists());
    assert!(!dir.join(".grok").exists());
    assert!(!dir.join("CLAUDE.md").exists());
    assert!(!dir.join("docs/REVIEW.md").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn init_skills_matches_the_golden_layout() {
    let (dir, stdout, stderr, ok) = run_init("golden", &["--skills"]);
    assert!(ok, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    let rendered = dir.display().to_string();
    let stdout = stdout.replace(&rendered, "{path}");
    let golden = fs::read_to_string(manifest().join("tests/golden/init/skills/stdout.txt"))
        .expect("skills stdout golden");
    assert_eq!(stdout, golden);

    assert_skill_copies(&dir, &manifest().join("templates/skills"));
    assert_eq!(
        fs::read_to_string(dir.join("CLAUDE.md")).unwrap(),
        fs::read_to_string(manifest().join("templates/CLAUDE.md")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(dir.join("docs/REVIEW.md")).unwrap(),
        fs::read_to_string(manifest().join("templates/docs/REVIEW.md")).unwrap()
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn init_skills_skips_existing_copies() {
    let (dir, _, stderr, ok) = run_init("skip", &["--skills"]);
    assert!(ok, "{stderr}");
    let again = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args([
            "init",
            "--class",
            "library-core",
            "--name",
            "golden-crate",
            "--path",
        ])
        .arg(&dir)
        .arg("--skills")
        .output()
        .expect("second init");
    let stdout = String::from_utf8(again.stdout).unwrap();
    assert!(
        again.status.success(),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert!(stdout.contains("skip existing"));
    assert!(!stdout.contains("linked "));
    for root in COPIES {
        assert!(
            stdout.contains(&format!(
                "skip existing {}/{root}/rust-tops-adopt/SKILL.md",
                dir.display()
            )),
            "{stdout}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn repo_skill_trees_match_the_templates() {
    let root = workspace();
    assert_skill_copies(&root, &manifest().join("templates/skills"));
    assert_eq!(
        fs::read(root.join("CLAUDE.md")).expect("CLAUDE.md"),
        fs::read(manifest().join("templates/CLAUDE.md")).unwrap()
    );
    assert_eq!(
        fs::read(root.join("docs/REVIEW.md")).expect("docs/REVIEW.md"),
        fs::read(manifest().join("templates/docs/REVIEW.md")).unwrap()
    );
}

#[test]
fn skill_text_is_portable_and_the_gate_remedies_match_agents_md() {
    let templates = manifest().join("templates/skills");
    let agents = fs::read_to_string(manifest().join("templates/AGENTS.md")).unwrap();
    let gate = fs::read_to_string(templates.join("rust-tops-gate/rust-tops-gate.nu")).unwrap();
    for phrase in PHRASES {
        assert!(agents.contains(phrase), "AGENTS.md missing {phrase}");
        assert!(gate.contains(phrase), "gate.nu missing {phrase}");
    }
    assert!(!gate.contains("target/rust-tops"));
    assert!(!gate.contains(".passed"));
    assert!(!gate.contains("get passed"));
    assert!(gate.contains("CARGO_TARGET_DIR"));
    assert!(gate.contains("cargo metadata"));
    assert!(gate.contains("Allow"));
    assert!(gate.contains("Ack"));
    assert!(gate.contains("not-evaluated"));
    assert!(gate.contains("gate_green"));
    assert!(gate.contains("no_fact_reader"));
    let review =
        fs::read_to_string(templates.join("rust-tops-review/rust-tops-review.nu")).unwrap();
    assert!(review.contains("committed-only"));
    assert!(review.contains("includes_worktree"));
    assert!(review.contains("\"HEAD\""));
    for name in NAMES {
        let skill = fs::read_to_string(templates.join(name).join("SKILL.md")).unwrap();
        let script = fs::read_to_string(templates.join(name).join(format!("{name}.nu"))).unwrap();
        assert!(script.starts_with("#!/usr/bin/env nu\n"), "{name}");
        assert!(skill.contains("Run"), "{name}");
        assert!(skill.contains("nu: command not found"), "{name}");
        assert!(skill.contains("Nushell"), "{name}");
        let keys = frontmatter_keys(&skill);
        assert_eq!(keys, ["description", "name"], "{name}: {keys:?}");
        assert_eq!(frontmatter_value(&skill, "name"), *name);
        assert!(!frontmatter_value(&skill, "description").is_empty());
        for word in HARNESS_NAMES {
            assert!(!has_word(&skill, word), "{name} SKILL.md contains {word}");
            assert!(!has_word(&script, word), "{name}.nu contains {word}");
        }
    }
}

fn has_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| token == word)
}

/// Fail when a harness copy is a symlink or its bytes differ from the template.
fn assert_skill_copies(root: &Path, templates: &Path) {
    for name in NAMES {
        for file in ["SKILL.md", &format!("{name}.nu")] {
            let expected = fs::read(templates.join(name).join(file))
                .unwrap_or_else(|err| panic!("template {name}/{file}: {err}"));
            for copy in COPIES {
                let path = root.join(copy).join(name).join(file);
                let meta = fs::symlink_metadata(&path)
                    .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
                assert!(
                    meta.file_type().is_file(),
                    "{} must be a regular file with the template bytes",
                    path.display()
                );
                let actual =
                    fs::read(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
                assert_eq!(actual, expected, "{}", path.display());
            }
        }
    }
}

fn frontmatter_keys(md: &str) -> Vec<String> {
    let value = frontmatter(md);
    let mut keys: Vec<String> = value.as_object().expect("map").keys().cloned().collect();
    keys.sort();
    keys
}

fn frontmatter_value(md: &str, key: &str) -> String {
    frontmatter(md)
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string()
}

fn frontmatter(md: &str) -> serde_json::Value {
    let rest = md.strip_prefix("---\n").expect("opening ---");
    let yaml = rest.split_once("\n---").expect("closing ---").0;
    serde_saphyr::from_str(yaml).expect("frontmatter yaml")
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
    if let Some(path) = found {
        return Some(path);
    }
    let local = PathBuf::from("/tmp/nu/nu");
    local.is_file().then_some(local)
}

fn parse_json(stdout: &[u8]) -> serde_json::Value {
    let text = String::from_utf8_lossy(stdout);
    serde_json::from_str(text.trim()).unwrap_or_else(|err| panic!("json ({err}): {text}"))
}

#[test]
fn nu_scripts_honor_the_envelope_when_nu_is_installed() {
    let Some(nu) = nu_bin() else {
        if std::env::var_os("RUST_TOPS_REQUIRE_NU").is_some() {
            panic!("nu is not installed; RUST_TOPS_REQUIRE_NU is set");
        }
        eprintln!("nu is not installed; skill smoke skipped");
        return;
    };
    for name in NAMES {
        let script = manifest()
            .join("templates/skills")
            .join(name)
            .join(format!("{name}.nu"));
        let check = Command::new(&nu)
            .args(["--ide-check", "20"])
            .arg(&script)
            .output()
            .expect("nu --ide-check");
        assert!(
            check.status.success(),
            "{name} parse: {}",
            String::from_utf8_lossy(&check.stderr)
        );
    }
    smoke_gate(&nu);
    smoke_adopt(&nu);
    smoke_review(&nu);
}

fn smoke_gate(nu: &Path) {
    let root = std::env::temp_dir().join(format!("cargo-tops-nu-gate-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let bin = root.join("bin");
    let target = root.join("target");
    fs::create_dir_all(&bin).unwrap();
    fs::write(bin.join("cargo"), include_str!("skills_cargo_stub.sh")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(bin.join("cargo"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let script = manifest().join("templates/skills/rust-tops-gate/rust-tops-gate.nu");
    let mut path = std::ffi::OsString::from(&bin);
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let run = |mode: &str| {
        let out = Command::new(nu)
            .arg(&script)
            .args(["--path", "."])
            .current_dir(&root)
            .env("PATH", &path)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_STUB_MODE", mode)
            .output()
            .expect("nu gate");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        parse_json(&out.stdout)
    };

    let pass = run("pass");
    assert_eq!(pass["ok"], true, "{pass}");
    assert_eq!(pass["data"]["gate_green"], true, "{pass}");
    assert!(pass["command"]
        .as_str()
        .unwrap()
        .contains("cargo tops gate"));
    assert_eq!(pass["data"]["fixes"].as_array().unwrap().len(), 0);

    let missed = run("not-evaluated");
    assert_eq!(missed["ok"], false, "{missed}");
    assert_eq!(missed["data"]["gate_green"], true, "{missed}");
    assert!(missed["data"]["fixes"].as_array().unwrap().is_empty());
    let gaps = missed["data"]["gaps"].as_array().unwrap();
    assert_eq!(gaps[0]["law"], "evidence_not_self_served");
    assert_eq!(gaps[0]["reason_code"], "no_fact_reader");

    let dependent = run("run-dependent");
    assert_eq!(dependent["ok"], false, "{dependent}");
    assert_eq!(dependent["data"]["gate_green"], true, "{dependent}");
    assert!(dependent["data"]["gaps"].as_array().unwrap().is_empty());
    assert_eq!(
        dependent["data"]["fixes"][0]["reason_code"],
        "run_dependent"
    );

    let refused = run("refuse");
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(refused["data"]["fixes"][0]["remedy"]
        .as_str()
        .unwrap()
        .contains("fix the fact it read"));

    fs::create_dir_all(target.join("rust-tops")).unwrap();
    fs::write(
        target.join("rust-tops/cases.json"),
        r#"{"passed":true,"laws":[{"law":"old","status":"checked","verdict":"Allow"}]}"#,
    )
    .unwrap();
    let stale = run("missing");
    assert_eq!(stale["ok"], false, "{stale}");
    assert_eq!(stale["exit"], 1, "{stale}");
    let stale_error = stale["error"].as_str().unwrap();
    assert!(stale_error.contains("exited"), "{stale}");
    assert!(
        !stale_error.contains("cases.json is missing"),
        "a failed command was reported as a missing report: {stale}"
    );

    let no_tops = run("no-tops");
    assert_eq!(no_tops["ok"], false, "{no_tops}");
    assert_eq!(no_tops["exit"], 1, "{no_tops}");
    let no_tops_error = no_tops["error"].as_str().unwrap();
    assert!(no_tops_error.contains("no such command"), "{no_tops}");
    assert!(
        !no_tops_error.contains("cases.json is missing"),
        "{no_tops}"
    );

    let bad = run("bad-json");
    assert_eq!(bad["ok"], false, "{bad}");
    assert!(bad["error"]
        .as_str()
        .unwrap()
        .contains("missing or not JSON"));

    let coverage = run("coverage");
    assert_eq!(coverage["ok"], false, "{coverage}");
    let layers = coverage["data"]["layers"].as_array().unwrap();
    let static_lint = layers
        .iter()
        .find(|row| row["id"] == "static-lint")
        .unwrap();
    let cov = layers.iter().find(|row| row["id"] == "coverage").unwrap();
    assert_eq!(static_lint["ok"], true);
    assert_eq!(cov["ok"], false);
    assert!(cov["remedy"]
        .as_str()
        .unwrap()
        .contains("specified missing branch"));

    let empty = root.join("no-bins");
    fs::create_dir_all(&empty).unwrap();
    let mut bare_path = std::ffi::OsString::from(&empty);
    bare_path.push(":");
    bare_path.push(nu.parent().expect("nu lives in a directory"));
    let bare = Command::new(nu)
        .arg(&script)
        .args(["--path", "."])
        .current_dir(&root)
        .env("PATH", bare_path)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("nu gate without cargo");
    assert!(
        bare.status.success(),
        "{}",
        String::from_utf8_lossy(&bare.stderr)
    );
    let missing_cargo = parse_json(&bare.stdout);
    assert_eq!(missing_cargo["ok"], false, "{missing_cargo}");
    assert_ne!(
        missing_cargo["exit"].as_i64().unwrap_or(0),
        0,
        "{missing_cargo}"
    );
    let missing_error = missing_cargo["error"].as_str().unwrap_or("");
    assert!(
        missing_error.contains("command not found"),
        "expected the missing-command kind, got {missing_cargo}"
    );
    let _ = fs::remove_dir_all(&root);
}

fn smoke_adopt(nu: &Path) {
    let root = std::env::temp_dir().join(format!("cargo-tops-nu-adopt-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        bin.join("cargo"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$CARGO_LOG\"\nexit 0\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(bin.join("cargo"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let log = root.join("cargo.log");
    let script = manifest().join("templates/skills/rust-tops-adopt/rust-tops-adopt.nu");
    let out = Command::new(nu)
        .arg(&script)
        .args(["adopt", "--class", "library-core", "--path", "."])
        .current_dir(&root)
        .env("PATH", {
            let mut path = std::ffi::OsString::from(&bin);
            path.push(":");
            path.push(std::env::var_os("PATH").unwrap_or_default());
            path
        })
        .env("CARGO_LOG", &log)
        .output()
        .expect("nu adopt");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let body = parse_json(&out.stdout);
    assert_eq!(body["ok"], true, "{body}");
    let command = body["command"].as_str().unwrap();
    assert!(command.contains("cargo tops init"));
    assert!(command.contains("--skills"), "{command}");
    assert!(command.contains("cargo tops check"));
    let _ = fs::remove_dir_all(&root);
}

fn smoke_review(nu: &Path) {
    let root = std::env::temp_dir().join(format!("cargo-tops-nu-review-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&root)
            .env("GIT_AUTHOR_NAME", "Review Fixture")
            .env("GIT_AUTHOR_EMAIL", "review@example.com")
            .env("GIT_COMMITTER_NAME", "Review Fixture")
            .env("GIT_COMMITTER_EMAIL", "review@example.com")
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "-b", "main"]);
    git(&["commit", "--allow-empty", "-m", "base"]);
    fs::write(root.join("note.txt"), "hello from the diff\n").unwrap();
    git(&["add", "note.txt"]);
    git(&["commit", "-m", "add note"]);
    let script = manifest().join("templates/skills/rust-tops-review/rust-tops-review.nu");
    let files = Command::new(nu)
        .arg(&script)
        .args(["files", "--base", "HEAD~1", "--path", "."])
        .current_dir(&root)
        .output()
        .expect("nu files");
    assert!(
        files.status.success(),
        "{}",
        String::from_utf8_lossy(&files.stderr)
    );
    let files_json = parse_json(&files.stdout);
    assert_eq!(files_json["ok"], true, "{files_json}");
    assert_eq!(files_json["data"]["read_diff"], false);
    assert_eq!(files_json["data"]["files"][0], "note.txt");
    assert!(!files
        .stdout
        .windows(b"hello from the diff".len())
        .any(|w| w == b"hello from the diff"));
    let diff = Command::new(nu)
        .arg(&script)
        .args(["diff", "--base", "HEAD~1", "--path", "."])
        .current_dir(&root)
        .output()
        .expect("nu diff");
    let diff_json = parse_json(&diff.stdout);
    assert_eq!(diff_json["ok"], true, "{diff_json}");
    assert_eq!(diff_json["data"]["read_diff"], true);
    assert!(diff_json["data"]["diff"]
        .as_str()
        .unwrap()
        .contains("hello from the diff"));
    assert_eq!(diff_json["data"]["includes_worktree"], true);
    fs::write(
        root.join("note.txt"),
        "hello from the diff\nuncommitted line\n",
    )
    .unwrap();
    fs::write(root.join("brand-new.txt"), "untracked body\n").unwrap();
    let dirty = Command::new(nu)
        .arg(&script)
        .args(["files", "--base", "HEAD~1", "--path", "."])
        .current_dir(&root)
        .output()
        .expect("nu files with worktree");
    let dirty_json = parse_json(&dirty.stdout);
    assert_eq!(dirty_json["ok"], true, "{dirty_json}");
    let names: Vec<&str> = dirty_json["data"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(names.contains(&"note.txt"), "{dirty_json}");
    assert!(names.contains(&"brand-new.txt"), "{dirty_json}");
    let dirty_diff = Command::new(nu)
        .arg(&script)
        .args(["diff", "--base", "HEAD~1", "--path", "."])
        .current_dir(&root)
        .output()
        .expect("nu diff with worktree");
    let dirty_diff_json = parse_json(&dirty_diff.stdout);
    let patch = dirty_diff_json["data"]["diff"].as_str().unwrap();
    assert!(patch.contains("uncommitted line"), "{dirty_diff_json}");
    assert!(!patch.contains("untracked body"), "{dirty_diff_json}");
    let committed = Command::new(nu)
        .arg(&script)
        .args([
            "files",
            "--base",
            "HEAD~1",
            "--path",
            ".",
            "--committed-only",
        ])
        .current_dir(&root)
        .output()
        .expect("nu files committed only");
    let committed_json = parse_json(&committed.stdout);
    assert_eq!(committed_json["data"]["includes_worktree"], false);
    let committed_names: Vec<&str> = committed_json["data"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(committed_names.contains(&"note.txt"), "{committed_json}");
    assert!(
        !committed_names.contains(&"brand-new.txt"),
        "{committed_json}"
    );
    let _ = fs::remove_dir_all(&root);
}
