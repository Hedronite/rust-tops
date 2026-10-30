//! `init` never writes through a symlink. A link at a destination, dangling
//! or not, fails `init` with or without `--force`, and the file it points at
//! is left as it was. A regular file is still skipped, or rewritten with
//! `--force`.
#![cfg(unix)]
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(label: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("init-symlink-{label}"));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("repo")).expect("create repo");
    root
}

/// `cargo-tops init` into `repo`. Returns success and stdout plus stderr.
fn init(repo: &Path, extra: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args([
            "init",
            "--class",
            "library-core",
            "--name",
            "linked",
            "--path",
        ])
        .arg(repo)
        .args(extra)
        .output()
        .expect("spawn cargo-tops init");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

fn template(rel: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("templates")
            .join(rel),
    )
    .expect("template")
}

#[test]
fn a_symlinked_destination_fails_init_with_or_without_force() {
    let root = scratch("linked");
    let repo = root.join("repo");
    let outside = root.join("outside.md");
    fs::write(&outside, "keep\n").expect("write outside");
    symlink(&outside, repo.join("AGENTS.md")).expect("symlink");

    for extra in [&[][..], &["--force"]] {
        let (ok, text) = init(&repo, extra);
        assert!(!ok, "{extra:?}: {text}");
        assert!(
            text.contains(&format!(
                "refusing to write {}: it is a symlink, and init does not follow one",
                repo.join("AGENTS.md").display()
            )),
            "{extra:?}: {text}"
        );
        assert_eq!(fs::read_to_string(&outside).expect("outside"), "keep\n");
        let meta = fs::symlink_metadata(repo.join("AGENTS.md")).expect("stat");
        assert!(
            meta.file_type().is_symlink(),
            "{extra:?}: the link was replaced"
        );
    }
}

#[test]
fn a_dangling_symlink_is_not_followed_to_create_its_target() {
    let root = scratch("dangling");
    let repo = root.join("repo");
    let target = root.join("made-by-init.toml");
    symlink(&target, repo.join("clippy.toml")).expect("symlink");

    let (ok, text) = init(&repo, &[]);
    assert!(!ok, "{text}");
    assert!(text.contains("clippy.toml: it is a symlink"), "{text}");
    assert!(!target.exists(), "init wrote through a dangling link");
}

#[test]
fn a_symlinked_skill_copy_fails_init_skills() {
    let root = scratch("skill");
    let repo = root.join("repo");
    let outside = root.join("SKILL.md");
    fs::write(&outside, "keep\n").expect("write outside");
    let skill = repo.join(".claude/skills/rust-tops-gate");
    fs::create_dir_all(&skill).expect("mkdir");
    symlink(&outside, skill.join("SKILL.md")).expect("symlink");

    let (ok, text) = init(&repo, &["--skills", "--force"]);
    assert!(!ok, "{text}");
    assert!(
        text.contains(".claude/skills/rust-tops-gate/SKILL.md: it is a symlink"),
        "{text}"
    );
    assert_eq!(fs::read_to_string(&outside).expect("outside"), "keep\n");
}

#[test]
fn a_regular_file_is_skipped_or_rewritten_with_force() {
    let root = scratch("regular");
    let repo = root.join("repo");
    fs::write(repo.join("AGENTS.md"), "mine\n").expect("write");

    let (ok, text) = init(&repo, &[]);
    assert!(ok, "{text}");
    assert!(
        text.contains(&format!(
            "skip existing {}",
            repo.join("AGENTS.md").display()
        )),
        "{text}"
    );
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md")).expect("read"),
        "mine\n"
    );

    let (ok, text) = init(&repo, &["--force"]);
    assert!(ok, "{text}");
    assert!(
        text.contains(&format!("wrote {}", repo.join("AGENTS.md").display())),
        "{text}"
    );
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md")).expect("read"),
        template("AGENTS.md")
    );
    let script = repo.join("scripts/fetch-bend.sh");
    assert_eq!(
        fs::read_to_string(&script).expect("read"),
        template("fetch-bend.sh")
    );
    let mode = fs::metadata(&script).expect("stat").permissions().mode();
    assert_eq!(mode & 0o777, 0o755, "{text}");
}
