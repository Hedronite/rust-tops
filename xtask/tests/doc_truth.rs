//! `xtask docs` against fixture trees. The built binary runs directly, so
//! nothing here races another cargo on the target directory.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const README: &str = "| Contract | x | y |\n| Class floors | None | Two profile files; more |\n| Package | 0.4.0 | Cargo |\n| Tool lock | rustc 1.99.1, cargo-deny 0.18 | lock |\n";

fn fixture(name: &str, readme: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("docs-{name}"));
    fs::create_dir_all(root.join("profiles")).expect("make the fixture dirs");
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.4.0\"\n",
    )
    .expect("write Cargo.toml");
    fs::write(
        root.join("rust-tops.lock"),
        "[tools]\nrustc = \"1.99.1\"\ncargo-deny = \"0.18\"\n",
    )
    .expect("write the lock");
    for profile in ["a.yaml", "b.yaml"] {
        fs::write(root.join("profiles").join(profile), "x: 1\n").expect("write a profile");
    }
    fs::write(root.join("README.md"), readme).expect("write the README");
    root
}

fn docs(root: &Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["docs", "--root"])
        .arg(root)
        .output()
        .expect("spawn xtask docs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().expect("exit code"), text)
}

#[test]
fn a_table_that_agrees_passes() {
    let (code, text) = docs(&fixture("agree", README));
    assert_eq!(code, 0, "{text}");
}

#[test]
fn a_planted_wrong_package_version_fails() {
    let readme = README.replace("| Package | 0.4.0 |", "| Package | 0.3.9 |");
    let (code, text) = docs(&fixture("pkg", &readme));
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("Package row says 0.3.9, Cargo.toml says 0.4.0"),
        "{text}"
    );
}

#[test]
fn a_planted_wrong_tool_pin_fails() {
    let readme = README.replace("rustc 1.99.1", "rustc 1.91.0");
    let (code, text) = docs(&fixture("tool", &readme));
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("rustc 1.91.0, rust-tops.lock says 1.99.1"),
        "{text}"
    );
}

#[test]
fn a_planted_wrong_profile_count_fails() {
    let readme = README.replace("Two profile files", "Five profile files");
    let (code, text) = docs(&fixture("count", &readme));
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("says 5 profile files, profiles/ has 2"),
        "{text}"
    );
}

#[test]
fn a_readme_without_the_row_is_an_error() {
    let (code, text) = docs(&fixture("norow", "no table here\n"));
    assert_eq!(code, 2, "{text}");
}

#[test]
fn the_committed_tree_passes() {
    let (code, text) = docs(&Path::new(env!("CARGO_MANIFEST_DIR")).join(".."));
    assert_eq!(code, 0, "{text}");
}
