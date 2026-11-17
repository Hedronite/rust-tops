//! `cargo-tops --version`, `cargo tops --version` and `cargo tops version`.
//!
//! These drive the binary the suite just built. `cargo tops …` reaches it
//! through argv[1] == "tops", the argv cargo passes, so the test passes that
//! word itself and never depends on which `cargo-tops` a host has installed.
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const PACKAGE: &str = env!("CARGO_PKG_VERSION");

fn tops(args: &[&str], dir: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn cargo-tops")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("cargo-tops-version-{name}-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).expect("mkdir");
    dir
}

#[test]
fn both_spellings_of_dash_dash_version_print_the_package_version() {
    let dir = scratch("flag");
    for args in [&["--version"][..], &["tops", "--version"][..]] {
        let out = tops(args, &dir);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {}", text(&out));
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            format!("cargo-tops {PACKAGE}\n"),
            "{args:?}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_version_subcommand_prints_package_protocol_and_the_lock_pins() {
    let dir = scratch("pins");
    fs::write(
        dir.join("rust-tops.lock"),
        "[tools]\nrustc = \"1.99.1\"\ncargo-nextest = \"0.9.145\"\n\n[notes]\nschema = \"1.0.0\"\n",
    )
    .expect("write lock");
    for args in [&["version"][..], &["tops", "version"][..]] {
        let out = tops(args, &dir);
        let text = text(&out);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {text}");
        assert!(text.contains(&format!("cargo-tops {PACKAGE}\n")), "{text}");
        assert!(text.contains("protocol 1.2.0\n"), "{text}");
        assert!(text.contains("  rustc = 1.99.1\n"), "{text}");
        assert!(text.contains("  cargo-nextest = 0.9.145\n"), "{text}");
        assert!(!text.contains("schema"), "{text}");
    }
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_version_subcommand_without_a_lock_says_so_and_exits_zero() {
    let dir = scratch("nolock");
    let out = tops(&["version"], &dir);
    let text = text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("protocol 1.2.0\n"), "{text}");
    assert!(
        text.contains("tool pins: none (no ./rust-tops.lock)\n"),
        "{text}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_version_subcommand_with_a_broken_lock_still_exits_zero() {
    let dir = scratch("broken");
    fs::write(dir.join("rust-tops.lock"), "[tools\n").expect("write lock");
    let out = tops(&["version"], &dir);
    let text = text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("is not valid TOML"), "{text}");
    fs::remove_dir_all(&dir).ok();
}
