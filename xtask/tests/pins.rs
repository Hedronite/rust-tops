//! `xtask pins` against fixture trees. The built binary runs directly: no
//! `cargo run`, so nothing here races another cargo on the target directory.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str, workspace: &str, drop_in: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("pins-{name}"));
    fs::create_dir_all(root.join("config/drop-in")).expect("make the fixture dirs");
    fs::write(
        root.join("Cargo.toml"),
        format!("[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"{workspace}\"\n"),
    )
    .expect("write Cargo.toml");
    fs::write(
        root.join("config/drop-in/rust-tops.yml"),
        format!("jobs:\n  g:\n    steps:\n      - run: cargo +1.99.1 install cargo-tops --version {drop_in} --locked\n"),
    )
    .expect("write the drop-in");
    root
}

fn pins(root: &Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["pins", "--root"])
        .arg(root)
        .output()
        .expect("spawn xtask pins");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().expect("exit code"), text)
}

#[test]
fn a_matching_pin_passes() {
    let (code, text) = pins(&fixture("match", "0.4.0", "0.4.0"));
    assert_eq!(code, 0, "{text}");
}

#[test]
fn a_planted_older_pin_fails_and_names_both_versions() {
    let (code, text) = pins(&fixture("planted", "0.4.0", "0.3.9"));
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("installs cargo-tops 0.3.9, the workspace is 0.4.0"),
        "{text}"
    );
}

#[test]
fn a_drop_in_with_no_install_line_fails() {
    let root = fixture("noline", "0.4.0", "0.4.0");
    fs::write(root.join("config/drop-in/rust-tops.yml"), "jobs: {}\n").expect("rewrite");
    let (code, text) = pins(&root);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("has no `install cargo-tops --version X` line"),
        "{text}"
    );
}

#[test]
fn a_missing_drop_in_is_an_error_not_a_pass() {
    let root = fixture("nofile", "0.4.0", "0.4.0");
    fs::remove_file(root.join("config/drop-in/rust-tops.yml")).expect("remove");
    let (code, text) = pins(&root);
    assert_eq!(code, 2, "{text}");
}

#[test]
fn the_committed_tree_passes() {
    let (code, text) = pins(&Path::new(env!("CARGO_MANIFEST_DIR")).join(".."));
    assert_eq!(code, 0, "{text}");
}
