//! `cargo run -p xtask -- sync [--check] [--root DIR]`.
//!
//! `copies.toml` at the repo root pairs each hand copy (`dest`) with the
//! file it copies (`source`). Sources are under `crates/cargo-tops/templates/`,
//! except `schema/rust-tops.schema.json`, which the template vendors.
//!
//! - `sync` rewrites every dest that is missing or differs from its source.
//! - `sync --check` writes nothing. It exits 1 when a dest is missing, is not
//!   a regular file, or differs from its source by one byte or by the
//!   executable bit.
//!
//! A dest is a regular file. Sync does not follow or replace a symlink: it
//! names the link and exits 1. A list that would write under `schema/`,
//! `.github/` or the init goldens is refused before any file is read, with
//! exit 2.
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

const LIST: &str = "copies.toml";
const TEMPLATES: &str = "crates/cargo-tops/templates/";
/// The one source outside `templates/`. The template is its copy.
const SCHEMA: &str = "schema/rust-tops.schema.json";
/// `schema/` is a source. The kit workflow and the init goldens are not
/// hand copies: init and the golden test own those bytes.
const NEVER_WRITTEN: [&str; 3] = ["schema/", ".github/", "crates/cargo-tops/tests/golden/"];
const USAGE: &str = "usage: cargo run -p xtask -- sync [--check] [--root DIR]";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    copy: Vec<Pair>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pair {
    source: String,
    dest: String,
}

struct Args {
    check: bool,
    root: PathBuf,
}

/// A source file as sync writes it.
struct Source {
    bytes: Vec<u8>,
    executable: bool,
}

/// How a dest compares with its source.
enum State {
    Same,
    Missing,
    Differs,
    Mode,
    Symlink,
    NotAFile,
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("error: {err}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match sync(&args.root, args.check) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args> {
    if args.next().as_deref() != Some("sync") {
        bail!("the only task is `sync`");
    }
    let mut parsed = Args {
        check: false,
        root: Path::new(env!("CARGO_MANIFEST_DIR")).join(".."),
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" => parsed.check = true,
            "--root" => {
                parsed.root = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow!("--root needs a directory"))?;
            }
            other => bail!("unknown argument {other}"),
        }
    }
    Ok(parsed)
}

/// Check or write every pair. Returns whether every dest matches its source
/// when sync is done.
fn sync(root: &Path, check: bool) -> Result<bool> {
    let pairs = read_list(root)?;
    let mut problems = Vec::new();
    let mut wrote = 0;
    for pair in &pairs {
        let source = read_source(&root.join(&pair.source))?;
        let dest = root.join(&pair.dest);
        let problem = match state(&dest, &source)? {
            State::Same => continue,
            State::Symlink => "is a symlink; copies are regular files, so remove the link",
            State::NotAFile => "is not a regular file",
            _ if !check => {
                write(&dest, &source)?;
                println!("wrote {} from {}", pair.dest, pair.source);
                wrote += 1;
                continue;
            }
            State::Missing => "is missing; its source is",
            State::Differs => "differs from",
            State::Mode => "has a different executable bit from",
        };
        problems.push(format!("{} {problem} {}", pair.dest, pair.source));
    }
    let total = pairs.len();
    if check && problems.is_empty() {
        println!("sync --check: {total} copies match their sources");
    } else if check {
        println!("sync --check: {} of {total} copies differ", problems.len());
    } else {
        println!("sync: wrote {wrote} of {total} copies");
    }
    for problem in &problems {
        println!("  - {problem}");
    }
    Ok(problems.is_empty())
}

fn read_list(root: &Path) -> Result<Vec<Pair>> {
    let path = root.join(LIST);
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let list: List = toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    validate(&list.copy)?;
    Ok(list.copy)
}

/// Refuse a list that could write a source, a path outside the repo, or a
/// file that is not a hand copy.
fn validate(pairs: &[Pair]) -> Result<()> {
    if pairs.is_empty() {
        bail!("{LIST} lists no copy");
    }
    let sources: HashSet<&str> = pairs.iter().map(|pair| pair.source.as_str()).collect();
    let mut dests = HashSet::new();
    for pair in pairs {
        inside(&pair.source)?;
        inside(&pair.dest)?;
        if pair.source != SCHEMA && !pair.source.starts_with(TEMPLATES) {
            bail!(
                "source {}: a source is under {TEMPLATES} or is {SCHEMA}",
                pair.source
            );
        }
        if let Some(dir) = NEVER_WRITTEN.iter().find(|dir| pair.dest.starts_with(*dir)) {
            bail!("dest {}: sync never writes under {dir}", pair.dest);
        }
        if sources.contains(pair.dest.as_str()) {
            bail!("dest {}: it is also a source", pair.dest);
        }
        if !dests.insert(pair.dest.as_str()) {
            bail!("dest {}: listed twice", pair.dest);
        }
    }
    Ok(())
}

/// A listed path is relative and stays inside the repo.
fn inside(path: &str) -> Result<()> {
    let normal = Path::new(path)
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    if path.is_empty() || !normal {
        bail!("{path:?}: a listed path is relative, with no `.` or `..`");
    }
    Ok(())
}

fn read_source(path: &Path) -> Result<Source> {
    let meta = fs::symlink_metadata(path).with_context(|| format!("read {}", path.display()))?;
    if !meta.file_type().is_file() {
        bail!("source {} is not a regular file", path.display());
    }
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(Source {
        bytes,
        executable: executable(&meta),
    })
}

fn state(dest: &Path, source: &Source) -> Result<State> {
    let meta = match fs::symlink_metadata(dest) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(State::Missing),
        Err(err) => return Err(err).with_context(|| format!("stat {}", dest.display())),
    };
    if meta.file_type().is_symlink() {
        return Ok(State::Symlink);
    }
    if !meta.is_file() {
        return Ok(State::NotAFile);
    }
    let bytes = fs::read(dest).with_context(|| format!("read {}", dest.display()))?;
    Ok(if bytes != source.bytes {
        State::Differs
    } else if executable(&meta) != source.executable {
        State::Mode
    } else {
        State::Same
    })
}

/// Write `dest` through a new file beside it, then rename. A rename replaces
/// the name, so a link that appeared at `dest` since `state` is replaced,
/// not followed.
fn write(dest: &Path, source: &Source) -> Result<()> {
    let dir = dest
        .parent()
        .ok_or_else(|| anyhow!("{} has no directory", dest.display()))?;
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let name = dest.file_name().unwrap_or_default().to_string_lossy();
    let temp = dir.join(format!(".{name}.xtask-sync"));
    // A temp file left by an interrupted sync. If it cannot go, `create_new`
    // below fails and names it.
    let _ = fs::remove_file(&temp);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .with_context(|| format!("create {}", temp.display()))?;
    file.write_all(&source.bytes)
        .with_context(|| format!("write {}", temp.display()))?;
    drop(file);
    set_executable(&temp, source.executable)?;
    fs::rename(&temp, dest).with_context(|| format!("rename to {}", dest.display()))
}

/// Git keeps one mode bit, so a copy keeps its source's. Off Unix there is
/// no bit, and every file reads as not executable.
fn executable(meta: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

fn set_executable(path: &Path, executable: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable { 0o755 } else { 0o644 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .with_context(|| format!("chmod {}", path.display()))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, executable);
        Ok(())
    }
}
