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
//!   number of `profiles/*.yaml`;
//! - the table between the `enforcement` markers against
//!   `schema/enforcement.json`: each section's keys, how many are judged,
//!   and the total ratio. `--table` prints that table.
//!
//! It exits 0 when they agree, 1 with each disagreement listed, and 2 when
//! a file or a row cannot be read.
use anyhow::{anyhow, bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo run -p xtask -- docs [--root DIR] [--table]";
const TABLE_START: &str = "<!-- enforcement:start -->";
const TABLE_END: &str = "<!-- enforcement:end -->";
const COUNTS: [&str; 12] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
];

/// Run the `docs` task over the arguments after the task name.
pub(crate) fn run(args: impl Iterator<Item = String>) -> ExitCode {
    let (root, table) = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("error: {err}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if table {
        return print_table(&root);
    }
    match check(&root) {
        Ok(problems) if problems.is_empty() => {
            println!("docs: the README pin table matches Cargo.toml, rust-tops.lock and profiles/");
            println!("docs: {}", ratio_line(&root).unwrap_or_default());
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

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<(PathBuf, bool)> {
    let mut root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut table = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--table" => table = true,
            "--root" => {
                root = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow!("--root needs a directory"))?;
            }
            other => bail!("unknown argument {other}"),
        }
    }
    Ok((root, table))
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
    problems.extend(check_enforcement(root, &readme)?);
    Ok(problems)
}

/// The sections of `schema/enforcement.json` with their key counts: a
/// section is the first path part, or `gates.<name>` under `gates`.
fn sections(root: &Path) -> Result<Vec<(String, usize, usize)>> {
    let path = root.join("schema").join("enforcement.json");
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let doc: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    let keys = doc
        .get("keys")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| anyhow!("{} has no `keys` object", path.display()))?;
    let mut rows: Vec<(String, usize, usize)> = Vec::new();
    for (key, entry) in keys {
        let section = section_of(key);
        let judged = entry.get("class").and_then(serde_json::Value::as_str) == Some("judged");
        match rows.iter_mut().find(|row| row.0 == section) {
            Some(row) => {
                row.1 += 1;
                row.2 += usize::from(judged);
            }
            None => rows.push((section, 1, usize::from(judged))),
        }
    }
    Ok(rows)
}

fn section_of(key: &str) -> String {
    let mut parts = key.trim_start_matches('/').split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("gates"), Some(name), Some(_)) => format!("gates.{name}"),
        (Some(first), ..) => first.to_string(),
        (None, ..) => String::new(),
    }
}

/// `judged` of `total` as tenths of a percent, rounded down.
fn percent_tenths(judged: usize, total: usize) -> usize {
    (judged * 1000).checked_div(total).unwrap_or(0)
}

fn ratio_line(root: &Path) -> Result<String> {
    let rows = sections(root)?;
    let total: usize = rows.iter().map(|row| row.1).sum();
    let judged: usize = rows.iter().map(|row| row.2).sum();
    let tenths = percent_tenths(judged, total);
    Ok(format!(
        "{judged} of {total} schema keys are judged ({}.{}%)",
        tenths / 10,
        tenths % 10
    ))
}

/// The table the README holds between its `enforcement` markers.
fn render_table(root: &Path) -> Result<String> {
    let mut rows = sections(root)?;
    rows.sort();
    let mut out =
        String::from("| Section | Keys | Judged | Declarative |\n| --- | ---: | ---: | ---: |\n");
    for (section, total, judged) in &rows {
        out.push_str(&format!(
            "| `{section}` | {total} | {judged} | {} |\n",
            total - judged
        ));
    }
    let total: usize = rows.iter().map(|row| row.1).sum();
    let judged: usize = rows.iter().map(|row| row.2).sum();
    out.push_str(&format!(
        "| **All** | **{total}** | **{judged}** | **{}** |\n\n{}.\n",
        total - judged,
        ratio_line(root)?
    ));
    Ok(out)
}

fn print_table(root: &Path) -> ExitCode {
    match render_table(root) {
        Ok(table) => {
            print!("{TABLE_START}\n{table}{TABLE_END}\n");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn check_enforcement(root: &Path, readme: &str) -> Result<Option<String>> {
    let want = render_table(root)?;
    let held = readme
        .split_once(TABLE_START)
        .and_then(|(_, rest)| rest.split_once(TABLE_END))
        .map(|(block, _)| block.trim_start_matches('\n'));
    Ok(match held {
        None => Some(format!(
            "README has no table between {TABLE_START} and {TABLE_END}; print it with `cargo run -p xtask -- docs --table`"
        )),
        Some(block) if block != want => Some(
            "README enforcement table disagrees with schema/enforcement.json; reprint it with `cargo run -p xtask -- docs --table`"
                .to_string(),
        ),
        Some(_) => None,
    })
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
