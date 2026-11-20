#![cfg_attr(
    test,
    expect(
        clippy::needless_pass_by_value,
        reason = "this crate already violates the strict table"
    )
)]
#![cfg_attr(
    test,
    expect(clippy::panic, reason = "this crate already violates the strict table")
)]
#![expect(
    elided_lifetimes_in_paths,
    reason = "this crate already violates the strict table"
)]
use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod config;
mod coverage_floor;
mod crap_ratchet;
mod credit;
mod evidence;
mod exemptions;
mod facts;
mod floors;
mod gate;
mod init_meta;
mod laws;
mod lock;
mod mutation;
mod profile_data;
mod profiles;
mod schema;
mod scope;
mod skills;
mod steps;
mod strict_lints;
mod tiers;

#[cfg(test)]
mod parse_oracle;

use config::TopsConfig;

#[derive(Parser)]
#[command(
    name = "cargo-tops",
    bin_name = "cargo-tops",
    version,
    about = "Rust-TOPS protocol CLI: init, check, gate. Policy and sequencing only — not a test engine."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// The flags of `init`.
#[derive(clap::Args)]
struct InitArgs {
    /// Crate class: one of the profiles in `profiles/` (default: library-core)
    #[arg(long, default_value = "library-core")]
    class: String,
    /// Directory to initialize (default: cwd)
    #[arg(long, default_value = ".")]
    path: PathBuf,
    /// Crate name written into rust-tops.yaml (default: directory name)
    #[arg(long)]
    name: Option<String>,
    /// Overwrite existing protocol files
    #[arg(long)]
    force: bool,
    /// Also write portable agent skills, CLAUDE.md, and docs/REVIEW.md
    #[arg(long)]
    skills: bool,
    /// Also write the Bend laws kit and turn the laws layer on
    #[arg(long)]
    laws: bool,
}

#[derive(clap::Args)]
struct LockArgs {
    /// Ask each pinned tool for its version and compare it with the pin
    #[arg(long, conflicts_with = "write", required_unless_present = "write")]
    verify: bool,
    /// Write the versions the tools print into the [tools] values, keeping comments and order
    #[arg(long)]
    write: bool,
    /// With --verify: only the tools the enabled layers at or below this tier need
    #[arg(long, requires = "verify")]
    max_tier: Option<u8>,
    #[arg(long, default_value = ".")]
    path: PathBuf,
}

impl LockArgs {
    /// `--verify` (with its tier) or, without it, `--write`.
    fn action(&self) -> lock::Action {
        if self.verify {
            lock::Action::Verify {
                max_tier: self.max_tier,
            }
        } else {
            lock::Action::Write
        }
    }
}

/// The commands that act on a project: the directory `--path` names, with its
/// `rust-tops.yaml` and `rust-tops.lock`.
#[derive(Subcommand)]
enum ProjectCommand {
    /// Validate rust-tops.yaml against the protocol schema (typed check)
    Check {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Run enabled Tier 0–1 tools in protocol order, then the Bend laws layer
    #[command(after_help = "Writes <target>/rust-tops/report.json on every run.\n\
Exit codes:\n  0  pass\n  1  the gate ran and failed (a dry run included)\n  \
2  the config is invalid, or the request cannot be gated: rust-tops.yaml missing, unreadable \
or failing check, no enabled layer at or below --max-tier, or rust-tops.lock unreadable \
when a layer in range needs a pinned tool (every pinned tool a running layer needs reads \
the lock); a command-line usage error is 2 too and writes no report. An expired \
exemption is not 2: it is 1\n  \
3  the environment: a required tool or cargo metadata is missing, a pinned tool prints \
another version than its pin in rust-tops.lock, or report.json cannot be written\n\
Where more than one applies, 2 wins over 3, and 3 over 1.")]
    Gate {
        #[arg(long, default_value = ".")]
        path: PathBuf,
        /// Only run layers at or below this tier (default 1)
        #[arg(long, default_value_t = 1)]
        max_tier: u8,
        /// Print commands without running them
        #[arg(long)]
        dry_run: bool,
        /// Ref the mutation diff starts from, over `gates.base_ref` and the
        /// remote's default branch (`origin/HEAD`). In CI pass
        /// `origin/<default branch>` from a checkout with `fetch-depth: 0`
        #[arg(long, value_name = "REF")]
        base: Option<String>,
    },
    /// Check or rewrite the tool pins in rust-tops.lock against the tools on PATH
    #[command(
        after_help = "Exit codes:\n  0  every pin matches (verify) or the lock is written\n  \
2  rust-tops.lock is unreadable, or the config does not load for --max-tier, or a usage error\n  \
3  a pinned tool is missing or prints another version than its pin"
    )]
    Lock(LockArgs),
    /// Record the last gate's CRAP report as the baseline the crap ratchet reads
    CrapBaseline {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum Commands {
    /// Write rust-tops.yaml, rust-tops.lock, AGENTS.md, RUST-TOPS.md and the drop-in tool configs, read from the crate's Cargo.toml
    Init(InitArgs),
    #[command(flatten)]
    Project(ProjectCommand),
    /// Print the package version, the protocol it implements, and the pins in ./rust-tops.lock
    Version,
}

/// The protocol version this build implements.
const PROTOCOL_VERSION: &str = "1.3.0";

/// What `cargo tops version` prints: package, protocol, then the `[tools]`
/// pins of the lock text, or one line saying why there are none.
/// `lock` is `None` when `./rust-tops.lock` does not exist.
fn version_text(lock: Option<&str>) -> String {
    let mut lines = vec![
        format!("cargo-tops {}", env!("CARGO_PKG_VERSION")),
        format!("protocol {PROTOCOL_VERSION}"),
    ];
    match lock.map(str::parse::<toml::Table>) {
        None => lines.push("tool pins: none (no ./rust-tops.lock)".to_string()),
        Some(Err(err)) => {
            lines.push(format!(
                "tool pins: ./rust-tops.lock is not valid TOML: {err}"
            ));
        }
        Some(Ok(table)) => {
            lines.push("tool pins (./rust-tops.lock):".to_string());
            let pins = table.get("tools").and_then(toml::Value::as_table);
            for (name, pin) in pins.into_iter().flatten() {
                lines.push(format!("  {name} = {}", pin.as_str().unwrap_or("?")));
            }
        }
    }
    lines.push(String::new());
    lines.join("\n")
}

/// `cargo tops version`: reads `./rust-tops.lock` from the current directory.
fn print_version() {
    let lock = fs::read_to_string("rust-tops.lock").ok();
    print!("{}", version_text(lock.as_deref()));
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(2)
        }
    }
}

/// The argv clap parses. `cargo tops …` execs this binary with `argv[1]` ==
/// "tops"; `cargo-tops …` does not. Drop that one word so both forms parse.
fn subcommand_argv(mut args: Vec<std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    if args.get(1).and_then(|arg| arg.to_str()) == Some("tops") {
        args.remove(1);
    }
    args
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse_from(subcommand_argv(std::env::args_os().collect()));
    match cli.command {
        Commands::Init(args) => {
            init(&args)?;
            Ok(ExitCode::SUCCESS)
        }
        Commands::Project(command) => run_project(command),
        Commands::Version => {
            print_version();
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// A command that acts on a project.
fn run_project(command: ProjectCommand) -> Result<ExitCode> {
    match command {
        ProjectCommand::Check { path } => check_command(&path),
        ProjectCommand::Gate {
            path,
            max_tier,
            dry_run,
            base,
        } => Ok(run_gate(
            &path,
            gate::Options {
                max_tier,
                mode: dry_run.into(),
            },
            base.as_deref(),
        )),
        ProjectCommand::Lock(args) => lock::command(&args.path, &args.action()),
        ProjectCommand::CrapBaseline { path } => {
            let cfg = load_config(&path)?;
            floors::record_baseline(&path, &cfg)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// `cargo tops check`: an expired exemption is a warning here and a failure
/// of the gate; the config is still valid.
fn check_command(path: &Path) -> Result<ExitCode> {
    let cfg = load_config(path)?;
    check(&cfg)?;
    for problem in gate::judge::expired_problems(&cfg) {
        println!("warning: {problem}");
    }
    println!(
        "ok: {} ({}) matches rust-tops {}",
        cfg.crate_.name, cfg.crate_.class, cfg.protocol.version
    );
    Ok(ExitCode::SUCCESS)
}

/// `cargo tops gate`: a config that does not load, or fails `check`, is exit
/// 2 with a report; anything else is the gate's own verdict.
fn run_gate(path: &Path, opts: gate::Options, base: Option<&str>) -> ExitCode {
    match load_config(path).and_then(|cfg| check(&cfg).map(|()| cfg)) {
        Ok(cfg) => gate::run(path, &cfg, opts, base),
        Err(err) => gate::config_error(path, &err, opts),
    }
}

/// `rust-tops.yaml` under `path`, typed, with a class profile, and valid
/// against `schema/rust-tops.schema.json`, with tiers and floors that hold the
/// class (`tiers::check`). Any of the four failing is a `check` failure.
fn load_config(path: &Path) -> Result<TopsConfig> {
    let file = config_path(path);
    let raw = fs::read_to_string(&file)
        .with_context(|| format!("missing {}; run `cargo tops init`", file.display()))?;
    let cfg = parse_config(&file, &raw)?;
    profiles::profile_yaml(&cfg.crate_.class, &cfg.crate_.name)
        .with_context(|| format!("{}: crate.class", file.display()))?;
    let doc = facts::parse_yaml(&raw)
        .map_err(|err| anyhow!("invalid YAML: {}: {err}", file.display()))?;
    let schema: serde_json::Value =
        serde_json::from_str(profiles::SCHEMA_JSON).context("bundled rust-tops.schema.json")?;
    let errors = schema::validate(&schema, &doc);
    if !errors.is_empty() {
        return Err(anyhow!(
            "{} does not match schema/rust-tops.schema.json:\n  {}",
            file.display(),
            errors.join("\n  ")
        ));
    }
    let problems = tiers::check(&file.display().to_string(), &doc, &exemptions::today());
    if !problems.is_empty() {
        return Err(anyhow!("{}", problems.join("\n")));
    }
    Ok(cfg)
}

/// The typed decode alone, with YAML 1.2 booleans.
fn parse_config(file: &Path, raw: &str) -> Result<TopsConfig> {
    // YAML 1.2 booleans: only true/false. Unquoted yes/no/on/off/y/n stay strings.
    serde_saphyr::from_str_with_options(
        raw,
        serde_saphyr::options! {
            strict_booleans: true,
        },
    )
    .with_context(|| format!("invalid YAML: {}", file.display()))
}

fn config_path(dir: &Path) -> PathBuf {
    dir.join("rust-tops.yaml")
}

fn check(cfg: &TopsConfig) -> Result<()> {
    if cfg.protocol.id != "rust-tops" {
        return Err(anyhow!("protocol.id must be rust-tops"));
    }
    if cfg.protocol.version.is_empty() {
        return Err(anyhow!("protocol.version is required"));
    }
    if cfg.crate_.name.is_empty() {
        return Err(anyhow!("crate.name is required"));
    }
    if cfg
        .gates
        .crap
        .threshold_agent
        .is_some_and(|agent| agent < 1.0)
    {
        return Err(anyhow!("gates.crap.threshold_agent must be >= 1"));
    }
    Ok(())
}

#[expect(clippy::too_many_lines, reason = "over the kit line threshold")]
fn init(args: &InitArgs) -> Result<()> {
    let (dir, class, name) = (
        args.path.as_path(),
        args.class.as_str(),
        args.name.as_deref(),
    );
    let (force, skills, laws) = (args.force, args.skills, args.laws);
    let fallback = dir_name(dir).unwrap_or_else(|| "unnamed".to_string());
    let rustc = lock_rustc(dir)?;
    let plan = init_meta::plan(&init_meta::Request {
        dir,
        name_flag: name,
        dir_name: &fallback,
        lock_rustc: &rustc,
        laws,
    })?;
    let crate_name = plan.name.as_str();
    let msrv = plan.settings.msrv.as_deref().unwrap_or(&rustc);
    // An unknown class fails here, before any directory or file is written.
    let yaml = with_init_exemptions(&profiles::profile_yaml_with(
        class,
        crate_name,
        &plan.settings,
    )?);

    // The directory named on the command line may sit behind a link; the
    // directories below it may not, and `write_file` makes those.
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    write_file(dir, "rust-tops.yaml", &yaml, force)?;
    write_file(dir, "rust-tops.lock", profiles::LOCK_TEMPLATE, force)?;
    if plan.write_toolchain {
        write_file(
            dir,
            "rust-toolchain.toml",
            &profiles::rust_toolchain_toml(msrv),
            force,
        )?;
    }
    write_file(dir, "AGENTS.md", profiles::AGENTS_MD, force)?;
    write_file(
        dir,
        "RUST-TOPS.md",
        &profiles::adoption_record(&yaml),
        force,
    )?;
    write_file(dir, "clippy.toml", &profiles::clippy_toml(class)?, force)?;
    write_file(dir, ".config/nextest.toml", profiles::NEXTEST_TOML, force)?;
    write_file(dir, ".cargo/mutants.toml", profiles::MUTANTS_TOML, force)?;
    write_file(dir, ".cargo-crap.toml", profiles::CARGO_CRAP_TOML, force)?;
    write_file(dir, "deny.toml", profiles::DENY_TOML, force)?;
    write_file(
        dir,
        ".github/workflows/rust-tops.yml",
        &profiles::workflow_yml(class),
        force,
    )?;
    if laws {
        write_file(dir, laws::PACK, profiles::LAWS_BEND, force)?;
        write_file(dir, laws::LOCK, profiles::BEND_LOCK, force)?;
        if write_file(dir, laws::FETCH, profiles::FETCH_BEND_SH, force)? {
            make_executable(&dir.join(laws::FETCH))?;
        }
    }
    if skills {
        skills::emit(dir, force)?;
        println!(
            "note: the skills run on Nushell (nu) 0.108.0, a required dependency; see the README"
        );
    }
    print_init_notes(&plan, &yaml);

    println!(
        "initialized rust-tops for {crate_name} ({class}) in {}",
        dir.display()
    );
    println!("next: cargo tops check && cargo tops gate");
    Ok(())
}

/// The `rustc` pin that stands in for an absent `rust-version`: the lock
/// already in `dir`, else the template `init` writes.
fn lock_rustc(dir: &Path) -> Result<String> {
    let there = fs::read_to_string(dir.join("rust-tops.lock")).ok();
    there
        .as_deref()
        .and_then(init_meta::lock_rustc)
        .or_else(|| init_meta::lock_rustc(profiles::LOCK_TEMPLATE))
        .ok_or_else(|| anyhow!("rust-tops.lock has no [tools] rustc"))
}

/// One line for each value `init` chose without the crate saying it.
fn print_init_notes(plan: &init_meta::Plan, yaml: &str) {
    for note in &plan.notes {
        println!("{note}");
    }
    let policy = facts::parse_yaml(yaml)
        .ok()
        .and_then(|doc| {
            doc.pointer("/crate/unsafe_policy")?
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_default();
    if let Some(note) = init_meta::unsafe_note(&plan.settings, &policy) {
        println!("{note}");
    }
    if !plan.write_toolchain {
        println!("note: rust-toolchain.toml not written; the crate or a directory above it, up to the workspace, git or filesystem root, already pins a toolchain");
    }
}

/// The last path component of `dir`, which need not exist yet.
fn dir_name(dir: &Path) -> Option<String> {
    let full = dir
        .canonicalize()
        .or_else(|_| std::path::absolute(dir))
        .ok()?;
    full.file_name().map(|s| s.to_string_lossy().into_owned())
}

/// Writes `root/rel`. Returns whether the file was written (false when it
/// existed and `force` was off).
///
/// A symlink at the file is refused, with or without `force`, and so is a
/// dangling one: the write never follows a link. So is a link at any
/// directory between `root` and the file. `force` removes the old file and
/// writes a new one with `create_new`, which fails rather than follow a link
/// that appears in between.
///
/// [`Parent::open`] says how the directories are made and how far that
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "already in this crate when the strict table was turned on"
)]
/// holds against a link swapped in while init runs.
pub(crate) fn write_file(
    root: &Path,
    rel: impl AsRef<Path>,
    contents: &str,
    force: bool,
) -> Result<bool> {
    let rel = rel.as_ref();
    let path = root.join(rel);
    let name = rel
        .file_name()
        .ok_or_else(|| anyhow!("refusing to write {}: no file name", path.display()))?;
    let parent = Parent::open(root, rel, &path)?;
    match parent.entry(name) {
        Entry::Symlink => {
            return Err(anyhow!(
                "refusing to write {}: it is a symlink, and init does not follow one",
                path.display()
            ));
        }
        Entry::Other if !force => {
            println!("skip existing {}", path.display());
            return Ok(false);
        }
        Entry::Other => parent
            .remove(name)
            .with_context(|| format!("remove {}", path.display()))?,
        // Nothing there, or nothing that can be read: `create_new` below
        // either makes the file or fails, and never follows a link.
        Entry::Absent => {}
    }
    let mut file = parent
        .create_new(name)
        .with_context(|| format!("create {}", path.display()))?;
    std::io::Write::write_all(&mut file, contents.as_bytes())
        .with_context(|| format!("write {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(true)
}

/// What is at a name in a [`Parent`], without following a link there.
enum Entry {
    Absent,
    Symlink,
    Other,
}

fn refused(path: &Path, dir: &Path) -> anyhow::Error {
    anyhow!(
        "refusing to write {}: {} is a symlink or not a directory, and init does not follow one",
        path.display(),
        dir.display()
    )
}

/// The directory a [`write_file`] writes into, held open on Unix.
#[cfg(unix)]
struct Parent(std::os::fd::OwnedFd);

#[cfg(unix)]
impl Parent {
    /// Opens `root`, then each directory between it and `rel`'s file,
    /// making the ones that are missing.
    ///
    /// `root` is opened by path, so a link at `root` itself is followed.
    /// Below it nothing is looked up by path again: each directory is made
    /// with `mkdirat` and opened with `O_NOFOLLOW` relative to the one
    /// above, and the file is made relative to the last. A link at any of
    /// those names, there before the walk or swapped in during it, fails
    /// the open and the write is refused. So does a name that is gone again
    /// by the time it is opened.
    fn open(root: &Path, rel: &Path, path: &Path) -> Result<Self> {
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
        for part in rel.parent().map(Path::components).into_iter().flatten() {
            let name = part.as_os_str();
            dir.push(name);
            match mkdirat(&fd, name, Mode::from_raw_mode(0o777)) {
                Ok(()) | Err(Errno::EXIST) => {}
                Err(err) => return Err(err).with_context(|| format!("create {}", dir.display())),
            }
            fd = match openat(&fd, name, flags.union(OFlags::NOFOLLOW), Mode::empty()) {
                Ok(next) => next,
                Err(Errno::LOOP | Errno::NOTDIR | Errno::NOENT) => return Err(refused(path, &dir)),
                Err(err) => return Err(err).with_context(|| format!("open {}", dir.display())),
            };
        }
        Ok(Self(fd))
    }

    fn entry(&self, name: &std::ffi::OsStr) -> Entry {
        use rustix::fs::{statat, AtFlags, FileType};
        match statat(&self.0, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) if FileType::from_raw_mode(stat.st_mode) == FileType::Symlink => {
                Entry::Symlink
            }
            Ok(_) => Entry::Other,
            Err(_) => Entry::Absent,
        }
    }

    fn remove(&self, name: &std::ffi::OsStr) -> std::io::Result<()> {
        rustix::fs::unlinkat(&self.0, name, rustix::fs::AtFlags::empty()).map_err(Into::into)
    }

    fn create_new(&self, name: &std::ffi::OsStr) -> std::io::Result<fs::File> {
        use rustix::fs::{openat, Mode, OFlags};
        let flags = OFlags::WRONLY
            .union(OFlags::CREATE)
            .union(OFlags::EXCL)
            .union(OFlags::NOFOLLOW)
            .union(OFlags::CLOEXEC);
        let fd = openat(&self.0, name, flags, Mode::from_raw_mode(0o666))?;
        Ok(fs::File::from(fd))
    }
}

/// The directory a [`write_file`] writes into, as a path off Unix.
#[cfg(not(unix))]
type Parent = PathParent;

/// The path walk. It is built on Unix too, so its tests run there.
#[cfg_attr(all(unix, not(test)), expect(dead_code, reason = "the walk off Unix"))]
struct PathParent(PathBuf);

#[cfg_attr(all(unix, not(test)), expect(dead_code, reason = "the walk off Unix"))]
impl PathParent {
    /// Each directory between `root` and the file is made with `create_dir`
    /// before anything looks at it, and a name it could not make must be a
    /// real directory. This walk goes by path, so the window is still open:
    /// a directory removed and replaced by a link after it was checked and
    /// before the file is created is followed.
    fn open(root: &Path, rel: &Path, path: &Path) -> Result<Self> {
        let mut dir = root.to_path_buf();
        for part in rel.parent().map(Path::components).into_iter().flatten() {
            dir.push(part);
            if fs::create_dir(&dir).is_err()
                && !fs::symlink_metadata(&dir).is_ok_and(|meta| meta.is_dir())
            {
                return Err(refused(path, &dir));
            }
        }
        Ok(Self(dir))
    }

    fn entry(&self, name: &std::ffi::OsStr) -> Entry {
        match fs::symlink_metadata(self.0.join(name)) {
            Ok(meta) if meta.file_type().is_symlink() => Entry::Symlink,
            Ok(_) => Entry::Other,
            Err(_) => Entry::Absent,
        }
    }

    fn remove(&self, name: &std::ffi::OsStr) -> std::io::Result<()> {
        fs::remove_file(self.0.join(name))
    }

    fn create_new(&self, name: &std::ffi::OsStr) -> std::io::Result<fs::File> {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.0.join(name))
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("chmod {}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

/// The tool pins a repo keeps at its root, beside `rust-tops.yaml`. `init`
/// does not write one. Adopters copy `config/rust-tops.lock.example`.
pub(crate) const TOOL_LOCK: &str = "rust-tops.lock";

/// The profile YAML with its `exemptions:` block replaced by
/// [`steps::init_exemptions`], which is empty. An exemption is a dated choice
/// the adopter makes, never one `init` makes for them.
#[expect(clippy::too_many_lines, reason = "over the kit line threshold")]
fn with_init_exemptions(yaml: &str) -> String {
    let mut block = String::new();
    match steps::init_exemptions() {
        [] => block.push_str("exemptions: []\n"),
        entries => {
            block.push_str("exemptions:\n");
            for entry in entries {
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "already in this crate when the strict table was turned on"
                )]
                let _ = writeln!(
                    block,
                    "  - {{ layer: {}, expires: \"{}\" }}",
                    entry.layer, entry.expires
                );
            }
        }
    }
    let mut out = String::new();
    let mut placed = false;
    let mut skipping = false;
    for line in yaml.lines() {
        if skipping {
            let inside = line.trim().is_empty() || line.starts_with([' ', '\t', '-']);
            if inside {
                continue;
            }
            skipping = false;
        }
        if line.split('#').next().map(str::trim_end) == Some("exemptions:") {
            out.push_str(&block);
            placed = true;
            skipping = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !placed {
        out.push_str(&block);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    type Plant = fn(&mut TopsConfig);

    /// The library-core golden, typed.
    fn golden_cfg() -> TopsConfig {
        let raw = include_str!("../tests/golden/init/library-core/rust-tops.yaml");
        parse_config(Path::new("rust-tops.yaml"), raw).expect("golden parses")
    }

    #[test]
    fn check_accepts_the_golden_and_names_each_typed_fault() {
        assert!(check(&golden_cfg()).is_ok());
        let faults: [(&str, Plant); 4] = [
            ("protocol.id must be rust-tops", |c| {
                c.protocol.id = "other".into();
            }),
            ("protocol.version is required", |c| {
                c.protocol.version.clear();
            }),
            ("crate.name is required", |c| c.crate_.name.clear()),
            ("gates.crap.threshold_agent must be >= 1", |c| {
                c.gates.crap.threshold_agent = Some(0.5);
            }),
        ];
        for (message, plant) in faults {
            let mut cfg = golden_cfg();
            plant(&mut cfg);
            let err = check(&cfg).expect_err(message);
            assert_eq!(err.to_string(), message);
        }
    }

    /// Every file in `profiles/`.
    const PROFILE_CLASSES: [&str; 8] = [
        "library-core",
        "parser-codec",
        "no-std-embedded",
        "unsafe-kernel",
        "binary-cli",
        "library-util",
        "ffi-bridge",
        "workspace-root",
    ];

    fn init_yaml(class: &str) -> String {
        with_init_exemptions(&profiles::profile_yaml(class, "x").expect("profile merges"))
    }

    /// Cargo passes the subcommand name; the bare binary does not. The
    /// `cargo_argv` journey runs whichever `cargo-tops` Cargo finds first,
    /// so this rule has its own oracle here.
    #[test]
    fn cargo_s_tops_word_is_dropped_and_nothing_else_is() {
        let argv = |words: &[&str]| -> Vec<std::ffi::OsString> {
            words.iter().map(std::ffi::OsString::from).collect()
        };
        let cases: [(&[&str], &[&str]); 4] = [
            (&["cargo-tops", "tops", "gate"], &["cargo-tops", "gate"]),
            (
                &["cargo-tops", "gate", "tops"],
                &["cargo-tops", "gate", "tops"],
            ),
            (&["cargo-tops", "check"], &["cargo-tops", "check"]),
            (&["cargo-tops"], &["cargo-tops"]),
        ];
        for (given, want) in cases {
            assert_eq!(subcommand_argv(argv(given)), argv(want), "{given:?}");
        }
    }

    #[test]
    fn init_turns_acceptance_and_laws_off_and_keeps_kani_at_tier_4() {
        for class in PROFILE_CLASSES {
            let yaml = init_yaml(class);
            let doc = facts::parse_yaml(&yaml).expect("init yaml parses");
            let layer = |id: &str| {
                #[expect(
                    clippy::indexing_slicing,
                    reason = "already in this crate when the strict table was turned on"
                )]
                doc["layers"]
                    .as_array()
                    .and_then(|layers| layers.iter().find(|l| l["id"] == id))
                    .cloned()
                    .unwrap_or_else(|| panic!("{class}: no {id} layer"))
            };
            #[expect(
                clippy::indexing_slicing,
                reason = "already in this crate when the strict table was turned on"
            )]
            let tops_idx_1593_24 = layer("acceptance")["enabled"].clone();
            assert_eq!(tops_idx_1593_24, false, "{class}");
            #[expect(
                clippy::indexing_slicing,
                reason = "already in this crate when the strict table was turned on"
            )]
            let tops_idx_1598_24 = layer("formal-kani")["tier"].clone();
            assert_eq!(tops_idx_1598_24, 4, "{class}");
            #[expect(
                clippy::indexing_slicing,
                reason = "already in this crate when the strict table was turned on"
            )]
            let tops_idx_1603_24 = layer("laws")["enabled"].clone();
            assert_eq!(tops_idx_1603_24, false, "{class}");
        }
    }

    #[test]
    fn init_yaml_has_no_exemption_for_every_class() {
        for class in PROFILE_CLASSES {
            let yaml = init_yaml(class);
            let doc = facts::parse_yaml(&yaml).expect("init yaml parses");
            #[expect(
                clippy::indexing_slicing,
                reason = "already in this crate when the strict table was turned on"
            )]
            let tops_idx_1641_24 = doc["exemptions"].clone();
            assert_eq!(tops_idx_1641_24, serde_json::json!([]), "{class}");
            assert!(!yaml.contains("generated protobuf"), "{class}");
            #[expect(
                clippy::indexing_slicing,
                reason = "already in this crate when the strict table was turned on"
            )]
            let tops_idx_1647_21 = doc["layers"].clone();
            assert!(tops_idx_1647_21.as_array().is_some_and(|l| !l.is_empty()));
        }
    }

    #[test]
    fn the_exemptions_block_is_replaced_in_place() {
        let yaml = "a: 1\nexemptions:\n  - layer: crap\n    expires: \"2027-01-01\"\nb: 2\n";
        assert_eq!(with_init_exemptions(yaml), "a: 1\nexemptions: []\nb: 2\n");
        assert_eq!(with_init_exemptions("a: 1\n"), "a: 1\nexemptions: []\n");
    }

    /// The directories are held open, so a directory swapped for a link
    /// after the walk is not where the file goes: it lands in the real
    /// directory, wherever that now is, and nothing goes behind the link.
    #[cfg(unix)]
    #[test]
    fn a_directory_swapped_for_a_link_after_the_walk_is_not_followed() {
        let root =
            std::env::temp_dir().join(format!("cargo-tops-parent-swap-{}", std::process::id()));
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&root);
        let repo = root.join("repo");
        let outside = root.join("outside");
        fs::create_dir_all(outside.join("workflows")).expect("mkdir outside");
        fs::create_dir_all(&repo).expect("mkdir repo");
        let rel = Path::new(".github/workflows/rust-tops.yml");
        let parent = Parent::open(&repo, rel, &repo.join(rel)).expect("walk");
        fs::rename(repo.join(".github"), repo.join("github-aside")).expect("move aside");
        std::os::unix::fs::symlink(&outside, repo.join(".github")).expect("link");
        let name = std::ffi::OsStr::new("rust-tops.yml");
        assert!(matches!(parent.entry(name), Entry::Absent));
        drop(parent.create_new(name).expect("create"));
        assert!(repo.join("github-aside/workflows/rust-tops.yml").is_file());
        assert_eq!(
            fs::read_dir(outside.join("workflows"))
                .expect("read outside")
                .count(),
            0
        );
        // A walk that starts now meets the link and refuses it.
        let err = Parent::open(&repo, rel, &repo.join(rel))
            .err()
            .expect("refused");
        assert!(
            err.to_string()
                .contains("is a symlink or not a directory, and init does not follow one"),
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
    #[cfg(unix)]
    fn taken_names_fail(
        dir: &Path,
        outside: &Path,
        create: impl Fn(&std::ffi::OsStr) -> std::io::Result<fs::File>,
    ) {
        fs::write(dir.join("file.md"), "old\n").expect("write file");
        std::os::unix::fs::symlink(outside, dir.join("link.md")).expect("link");
        std::os::unix::fs::symlink(dir.join("gone"), dir.join("dangling.md")).expect("link");
        for name in ["file.md", "link.md", "dangling.md"] {
            assert!(create(std::ffi::OsStr::new(name)).is_err(), "{name}");
        }
        assert_eq!(
            fs::read_to_string(dir.join("file.md")).expect("read"),
            "old\n"
        );
        assert_eq!(fs::read_to_string(outside).expect("read"), "keep\n");
        assert!(!dir.join("gone").exists());
        drop(create(std::ffi::OsStr::new("new.md")).expect("create"));
        assert!(dir.join("new.md").is_file());
    }

    /// `write_file` removes an old file before `create_new`, so only a name
    /// taken in between reaches it. Both walks refuse it.
    #[cfg(unix)]
    #[test]
    fn create_new_refuses_a_name_that_is_taken_on_either_walk() {
        let root =
            std::env::temp_dir().join(format!("cargo-tops-create-new-{}", std::process::id()));
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("mkdir");
        let outside = root.join("outside.md");
        fs::write(&outside, "keep\n").expect("write outside");
        let rel = Path::new("docs/a.md");
        for held in [true, false] {
            let repo = root.join(if held { "held" } else { "path" });
            fs::create_dir_all(&repo).expect("mkdir repo");
            let dir = repo.join("docs");
            if held {
                let parent = Parent::open(&repo, rel, &repo.join(rel)).expect("walk");
                taken_names_fail(&dir, &outside, |name| parent.create_new(name));
            } else {
                let parent = PathParent::open(&repo, rel, &repo.join(rel)).expect("walk");
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
    #[cfg(unix)]
    #[test]
    fn a_link_or_a_file_where_a_directory_should_be_is_refused_by_either_walk() {
        let root =
            std::env::temp_dir().join(format!("cargo-tops-walk-refuse-{}", std::process::id()));
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
        for (at, dir) in [("linked/sub/a.md", "linked"), ("file/sub/a.md", "file")] {
            let (rel, path) = (Path::new(at), repo.join(at));
            let want = format!(
                "refusing to write {}: {} is a symlink or not a directory, and init does not follow one",
                path.display(),
                repo.join(dir).display()
            );
            let held = Parent::open(&repo, rel, &path)
                .err()
                .expect("held walk refused");
            let walked = PathParent::open(&repo, rel, &path)
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

    #[test]
    fn version_text_names_package_protocol_and_each_tools_pin() {
        let pkg = env!("CARGO_PKG_VERSION");
        assert_eq!(
            version_text(None),
            format!("cargo-tops {pkg}\nprotocol 1.3.0\ntool pins: none (no ./rust-tops.lock)\n")
        );
        let lock = "[tools]\nrustc = \"1.99.1\"\n[notes]\nx = \"y\"\n";
        assert_eq!(
            version_text(Some(lock)),
            format!(
                "cargo-tops {pkg}\nprotocol 1.3.0\ntool pins (./rust-tops.lock):\n  rustc = 1.99.1\n"
            )
        );
        assert!(version_text(Some("[tools\n")).contains("is not valid TOML"));
    }

    /// The path walk `write_file` takes off Unix, run here: it makes the
    /// missing directories, walks real ones again, and tells a file from a
    /// link without following it.
    #[cfg(unix)]
    #[test]
    fn the_path_walk_makes_directories_and_reads_what_is_at_a_name() {
        let root =
            std::env::temp_dir().join(format!("cargo-tops-path-walk-{}", std::process::id()));
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&root);
        let repo = root.join("repo");
        fs::create_dir_all(&repo).expect("mkdir repo");
        let rel = Path::new("a/b/f.md");
        let parent = PathParent::open(&repo, rel, &repo.join(rel)).expect("walk");
        assert!(repo.join("a/b").is_dir());
        let again = PathParent::open(&repo, rel, &repo.join(rel)).expect("walk real dirs again");
        let name = std::ffi::OsStr::new("f.md");
        assert!(matches!(parent.entry(name), Entry::Absent));
        drop(parent.create_new(name).expect("create"));
        assert!(matches!(again.entry(name), Entry::Other));
        parent.remove(name).expect("remove");
        assert!(!repo.join(rel).exists());
        assert!(matches!(parent.entry(name), Entry::Absent));
        std::os::unix::fs::symlink(root.join("gone"), repo.join("a/b/l.md")).expect("link");
        assert!(matches!(
            parent.entry(std::ffi::OsStr::new("l.md")),
            Entry::Symlink
        ));
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&root);
    }
}
