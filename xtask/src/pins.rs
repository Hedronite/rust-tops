//! `cargo run -p xtask -- pins [--root DIR]`.
//!
//! The drop-in workflow installs `cargo-tops` at a pinned version. This check
//! reads every `cargo ... install cargo-tops ... --version X` line in
//! `config/drop-in/rust-tops.yml` and exits 1 when any `X` differs from the
//! workspace version in `Cargo.toml`, or when the file has no such line.
//! A pin that lags the release installs the wrong gate, and nothing else
//! notices.
use anyhow::{anyhow, bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const DROP_IN: &str = "config/drop-in/rust-tops.yml";
const USAGE: &str = "usage: cargo run -p xtask -- pins [--root DIR]";

/// Run the `pins` task over the arguments after the task name.
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
            println!("pins: the drop-in install line matches the workspace version");
            ExitCode::SUCCESS
        }
        Ok(problems) => {
            println!("pins: {} problem(s)", problems.len());
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

/// Every way the drop-in pin disagrees with the workspace version.
fn check(root: &Path) -> Result<Vec<String>> {
    let version = workspace_version(root)?;
    let path = root.join(DROP_IN);
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let pins = install_pins(&text);
    if pins.is_empty() {
        return Ok(vec![format!(
            "{DROP_IN} has no `install cargo-tops --version X` line"
        )]);
    }
    Ok(pins
        .into_iter()
        .filter(|(_, pin)| *pin != version)
        .map(|(line, pin)| {
            format!("{DROP_IN}:{line} installs cargo-tops {pin}, the workspace is {version}")
        })
        .collect())
}

fn workspace_version(root: &Path) -> Result<String> {
    let path = root.join("Cargo.toml");
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let manifest: toml::Table =
        toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    manifest
        .get("workspace")
        .and_then(|table| table.get("package"))
        .and_then(|table| table.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("{} has no [workspace.package] version", path.display()))
}

/// `(line number, version)` for each non-comment line that installs
/// `cargo-tops` with `--version`.
fn install_pins(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with('#'))
        .filter_map(|(index, line)| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let install = words.iter().position(|word| *word == "install")?;
            if words.get(install + 1) != Some(&"cargo-tops") {
                return None;
            }
            let flag = words.iter().position(|word| *word == "--version")?;
            let pin = words.get(flag + 1)?;
            Some((index + 1, (*pin).to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::install_pins;

    #[test]
    fn a_pin_is_read_from_an_install_line_and_comments_are_skipped() {
        let text = "a: 1\n      - run: cargo +1.99.1 install cargo-tops --version 0.4.0 --locked\n# cargo install cargo-tops --version 9.9.9\n      - run: cargo install other --version 1.0.0\n";
        assert_eq!(install_pins(text), vec![(2, "0.4.0".to_owned())]);
    }
}
