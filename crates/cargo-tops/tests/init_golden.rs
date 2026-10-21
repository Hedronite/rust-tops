//! `cargo tops init` file bytes and stdout, recorded from main `968c390`.
//! Re-recorded for protocol 1.1.0 / package 0.2.0: the `laws` layer and
//! version in `rust-tops.yaml`, the Bend kit (`laws/bend.lock`,
//! `scripts/fetch-bend.sh`), `rust-toolchain.toml`, and the per-class
//! workflow (drop-in floor plus the jobs §4 requires).
//!
//! `RUST_TOPS_RECORD_GOLDEN` rewrites these files for a local re-record.
//! CI does not set it.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CLASSES: &[&str] = &[
    "library-core",
    "parser-codec",
    "no-std-embedded",
    "unsafe-kernel",
    "binary-cli",
];

/// Files whose bytes depend on the class.
const PER_CLASS: &[&str] = &[
    "rust-tops.yaml",
    "rust-toolchain.toml",
    ".github/workflows/rust-tops.yml",
];

const SHARED: &[&str] = &[
    "AGENTS.md",
    "clippy.toml",
    ".config/nextest.toml",
    ".cargo/mutants.toml",
    ".cargo-crap.toml",
    "deny.toml",
    "LAWS.bend",
    "laws/bend.lock",
    "scripts/fetch-bend.sh",
];

fn golden_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/init")
}

fn recording() -> bool {
    std::env::var_os("RUST_TOPS_RECORD_GOLDEN").is_some()
}

fn run_init(class: &str) -> (PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("cargo-tops-init-{class}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(["init", "--class", class, "--name", "golden-crate", "--path"])
        .arg(&dir)
        .output()
        .unwrap_or_else(|err| panic!("spawn cargo-tops init: {err}"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "init {class} exited {}: {stderr}",
        output.status
    );
    assert!(stderr.is_empty(), "init {class} stderr: {stderr}");
    let stdout = String::from_utf8(output.stdout).expect("init stdout is utf-8");
    let rendered = dir.display().to_string();
    let stdout = stdout.replace(&rendered, "{path}");
    assert!(
        stdout.contains("{path}/rust-tops.yaml"),
        "stdout did not contain the init path for {class}:\n{stdout}"
    );
    assert!(
        !stdout.contains(&rendered),
        "stdout still contains the absolute init path for {class}"
    );
    (dir, stdout)
}

fn expect_bytes(golden: &Path, actual: &[u8]) {
    if recording() {
        if let Some(parent) = golden.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|err| panic!("mkdir {}: {err}", parent.display()));
        }
        fs::write(golden, actual).unwrap_or_else(|err| panic!("write {}: {err}", golden.display()));
        return;
    }
    let expected =
        fs::read(golden).unwrap_or_else(|err| panic!("read {}: {err}", golden.display()));
    if actual == expected.as_slice() {
        return;
    }
    let at = actual
        .iter()
        .zip(expected.iter())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| actual.len().min(expected.len()));
    panic!(
        "{} differs from init output (golden {} bytes, actual {} bytes, first byte {at})",
        golden.display(),
        expected.len(),
        actual.len()
    );
}

fn expect_text(golden: &Path, actual: &str) {
    expect_bytes(golden, actual.as_bytes());
}

#[test]
fn init_output_matches_main_bytes() {
    for class in CLASSES {
        let (dir, stdout) = run_init(class);
        expect_text(&golden_root().join(class).join("stdout.txt"), &stdout);
        for rel in PER_CLASS {
            let bytes =
                fs::read(dir.join(rel)).unwrap_or_else(|err| panic!("read {class} {rel}: {err}"));
            expect_bytes(&golden_root().join(class).join(rel), &bytes);
        }
        for rel in SHARED {
            let bytes =
                fs::read(dir.join(rel)).unwrap_or_else(|err| panic!("read {class} {rel}: {err}"));
            expect_bytes(&golden_root().join("shared").join(rel), &bytes);
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
