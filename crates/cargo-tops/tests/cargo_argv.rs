//! `cargo tops` and `cargo-tops` both reach this binary.
//!
//! Cargo runs a subcommand by executing `cargo-<name>` with the name as
//! argv[1]. This test uses the cargo that built the suite and the
//! `cargo-tops` binary that suite just built. No stand-in for cargo.
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn real_cargo() -> &'static str {
    env!("CARGO")
}

fn tops_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cargo-tops"))
}

fn path_with_built_tops() -> OsString {
    let tops_dir = tops_bin()
        .parent()
        .expect("cargo-tops has a directory")
        .to_path_buf();
    let cargo_dir = PathBuf::from(real_cargo())
        .parent()
        .expect("cargo has a directory")
        .to_path_buf();
    std::env::join_paths([&tops_dir, &cargo_dir]).expect("join PATH")
}

/// A fresh directory that is removed when it drops, on every path out of the
/// test. It holds the fixture crate and an empty `CARGO_HOME`.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let dir =
            std::env::temp_dir().join(format!("cargo-tops-argv-{}-{nanos}", std::process::id()));
        fs::create_dir_all(dir.join("cargo-home")).expect("create the scratch directory");
        Self(dir)
    }

    fn cargo_home(&self) -> PathBuf {
        self.0.join("cargo-home")
    }

    fn root(&self) -> PathBuf {
        self.0.join("root")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// `cargo tops <args>` with an empty `CARGO_HOME` and a cleared environment.
///
/// Cargo looks for `cargo-<name>` in `$CARGO_HOME/bin` before it looks on
/// `PATH`. A host whose `CARGO_HOME` holds an older `cargo-tops` (the
/// `cargo install` copy) would then run that one, and this test would grade
/// the wrong binary. An empty `CARGO_HOME` leaves `PATH`, with the binary
/// just built first, as the only place cargo can look. The environment is
/// cleared for the same reason: an inherited `CARGO_ALIAS_TOPS` or the like
/// would also send `cargo tops` somewhere else. Only what cargo needs to run
/// passes through.
fn cargo_tops(
    scratch: &Scratch,
    args: &[&str],
    dir: Option<&std::path::Path>,
) -> std::process::Output {
    let mut cmd = Command::new(real_cargo());
    cmd.args(std::iter::once("tops").chain(args.iter().copied()))
        .env_clear()
        .env("CARGO_HOME", scratch.cargo_home())
        .env("PATH", path_with_built_tops());
    for key in [
        "HOME",
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "CARGO_TARGET_DIR",
        "TMPDIR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    cmd.output().expect("spawn cargo tops")
}

#[test]
#[expect(clippy::too_many_lines, reason = "over the kit line threshold")]
fn cargo_tops_help_and_check_use_the_built_binary() {
    let scratch = Scratch::new();
    let help = cargo_tops(&scratch, &["--help"], None);
    let help_text = format!(
        "{}{}",
        String::from_utf8_lossy(&help.stdout),
        String::from_utf8_lossy(&help.stderr)
    );
    assert!(help.status.success(), "{help_text}");
    assert!(help_text.contains("check"), "{help_text}");
    assert!(help_text.contains("gate"), "{help_text}");
    assert!(
        !help_text.contains("unrecognized subcommand"),
        "{help_text}"
    );

    let root = scratch.root();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"tops-argv\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "// fixture\n").unwrap();

    let init = cargo_tops(
        &scratch,
        &[
            "init",
            "--class",
            "library-core",
            "--name",
            "tops-argv",
            "--path",
            ".",
        ],
        Some(&root),
    );
    let init_text = format!(
        "{}{}",
        String::from_utf8_lossy(&init.stdout),
        String::from_utf8_lossy(&init.stderr)
    );
    assert!(init.status.success(), "{init_text}");

    let check = cargo_tops(&scratch, &["check", "--path", "."], Some(&root));
    let check_text = format!(
        "{}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(check.status.success(), "{check_text}");
    assert!(check_text.contains("ok:"), "{check_text}");
    assert!(
        !check_text.contains("unrecognized subcommand"),
        "{check_text}"
    );

    let direct = Command::new(tops_bin())
        .args(["check", "--path", "."])
        .current_dir(&root)
        .output()
        .expect("spawn cargo-tops check");
    let direct_text = format!(
        "{}{}",
        String::from_utf8_lossy(&direct.stdout),
        String::from_utf8_lossy(&direct.stderr)
    );
    assert!(direct.status.success(), "{direct_text}");
    assert!(direct_text.contains("ok:"), "{direct_text}");
}
