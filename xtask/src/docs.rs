//! `cargo run -p xtask -- docs [--root DIR]`.
//!
//! Prose states facts that Cargo, the tool lock and the profile directory
//! also state. This check reads the README pin table and fails when it
//! disagrees with them:
//!
//! - the `Package` row against the workspace version in `Cargo.toml`;
//! - each `name version` in the `Tool lock` row against `[tools]` in
//!   `rust-tops.lock`;
//! - the Nushell version the README prose, its install line and release link
//!   name, and the version the `init --skills` note in `main.rs` names,
//!   against `[tools] nushell` in `rust-tops.lock`, which the `Tool lock`
//!   row must also list;
//! - the count in the `Class floors` row (`Eight profile files`) against the
//!   number of `profiles/*.yaml`.
//!
//! It exits 0 when they agree, 1 with each disagreement listed, and 2 when
//! a file or a row cannot be read.
use anyhow::{anyhow, bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo run -p xtask -- docs [--root DIR]";
const COUNTS: [&str; 12] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
];

/// Run the `docs` task over the arguments after the task name.
pub(crate) fn run(args: impl Iterator<Item = String>) -> ExitCode {
    let root = match parse_root(args) {
        Ok(root) => root,
        Err(err) => {
            eprintln!("error: {err}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match check(&root) {
        Ok(problems) if problems.is_empty() => {
            println!("docs: the README pin table matches Cargo.toml, rust-tops.lock and profiles/");
            ExitCode::SUCCESS
        }
        Ok(problems) => {
            println!("docs: {} problem(s)", problems.len());
            for problem in &problems {
                println!("  - {problem}");
            }
            ExitCode::from(1)
        }
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn parse_root(mut args: impl Iterator<Item = String>) -> Result<PathBuf> {
    let mut root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                root = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow!("--root needs a directory"))?;
            }
            other => bail!("unknown argument {other}"),
        }
    }
    Ok(root)
}

fn read_toml(root: &Path, name: &str) -> Result<toml::Table> {
    let path = root.join(name);
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))
}

/// Every way the README pin table disagrees with the files it describes.
fn check(root: &Path) -> Result<Vec<String>> {
    let readme_path = root.join("README.md");
    let readme = fs::read_to_string(&readme_path)
        .with_context(|| format!("read {}", readme_path.display()))?;
    let mut problems = Vec::new();
    problems.extend(check_package(root, &readme)?);
    problems.extend(check_tools(root, &readme)?);
    problems.extend(check_nushell(root, &readme)?);
    problems.extend(check_profiles(root, &readme)?);
    Ok(problems)
}

fn check_package(root: &Path, readme: &str) -> Result<Option<String>> {
    let manifest = read_toml(root, "Cargo.toml")?;
    let version = manifest
        .get("workspace")
        .and_then(|table| table.get("package"))
        .and_then(|table| table.get("version"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| anyhow!("Cargo.toml has no [workspace.package] version"))?;
    let package = cell(readme, "| Package |", 1)?;
    Ok((package != version)
        .then(|| format!("README Package row says {package}, Cargo.toml says {version}")))
}

fn check_tools(root: &Path, readme: &str) -> Result<Vec<String>> {
    let lock = read_toml(root, "rust-tops.lock")?;
    let tools = lock
        .get("tools")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| anyhow!("rust-tops.lock has no [tools] table"))?;
    let mut problems = Vec::new();
    for item in cell(readme, "| Tool lock |", 1)?.split(',') {
        let item = item.trim();
        let (name, pinned) = item
            .rsplit_once(' ')
            .ok_or_else(|| anyhow!("README Tool lock item {item:?} is not `name version`"))?;
        match tools.get(name).and_then(toml::Value::as_str) {
            Some(locked) if locked == pinned => {}
            Some(locked) => problems.push(format!(
                "README Tool lock says {name} {pinned}, rust-tops.lock says {locked}"
            )),
            None => problems.push(format!(
                "README Tool lock names {name}, which rust-tops.lock does not pin"
            )),
        }
    }
    Ok(problems)
}

/// The version after `prefix` in `text`: digits and dots, no trailing dot.
fn version_after(text: &str, prefix: &str) -> Option<String> {
    let start = text.find(prefix)? + prefix.len();
    let version: String = text[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect();
    Some(version.trim_end_matches('.').to_owned())
}

/// Nushell is stated in four places besides the lock; each must match it.
fn check_nushell(root: &Path, readme: &str) -> Result<Vec<String>> {
    let lock = read_toml(root, "rust-tops.lock")?;
    let locked = lock
        .get("tools")
        .and_then(|tools| tools.get("nushell"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| anyhow!("rust-tops.lock pins no [tools] nushell"))?;
    let main_path = root.join("crates/cargo-tops/src/main.rs");
    let main_rs =
        fs::read_to_string(&main_path).with_context(|| format!("read {}", main_path.display()))?;
    let mut problems = Vec::new();
    if !cell(readme, "| Tool lock |", 1)?
        .split(',')
        .any(|item| item.trim().starts_with("nushell "))
    {
        problems.push("README Tool lock row does not list nushell".to_owned());
    }
    let places = [
        ("README prose", readme, "\nNushell "),
        (
            "README install line",
            readme,
            "cargo install nu --locked --version ",
        ),
        (
            "README release link",
            readme,
            "nushell/nushell/releases/tag/",
        ),
        ("init --skills note", main_rs.as_str(), "Nushell (nu) "),
    ];
    for (place, text, prefix) in places {
        match version_after(text, prefix) {
            Some(stated) if stated == locked => {}
            Some(stated) => problems.push(format!(
                "{place} says nushell {stated}, rust-tops.lock says {locked}"
            )),
            None => problems.push(format!("{place} does not state the nushell version")),
        }
    }
    Ok(problems)
}

fn check_profiles(root: &Path, readme: &str) -> Result<Option<String>> {
    let profiles = profile_count(root)?;
    let floors = cell(readme, "| Class floors |", 2)?;
    let word = floors
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();
    Ok(match COUNTS.iter().position(|count| *count == word) {
        Some(stated) if stated == profiles => None,
        Some(stated) => Some(format!(
            "README Class floors row says {stated} profile files, profiles/ has {profiles}"
        )),
        None => Some(format!(
            "README Class floors row starts with {word:?}, not a count word"
        )),
    })
}

fn profile_count(root: &Path) -> Result<usize> {
    let dir = root.join("profiles");
    let entries = fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))?;
    Ok(entries
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("yaml"))
        .count())
}

/// The trimmed text of cell `index` (0 is the row label) of the table row
/// that starts with `prefix`.
fn cell(readme: &str, prefix: &str, index: usize) -> Result<String> {
    let line = readme
        .lines()
        .find(|line| line.starts_with(prefix))
        .ok_or_else(|| anyhow!("README has no table row starting {prefix:?}"))?;
    line.trim_matches('|')
        .split('|')
        .nth(index)
        .map(|text| text.trim().to_owned())
        .ok_or_else(|| anyhow!("README row {prefix:?} has no cell {index}"))
}
