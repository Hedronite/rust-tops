//! `cargo run -p xtask -- sync [--check] [--root DIR]`.
//!
//! `copies.toml` at the repo root has one `[[copy]]` table per source: `from`
//! is the file read, and `to` lists the hand copies written from it. Sources
//! are under `crates/cargo-tops/templates/`, except
//! `schema/rust-tops.schema.json`, which the template vendors.
//!
//! - `sync` rewrites every dest that is missing or differs from its source.
//! - `sync --check` writes nothing. It exits 1 when a dest is missing, is not
//!   a regular file, or differs from its source by one byte or by the
//!   executable bit.
//!
//! A dest is a regular file in real directories. Sync does not follow or
//! replace a symlink, at the dest or at a directory above it under the root:
//! it names the link and exits 1. A list that would write under `schema/`,
//! `.github/` or the init goldens is refused before any file is read, with
//! exit 2.
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::ffi::OsStr;
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
    copy: Vec<Entry>,
}

/// One `[[copy]]` table: a source and every copy of it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    from: String,
    to: Vec<String>,
}

/// One copy and its source.
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

#[expect(clippy::too_many_lines, reason = "over the kit line threshold")]
fn upstream_diff(name: Option<&str>) -> ExitCode {
    if name != Some("beautifulrust") {
        eprintln!("error: upstream-diff needs beautifulrust");
        return ExitCode::from(2);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let lock = match fs::read_to_string(root.join("rust-tops.lock")) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };
    let Some(line) = lock
        .lines()
        .find(|line| line.starts_with("beautifulrust = "))
    else {
        eprintln!("error: rust-tops.lock has no beautifulrust pin");
        return ExitCode::from(2);
    };
    let pin = line.split('"').nth(1).unwrap_or("");
    let Some(sha) = pin.split_whitespace().nth(1) else {
        eprintln!("error: beautifulrust pin has no commit");
        return ExitCode::from(2);
    };
    let listed = std::process::Command::new("git")
        .args([
            "ls-remote",
            "https://github.com/morganlinton/BeautifulRust.git",
            "refs/tags/v1.5.0",
        ])
        .output();
    let Ok(listed) = listed else {
        eprintln!("error: git ls-remote failed to start");
        return ExitCode::from(2);
    };
    let text = String::from_utf8_lossy(&listed.stdout);
    let remote = text.split_whitespace().next().unwrap_or("");
    if remote.starts_with(sha) {
        println!("beautifulrust {pin} no change");
        ExitCode::SUCCESS
    } else {
        println!("beautifulrust {pin} changed {remote}");
        ExitCode::SUCCESS
    }
}

fn main() -> ExitCode {
    let mut raw = std::env::args().skip(1);
    if raw.next().as_deref() == Some("upstream-diff") {
        return upstream_diff(raw.next().as_deref());
    }
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

#[expect(
    clippy::fn_params_excessive_bools,
    reason = "already in this crate when the strict table was turned on"
)]
/// Check or write every pair. Returns whether every dest matches its source
/// when sync is done.
#[expect(clippy::too_many_lines, reason = "over the kit line threshold")]
fn sync(root: &Path, check: bool) -> Result<bool> {
    let pairs = read_list(root)?;
    let mut problems = Vec::new();
    let mut wrote = 0;
    for pair in &pairs {
        let source = read_source(&root.join(&pair.source))?;
        let dest = root.join(&pair.dest);
        if let Some(dir) = linked_dir(root, &pair.dest) {
            problems.push(format!(
                "{} is under {dir}, which is a symlink; copies sit in real directories, so remove the link {dir}",
                pair.dest
            ));
            continue;
        }
        let problem = match state(&dest, &source)? {
            State::Same => continue,
            State::Symlink => "is a symlink; copies are regular files, so remove the link",
            State::NotAFile => "is not a regular file",
            _ if !check => {
                write(root, &pair.dest, &source)?;
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
    let mut froms = HashSet::new();
    for entry in &list.copy {
        if !froms.insert(entry.from.as_str()) {
            bail!("from {}: listed twice; one table per source", entry.from);
        }
        if entry.to.is_empty() {
            bail!("from {}: its `to` lists no copy", entry.from);
        }
    }
    let pairs: Vec<Pair> = list
        .copy
        .into_iter()
        .flat_map(|entry| {
            let source = entry.from;
            entry.to.into_iter().map(move |dest| Pair {
                source: source.clone(),
                dest,
            })
        })
        .collect();
    validate(&pairs)?;
    Ok(pairs)
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

/// The first directory above `rel`, under `root`, that is a symlink. One
/// that is not there yet, or cannot be read, is not a link: `write` makes
/// it, or `state` reports why the copy cannot be read.
fn linked_dir(root: &Path, rel: &str) -> Option<String> {
    let mut dir = PathBuf::new();
    let parents = Path::new(rel).parent().map(Path::components);
    for part in parents.into_iter().flatten() {
        dir.push(part);
        match fs::symlink_metadata(root.join(&dir)) {
            Ok(meta) if meta.file_type().is_symlink() => return Some(dir.display().to_string()),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    None
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

/// Write `root/rel` through a new file beside it, then rename. A rename
/// replaces the name, so a link that appeared at the dest since `state` is
/// replaced, not followed.
///
/// [`Parent::open`] says how the directories are made and how far that
/// holds against a link swapped in while sync runs.
fn write(root: &Path, rel: &str, source: &Source) -> Result<()> {
    let name = Path::new(rel)
        .file_name()
        .ok_or_else(|| anyhow!("{rel}: no file name"))?;
    let temp = format!(".{}.xtask-sync", name.to_string_lossy());
    let parent = Parent::open(root, rel)?;
    let temp_path = parent.dir.join(&temp);
    // A temp file left by an interrupted sync. If it cannot go, `create_new`
    #[expect(
        clippy::let_underscore_must_use,
        reason = "already in this crate when the strict table was turned on"
    )]
    // below fails and names it.
    let _ = parent.remove(temp.as_ref());
    let mut file = parent
        .create_new(temp.as_ref())
        .with_context(|| format!("create {}", temp_path.display()))?;
    file.write_all(&source.bytes)
        .with_context(|| format!("write {}", temp_path.display()))?;
    set_executable(&file, &temp_path, source.executable)?;
    drop(file);
    parent
        .rename(temp.as_ref(), name)
        .with_context(|| format!("rename to {}", root.join(rel).display()))
}

fn refused(rel: &str, dir: &Path) -> anyhow::Error {
    anyhow!(
        "{rel}: {} is a symlink or not a directory; sync does not follow it",
        dir.display()
    )
}

/// The directory a copy is written into, held open.
#[cfg(unix)]
struct Parent {
    dir: PathBuf,
    fd: std::os::fd::OwnedFd,
}

/// The directory a copy is written into off Unix, as a path.
#[cfg(not(unix))]
type Parent = PathParent;

/// The path walk. It is built on Unix too, so its tests run there.
#[cfg_attr(all(unix, not(test)), expect(dead_code, reason = "the walk off Unix"))]
struct PathParent {
    dir: PathBuf,
}

#[cfg(unix)]
impl Parent {
    /// Opens `root`, then each directory above `rel`, making the ones that
    /// are missing.
    ///
    /// `root` is opened by path, so a link at `root` itself is followed.
    /// Below it nothing is looked up by path again: each directory is made
    /// with `mkdirat` and opened with `O_NOFOLLOW` relative to the one
    /// above, and the temp file, its mode and the rename are relative to the
    /// last. A link at any of those names, there before the walk or swapped
    /// in during it, fails the open and sync stops. So does a name that is
    /// gone again by the time it is opened.
    fn open(root: &Path, rel: &str) -> Result<Self> {
        use rustix::fs::{mkdirat, openat, Mode, OFlags, CWD};
        use rustix::io::Errno;
        // `union`, not `|`: on disjoint bits a mutated `^` is the same set,
        // a mutant no test can catch.
        let flags = OFlags::RDONLY
            .union(OFlags::DIRECTORY)
            .union(OFlags::CLOEXEC);
        let mut fd = openat(CWD, root, flags, Mode::empty())
            .with_context(|| format!("open {}", root.display()))?;
        let mut dir = root.to_path_buf();
        let parents = Path::new(rel).parent().map(Path::components);
        for part in parents.into_iter().flatten() {
            let name = part.as_os_str();
            dir.push(name);
            match mkdirat(&fd, name, Mode::from_raw_mode(0o777)) {
                Ok(()) | Err(Errno::EXIST) => {}
                Err(err) => return Err(err).with_context(|| format!("create {}", dir.display())),
            }
            fd = match openat(&fd, name, flags.union(OFlags::NOFOLLOW), Mode::empty()) {
                Ok(next) => next,
                Err(Errno::LOOP | Errno::NOTDIR | Errno::NOENT) => return Err(refused(rel, &dir)),
                Err(err) => return Err(err).with_context(|| format!("open {}", dir.display())),
            };
        }
        Ok(Self { dir, fd })
    }

    fn remove(&self, name: &OsStr) -> std::io::Result<()> {
        rustix::fs::unlinkat(&self.fd, name, rustix::fs::AtFlags::empty()).map_err(Into::into)
    }

    fn create_new(&self, name: &OsStr) -> std::io::Result<fs::File> {
        use rustix::fs::{openat, Mode, OFlags};
        let flags = OFlags::WRONLY
            .union(OFlags::CREATE)
            .union(OFlags::EXCL)
            .union(OFlags::NOFOLLOW)
            .union(OFlags::CLOEXEC);
        let fd = openat(&self.fd, name, flags, Mode::from_raw_mode(0o666))?;
        Ok(fs::File::from(fd))
    }

    fn rename(&self, from: &OsStr, to: &OsStr) -> std::io::Result<()> {
        rustix::fs::renameat(&self.fd, from, &self.fd, to).map_err(Into::into)
    }
}

#[cfg_attr(all(unix, not(test)), expect(dead_code, reason = "the walk off Unix"))]
impl PathParent {
    /// Each directory above the dest is made with `create_dir` before
    /// anything looks at it, and a name it could not make must be a real
    /// directory. This walk goes by path, so the window is still open: a
    /// directory removed and replaced by a link after it was checked and
    /// before the temp file is made is followed.
    fn open(root: &Path, rel: &str) -> Result<Self> {
        let mut dir = root.to_path_buf();
        let parents = Path::new(rel).parent().map(Path::components);
        for part in parents.into_iter().flatten() {
            dir.push(part);
            if fs::create_dir(&dir).is_err()
                && !fs::symlink_metadata(&dir).is_ok_and(|meta| meta.is_dir())
            {
                return Err(refused(rel, &dir));
            }
        }
        Ok(Self { dir })
    }

    fn remove(&self, name: &OsStr) -> std::io::Result<()> {
        fs::remove_file(self.dir.join(name))
    }

    fn create_new(&self, name: &OsStr) -> std::io::Result<fs::File> {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.dir.join(name))
    }

    fn rename(&self, from: &OsStr, to: &OsStr) -> std::io::Result<()> {
        fs::rename(self.dir.join(from), self.dir.join(to))
    }
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

#[expect(
    clippy::fn_params_excessive_bools,
    reason = "already in this crate when the strict table was turned on"
)]
/// Sets the mode through the open file, so it is the temp file's own.
fn set_executable(file: &fs::File, path: &Path, executable: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable { 0o755 } else { 0o644 };
        file.set_permissions(fs::Permissions::from_mode(mode))
            .with_context(|| format!("chmod {}", path.display()))
    }
    #[cfg(not(unix))]
    {
        let _ = (file, path, executable);
        Ok(())
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    /// The directories are held open, so a directory swapped for a link
    /// after the walk is not where the copy goes: the temp file and the
    /// rename land in the real directory, and nothing goes behind the link.
    #[test]
    fn a_directory_swapped_for_a_link_after_the_walk_is_not_followed() {
        let root = std::env::temp_dir().join(format!("xtask-parent-swap-{}", std::process::id()));
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&root);
        let repo = root.join("repo");
        let outside = root.join("outside");
        fs::create_dir_all(&outside).expect("mkdir outside");
        fs::create_dir_all(&repo).expect("mkdir repo");
        let parent = Parent::open(&repo, "config/AGENTS.md").expect("walk");
        fs::rename(repo.join("config"), repo.join("config-aside")).expect("move aside");
        std::os::unix::fs::symlink(&outside, repo.join("config")).expect("link");
        let mut file = parent
            .create_new(OsStr::new(".AGENTS.md.xtask-sync"))
            .expect("create");
        file.write_all(b"copy\n").expect("write");
        set_executable(&file, &parent.dir, false).expect("chmod");
        drop(file);
        parent
            .rename(OsStr::new(".AGENTS.md.xtask-sync"), OsStr::new("AGENTS.md"))
            .expect("rename");
        assert_eq!(
            fs::read(repo.join("config-aside/AGENTS.md")).expect("read"),
            b"copy\n"
        );
        assert_eq!(fs::read_dir(&outside).expect("read outside").count(), 0);
        // A walk that starts now meets the link and refuses it.
        let err = Parent::open(&repo, "config/AGENTS.md")
            .err()
            .expect("refused");
        assert!(
            err.to_string()
                .contains("is a symlink or not a directory; sync does not follow it"),
            "{err}"
        );
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&root);
    }

    /// `create_new` makes a file and nothing else. A name taken by a file,
    /// a link or a dangling link fails it, and what a link points at is left
    /// as it was.
    fn taken_names_fail(
        dir: &Path,
        outside: &Path,
        create: impl Fn(&OsStr) -> std::io::Result<fs::File>,
    ) {
        fs::write(dir.join("file.md"), "old\n").expect("write file");
        std::os::unix::fs::symlink(outside, dir.join("link.md")).expect("link");
        std::os::unix::fs::symlink(dir.join("gone"), dir.join("dangling.md")).expect("link");
        for name in ["file.md", "link.md", "dangling.md"] {
            assert!(create(OsStr::new(name)).is_err(), "{name}");
        }
        assert_eq!(
            fs::read_to_string(dir.join("file.md")).expect("read"),
            "old\n"
        );
        assert_eq!(fs::read_to_string(outside).expect("read"), "keep\n");
        assert!(!dir.join("gone").exists());
        drop(create(OsStr::new("new.md")).expect("create"));
        assert!(dir.join("new.md").is_file());
    }

    /// `write` removes a stale temp file before `create_new`, so only a name
    /// taken in between reaches it. Both walks refuse it.
    #[test]
    fn create_new_refuses_a_name_that_is_taken_on_either_walk() {
        let root = std::env::temp_dir().join(format!("xtask-create-new-{}", std::process::id()));
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("mkdir");
        let outside = root.join("outside.md");
        fs::write(&outside, "keep\n").expect("write outside");
        for held in [true, false] {
            let repo = root.join(if held { "held" } else { "path" });
            fs::create_dir_all(&repo).expect("mkdir repo");
            let dir = repo.join("config");
            if held {
                let parent = Parent::open(&repo, "config/AGENTS.md").expect("walk");
                taken_names_fail(&dir, &outside, |name| parent.create_new(name));
            } else {
                let parent = PathParent::open(&repo, "config/AGENTS.md").expect("walk");
                taken_names_fail(&dir, &outside, |name| parent.create_new(name));
            }
        }
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&root);
    }

    /// A link or a regular file where a directory should be is refused by
    /// the held walk and by the path walk, and nothing is made behind it.
    #[test]
    fn a_link_or_a_file_where_a_directory_should_be_is_refused_by_either_walk() {
        let root = std::env::temp_dir().join(format!("xtask-walk-refuse-{}", std::process::id()));
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&root);
        let repo = root.join("repo");
        let outside = root.join("outside");
        fs::create_dir_all(&outside).expect("mkdir outside");
        fs::create_dir_all(&repo).expect("mkdir repo");
        std::os::unix::fs::symlink(&outside, repo.join("linked")).expect("link");
        fs::write(repo.join("file"), "keep\n").expect("write file");
        for (rel, dir) in [("linked/sub/a.md", "linked"), ("file/sub/a.md", "file")] {
            let want = format!(
                "{rel}: {} is a symlink or not a directory; sync does not follow it",
                repo.join(dir).display()
            );
            let held = Parent::open(&repo, rel).err().expect("held walk refused");
            let walked = PathParent::open(&repo, rel)
                .err()
                .expect("path walk refused");
            assert_eq!(held.to_string(), want);
            assert_eq!(walked.to_string(), want);
        }
        assert_eq!(fs::read_dir(&outside).expect("read outside").count(), 0);
        assert_eq!(
            fs::read_to_string(repo.join("file")).expect("read"),
            "keep\n"
        );
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&root);
    }

    /// The path walk `write` takes off Unix, run here: it makes the missing
    /// directories, walks real ones again, and writes, renames and removes
    /// in the last.
    #[test]
    fn the_path_walk_makes_directories_and_writes_renames_and_removes_in_them() {
        let root = std::env::temp_dir().join(format!("xtask-path-walk-{}", std::process::id()));
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&root);
        let repo = root.join("repo");
        fs::create_dir_all(&repo).expect("mkdir repo");
        let parent = PathParent::open(&repo, "a/b/f.md").expect("walk");
        assert_eq!(parent.dir, repo.join("a/b"));
        assert!(parent.dir.is_dir());
        PathParent::open(&repo, "a/b/f.md").expect("walk real dirs again");
        let mut file = parent
            .create_new(OsStr::new(".f.md.xtask-sync"))
            .expect("create");
        file.write_all(b"copy\n").expect("write");
        drop(file);
        parent
            .rename(OsStr::new(".f.md.xtask-sync"), OsStr::new("f.md"))
            .expect("rename");
        assert_eq!(fs::read(repo.join("a/b/f.md")).expect("read"), b"copy\n");
        assert!(!repo.join("a/b/.f.md.xtask-sync").exists());
        parent.remove(OsStr::new("f.md")).expect("remove");
        assert!(!repo.join("a/b/f.md").exists());
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&root);
    }
}
