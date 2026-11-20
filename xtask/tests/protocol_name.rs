//! The protocol file is `PROTOCOL.md`. No tracked file but the changelog and
//! the released notes under `docs/release-notes/`, which are history and keep
//! the old name, may still use the old one.
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// A copy of the tree with no `.git`, such as the one cargo-mutants tests in,
/// has no tracked files to list. The scan then has nothing to read.
fn has_history() -> bool {
    workspace().join(".git").exists()
}

/// Files that record what was published, which keep the old name.
fn is_history(path: &str) -> bool {
    path == "CHANGELOG.md" || path.starts_with("docs/release-notes/")
}

/// The retired file name, built so that this file does not name it.
fn old_name() -> String {
    ["RUST", "_TOPS.md"].concat()
}

/// Every tracked file outside the history files that contains `needle`.
fn tracked_files_naming(needle: &str) -> Vec<String> {
    let listing = Command::new("git")
        .current_dir(workspace())
        .args(["ls-files", "-z"])
        .output()
        .expect("git ls-files runs");
    assert!(listing.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&listing.stdout)
        .split('\0')
        .filter(|path| !path.is_empty() && !is_history(path))
        .filter(|path| {
            std::fs::read(workspace().join(path))
                .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(needle))
        })
        .map(str::to_string)
        .collect()
}

#[test]
fn no_tracked_file_but_the_changelog_names_the_old_protocol_file() {
    if !has_history() {
        return;
    }
    assert_eq!(tracked_files_naming(&old_name()), Vec::<String>::new());
}

#[test]
fn the_scan_reads_tracked_files() {
    if !has_history() {
        return;
    }
    assert!(
        tracked_files_naming(&["zz-no-such", "-text-zz"].concat()).is_empty(),
        "an absent string is found nowhere"
    );
    assert!(
        tracked_files_naming("PROTOCOL.md").contains(&"README.md".to_string()),
        "the scan reads tracked files"
    );
}

#[test]
fn the_protocol_file_is_a_rename_and_the_old_one_is_gone() {
    assert!(workspace().join("PROTOCOL.md").is_file());
    assert!(!workspace().join(old_name()).exists());
}
