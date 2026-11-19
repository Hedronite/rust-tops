//! `cargo tops check` and `cargo tops gate` hold an instance to its class.
//!
//! The kit's own tier and floor logic is tested in `src/tiers.rs`; these tests
//! drive the built binary to show the wiring: a planted fault exits non-zero
//! with the file, key and fix, and the gate reports it as a config error.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/init/library-core/rust-tops.yaml"
);

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cargo-tops-tiers-{name}-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).expect("mkdir");
    dir
}

fn tops(command: &str, dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args([command, "--path"])
        .arg(dir)
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

/// The golden instance with `from` replaced by `to`, written to a scratch dir.
fn planted(name: &str, from: &str, to: &str) -> PathBuf {
    let raw = fs::read_to_string(GOLDEN).expect("golden");
    assert!(raw.contains(from), "golden lost {from:?}");
    let dir = scratch(name);
    fs::write(dir.join("rust-tops.yaml"), raw.replacen(from, to, 1)).expect("write");
    dir
}

#[test]
fn the_golden_instance_passes_check() {
    let dir = planted("clean", "tier: 0", "tier: 0");
    let out = tops("check", &dir);
    assert!(out.status.success(), "{}", text(&out));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_loosened_survivor_floor_fails_check_with_the_fix() {
    let dir = planted(
        "survivors",
        "unexplained_survivors_max: 0",
        "unexplained_survivors_max: 3",
    );
    let out = tops("check", &dir);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(said.contains("rust-tops.yaml"), "{said}");
    assert!(said.contains("unexplained_survivors_max"), "{said}");
    assert!(said.contains("lower it to 0"), "{said}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_wrong_tier_fails_check_and_the_gate_calls_it_a_config_error() {
    let raw = fs::read_to_string(GOLDEN).expect("golden");
    let at = raw
        .find("id: \"security-advisory\"")
        .expect("advisory layer");
    let tier_at = at + raw[at..].find("tier: 0").expect("its tier");
    let dir = scratch("tier");
    let mut doc = raw;
    doc.replace_range(tier_at..tier_at + "tier: 0".len(), "tier: 5");
    fs::write(dir.join("rust-tops.yaml"), doc).expect("write");
    let out = tops("check", &dir);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(said.contains("security-advisory"), "{said}");
    let gate = tops("gate", &dir);
    assert_eq!(gate.status.code(), Some(2), "{}", text(&gate));
    fs::remove_dir_all(&dir).ok();
}
