//! `xtask docs` against fixture trees. The built binary runs directly, so
//! nothing here races another cargo on the target directory.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const TABLE: &str = "<!-- enforcement:start -->\n| Section | Keys | Judged | Declarative |\n| --- | ---: | ---: | ---: |\n| `a` | 2 | 1 | 1 |\n| **All** | **2** | **1** | **1** |\n\n1 of 2 schema keys are judged (50.0%).\n<!-- enforcement:end -->\n";
const MAP: &str = r#"{"keys": {"/a/b": {"class": "judged", "reader": "x.rs", "via": "gate"}, "/a/c": {"class": "declarative"}}}"#;
const README: &str = "| Contract | x | y |\n| Class floors | None | Two profile files; more |\n| Package | 0.4.0 | Cargo |\n| Tool lock | rustc 1.99.1, cargo-deny 0.18, nushell 0.108.0 | lock |\n\nNushell 0.108.0 is a required dependency. Install it with `cargo install nu --locked --version 0.108.0`, or read https://github.com/nushell/nushell/releases/tag/0.108.0.\n\n<!-- enforcement:start -->\n| Section | Keys | Judged | Declarative |\n| --- | ---: | ---: | ---: |\n| `a` | 2 | 1 | 1 |\n| **All** | **2** | **1** | **1** |\n\n1 of 2 schema keys are judged (50.0%).\n<!-- enforcement:end -->\n";
const MAIN_RS: &str =
    "println!(\"note: the skills run on Nushell (nu) 0.108.0, a required dependency\");\n";

fn fixture(name: &str, readme: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("docs-{name}"));
    fs::create_dir_all(root.join("profiles")).expect("make the fixture dirs");
    fs::create_dir_all(root.join("crates/cargo-tops/src")).expect("make the src dir");
    fs::write(root.join("crates/cargo-tops/src/main.rs"), MAIN_RS).expect("write main.rs");
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.4.0\"\n",
    )
    .expect("write Cargo.toml");
    fs::write(
        root.join("rust-tops.lock"),
        "[tools]\nrustc = \"1.99.1\"\ncargo-deny = \"0.18\"\nnushell = \"0.108.0\"\n",
    )
    .expect("write the lock");
    for profile in ["a.yaml", "b.yaml"] {
        fs::write(root.join("profiles").join(profile), "x: 1\n").expect("write a profile");
    }
    fs::create_dir_all(root.join("schema")).expect("make the schema dir");
    fs::write(root.join("schema/enforcement.json"), MAP).expect("write the map");
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

/// Each plant gets its own fixture root, named by `name`.
fn plant_readme(name: &str, from: &str, to: &str) -> String {
    let (code, text) = docs(&fixture(name, &README.replace(from, to)));
    assert_eq!(code, 1, "{text}");
    text
}

#[test]
fn a_planted_wrong_nushell_pin_in_the_table_fails() {
    let text = plant_readme("nu-row", "nushell 0.108.0 |", "nushell 0.107.0 |");
    assert!(
        text.contains("Tool lock says nushell 0.107.0, rust-tops.lock says 0.108.0"),
        "{text}"
    );
}

#[test]
fn a_planted_wrong_nushell_in_the_prose_fails() {
    let text = plant_readme("nu-prose", "Nushell 0.108.0 is", "Nushell 0.107.0 is");
    assert!(text.contains("README prose says nushell 0.107.0"), "{text}");
}

#[test]
fn a_planted_wrong_nushell_in_the_install_line_fails() {
    let text = plant_readme("nu-install", "--version 0.108.0", "--version 0.107.0");
    assert!(
        text.contains("README install line says nushell 0.107.0"),
        "{text}"
    );
}

#[test]
fn a_table_row_without_nushell_fails() {
    let text = plant_readme("nu-absent", ", nushell 0.108.0 |", " |");
    assert!(text.contains("does not list nushell"), "{text}");
}

#[test]
fn a_planted_wrong_nushell_in_the_init_note_fails() {
    let root = fixture("nu-note", README);
    fs::write(
        root.join("crates/cargo-tops/src/main.rs"),
        MAIN_RS.replace("0.108.0", "0.107.0"),
    )
    .expect("plant the note");
    let (code, text) = docs(&root);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("init --skills note says nushell 0.107.0"),
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
fn a_table_that_disagrees_with_the_map_fails() {
    let text = plant_readme("table-count", "| `a` | 2 | 1 | 1 |", "| `a` | 2 | 2 | 0 |");
    assert!(
        text.contains("README enforcement table disagrees with schema/enforcement.json"),
        "{text}"
    );
}

#[test]
fn a_readme_without_the_enforcement_table_fails() {
    let text = plant_readme("table-absent", "<!-- enforcement:start -->", "");
    assert!(text.contains("README has no table between"), "{text}");
}

#[test]
fn a_run_that_agrees_prints_the_ratio() {
    let (code, text) = docs(&fixture("ratio", README));
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("docs: 1 of 2 schema keys are judged (50.0%)"),
        "{text}"
    );
}

#[test]
fn the_table_flag_prints_the_table_the_readme_holds() {
    let root = fixture("print", README);
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["docs", "--table", "--root"])
        .arg(&root)
        .output()
        .expect("spawn xtask docs");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), TABLE);
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
