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

fn cargo_tops(args: &[&str], dir: Option<&std::path::Path>) -> std::process::Output {
    let mut cmd = Command::new(real_cargo());
    cmd.args(std::iter::once("tops").chain(args.iter().copied()))
        .env("PATH", path_with_built_tops());
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    cmd.output().expect("spawn cargo tops")
}

#[test]
fn cargo_tops_help_and_check_use_the_built_binary() {
    let help = cargo_tops(&["--help"], None);
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

    let root = std::env::temp_dir().join(format!("cargo-tops-argv-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"tops-argv\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "// fixture\n").unwrap();

    let init = cargo_tops(
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

    let check = cargo_tops(&["check", "--path", "."], Some(&root));
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
    let _ = fs::remove_dir_all(&root);
}
