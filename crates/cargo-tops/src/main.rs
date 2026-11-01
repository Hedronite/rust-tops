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
use std::process::{Command, ExitCode};
use std::time::SystemTime;

mod config;
mod credit;
mod facts;
mod laws;
mod profile_data;
mod profiles;
mod schema;
mod skills;
mod steps;
mod strict_lints;

#[cfg(test)]
mod parse_oracle;

use config::TopsConfig;
use facts::{Outcome, Run};
use steps::{Kind, MutationPlan, Step};

#[derive(Parser)]
#[command(
    name = "cargo-tops",
    bin_name = "cargo-tops",
    about = "Rust-TOPS protocol CLI: init, check, gate. Policy and sequencing only — not a test engine."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Write rust-tops.yaml, AGENTS.md, drop-in tool configs, and the Bend laws kit
    Init {
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
    },
    /// Validate rust-tops.yaml against the protocol schema (typed check)
    Check {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Run enabled Tier 0–1 tools in protocol order, then the Bend laws layer
    Gate {
        #[arg(long, default_value = ".")]
        path: PathBuf,
        /// Only run layers at or below this tier (default 1)
        #[arg(long, default_value_t = 1)]
        max_tier: u8,
        /// Print commands without running them
        #[arg(long)]
        dry_run: bool,
    },
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

fn run() -> Result<ExitCode> {
    // `cargo tops …` execs this binary with argv[1] == "tops".
    // `cargo-tops …` does not. Drop that one word so both forms parse.
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.get(1).and_then(|arg| arg.to_str()) == Some("tops") {
        args.remove(1);
    }
    let cli = Cli::parse_from(args);
    match cli.command {
        Commands::Init {
            class,
            path,
            name,
            force,
            skills,
        } => {
            init(&path, &class, name.as_deref(), force, skills)?;
            Ok(ExitCode::SUCCESS)
        }
        Commands::Check { path } => {
            let cfg = load_config(&path)?;
            check(&cfg)?;
            println!(
                "ok: {} ({}) matches rust-tops {}",
                cfg.crate_.name, cfg.crate_.class, cfg.protocol.version
            );
            Ok(ExitCode::SUCCESS)
        }
        Commands::Gate {
            path,
            max_tier,
            dry_run,
        } => {
            let cfg = load_config(&path)?;
            check(&cfg)?;
            gate(&path, &cfg, max_tier, dry_run)
        }
    }
}

/// `rust-tops.yaml` under `path`, typed, with a class profile, and valid
/// against `schema/rust-tops.schema.json`. Any of the three failing is a
/// `check` failure.
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
    if cfg.gates.mutation.unexplained_survivors_max != 0 && cfg.crate_.class == "library-core" {
        return Err(anyhow!(
            "library-core cannot raise unexplained_survivors_max above 0"
        ));
    }
    Ok(())
}

#[expect(
    clippy::fn_params_excessive_bools,
    reason = "already in this crate when the strict table was turned on"
)]
#[expect(clippy::too_many_lines, reason = "over the kit line threshold")]
fn init(dir: &Path, class: &str, name: Option<&str>, force: bool, skills: bool) -> Result<()> {
    let crate_name = name
        .map(str::to_string)
        .or_else(|| dir_name(dir))
        .unwrap_or_else(|| "unnamed".to_string());
    // An unknown class fails here, before any directory or file is written.
    let yaml = with_init_exemptions(&profiles::profile_yaml(class, &crate_name)?);
    #[expect(
        clippy::indexing_slicing,
        reason = "already in this crate when the strict table was turned on"
    )]
    let msrv = facts::parse_yaml(&yaml)
        .ok()
        .and_then(|doc| doc["crate"]["msrv"].as_str().map(str::to_string))
        .ok_or_else(|| anyhow!("the {class} profile has no crate.msrv"))?;

    // The directory named on the command line may sit behind a link; the
    // directories below it may not, and `write_file` makes those.
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    write_file(dir, "rust-tops.yaml", &yaml, force)?;
    write_file(
        dir,
        "rust-toolchain.toml",
        &profiles::rust_toolchain_toml(&msrv),
        force,
    )?;
    write_file(dir, "AGENTS.md", profiles::AGENTS_MD, force)?;
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
    write_file(dir, laws::PACK, profiles::LAWS_BEND, force)?;
    write_file(dir, laws::LOCK, profiles::BEND_LOCK, force)?;
    if write_file(dir, laws::FETCH, profiles::FETCH_BEND_SH, force)? {
        make_executable(&dir.join(laws::FETCH))?;
    }
    if skills {
        skills::emit(dir, force)?;
    }

    println!(
        "initialized rust-tops for {crate_name} ({class}) in {}",
        dir.display()
    );
    println!("next: cargo tops check && cargo tops gate");
    Ok(())
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

/// What a layer runs.
enum Plan {
    /// Each chain runs on its own. Inside a chain only a pass continues.
    Chains(Vec<Vec<Step>>),
    /// The diff, judged, then `cargo mutants` on a non-empty diff.
    Mutation(MutationPlan),
}

#[expect(
    clippy::fn_params_excessive_bools,
    reason = "already in this crate when the strict table was turned on"
)]
#[expect(clippy::too_many_lines, reason = "over the kit line threshold")]
fn gate(dir: &Path, cfg: &TopsConfig, max_tier: u8, dry_run: bool) -> Result<ExitCode> {
    // A gate resolves `cargo metadata` afresh, once per project.
    laws::begin_gate();
    let enabled: Vec<&config::Layer> = cfg
        .layers
        .iter()
        .filter(|l| l.enabled && l.tier.unwrap_or(0) <= max_tier)
        .collect();

    if enabled.is_empty() {
        return Err(anyhow!("no enabled layers at or below tier {max_tier}"));
    }

    println!(
        "rust-tops gate: {} ({}) max-tier={max_tier}",
        cfg.crate_.name, cfg.crate_.class
    );

    // The laws layer reads the reports the other layers write, so it runs
    // last. Its judge is fetched first: a missing Bend fails before work.
    let laws_on = enabled.iter().any(|l| l.id == "laws");
    let judge = if laws_on && !dry_run {
        match laws::ensure_bend(dir) {
            Ok(judge) => Some(judge),
            Err(msg) => {
                println!("\nfail: {msg}");
                return Ok(ExitCode::from(1));
            }
        }
    } else {
        None
    };

    // The laws report, the mutation diff, the LCOV and the CRAP report share
    // one directory, which a nonempty `CARGO_TARGET_DIR` moves. Steps run
    // with `dir` as the working directory, so it must not be relative to the
    // caller's.
    let report = std::path::absolute(laws::artifact_dir(dir))
        .with_context(|| format!("resolve the report dir of {}", dir.display()))?;
    // Enabled layers above the range are not judged, so they cannot fail
    // the gate. Name them so the run says what it left out.
    let above: Vec<&str> = cfg
        .layers
        .iter()
        .filter(|l| l.enabled && l.tier.unwrap_or(0) > max_tier)
        .map(|l| l.id.as_str())
        .collect();
    if !above.is_empty() {
        println!(
            "above --max-tier {max_tier}, not judged: {}",
            above.join(", ")
        );
    }
    // A layer turned off still shows in the run: it is not run, not omitted.
    let disabled: Vec<&str> = cfg
        .layers
        .iter()
        .filter(|l| !l.enabled)
        .map(|l| l.id.as_str())
        .collect();
    if !disabled.is_empty() {
        println!("not run (enabled: false): {}", disabled.join(", "));
    }

    let started = SystemTime::now();
    let mut runs: Vec<Run> = Vec::new();
    // Enabled, in range, and no plan: a skip is never a pass (H-37).
    let mut not_run: Vec<String> = Vec::new();
    let mut listed = 0;
    let tests = TestCommands::new(&enabled, &report);
    let crap_pin = if enabled.iter().any(|layer| layer.id == "crap") {
        tool_pin(dir, "cargo-crap")?
    } else {
        None
    };
    // Layers that add no command, each with the layer whose run it takes.
    let mut shared: Vec<(&str, &str)> = Vec::new();
    for layer in enabled.iter().filter(|l| l.id != "laws") {
        if let Some(from) = tests.shared_from(&layer.id) {
            println!(
                "\n==> [{}] no command of its own: it takes the {from} run's outcome",
                layer.id
            );
            shared.push((&layer.id, from));
            continue;
        }
        let Some(plan) = plan_for(layer, &report) else {
            let tier = layer.tier.unwrap_or(0);
            println!(
                "\nNotRun [{}]: enabled at tier {tier} (<= --max-tier {max_tier}) and no command is mapped",
                layer.id
            );
            not_run.push(format!(
                "{}: NotRun (enabled at tier {tier}, no command mapped; disable it or lower --max-tier)",
                layer.id
            ));
            continue;
        };
        let chains = match plan {
            Plan::Chains(chains) => chains,
            Plan::Mutation(plan) => {
                println!(
                    "\n==> [{}] {} && {}",
                    layer.id,
                    plan.diff.display(),
                    plan.mutants.display()
                );
                listed += 1;
                if !dry_run {
                    runs.push(run_mutation(dir, &layer.id, &plan)?);
                }
                continue;
            }
        };
        let pin = crap_pin.as_deref().filter(|_| layer.id == "crap");
        for chain in chains {
            let mut shown: Vec<String> = pin.map(|_| CRAP_VERSION.join(" ")).into_iter().collect();
            shown.extend(chain.iter().map(Step::display));
            println!("\n==> [{}] {}", layer.id, shown.join(" && "));
            if let Some(pin) = pin {
                println!(
                    "    `{}` must print `cargo-crap {pin}`, the {TOOL_LOCK} pin",
                    CRAP_VERSION.join(" ")
                );
            }
            listed += 1;
            if !dry_run {
                runs.push(match pin {
                    Some(pin) => run_pinned_crap(dir, &layer.id, &chain, pin)?,
                    None => run_chain(dir, &layer.id, &chain)?,
                });
            }
        }
    }

    let total = fs::read_to_string(dir.join(laws::PACK))
        .map(|pack| facts::pack_law_count(&pack))
        .unwrap_or_else(|_| facts::pack_law_count(profiles::LAWS_BEND));

    if dry_run {
        if laws_on {
            println!(
                "\n==> [laws] sh {} (only if Bend from {} is not installed)",
                laws::FETCH,
                laws::LOCK
            );
            println!("\n==> [laws] bend {}/cases.bend", laws::report_display(dir));
        }
        println!();
        println!("laws evaluated: 0/{total} (dry run: bend did not run)");
        // Nothing ran, so the count is short for every layer: a dry run
        // lists the commands and is never a pass.
        let (count, _) = command_count(0, &enabled, &[], false, max_tier, &shared);
        println!("{count}");
        if not_run.is_empty() {
            println!("dry-run: {listed} command(s) listed, 0 run; a dry run is not a pass");
        } else {
            println!(
                "dry-run: {listed} command(s) listed, 0 run; {} layer(s) NotRun, so the gate would fail",
                not_run.len()
            );
        }
        for n in not_run.iter().chain([&count]) {
            println!("  - {n}");
        }
        return Ok(ExitCode::from(1));
    }

    for &(layer, from) in &shared {
        let run = tests.take(layer, from, &runs);
        println!("[{layer}] took the {from} run's outcome: {}", run.outcome);
        runs.push(run);
    }
    let mut failed: Vec<String> = not_run;
    failed.extend(runs.iter().filter_map(|run| {
        match shared.iter().find(|(layer, _)| *layer == run.layer) {
            Some((_, from)) => run.outcome.fails_the_gate().then(|| {
                format!(
                    "{}: no command of its own; the {from} run it took is {}",
                    run.layer, run.outcome
                )
            }),
            None => run_failure(run),
        }
    }));
    let mut bend_ran = false;
    let evaluated = match judge {
        Some(judge) => {
            // A report from an earlier gate must not be counted for this one.
            let cases = report.join("cases.json");
            match fs::remove_file(&cases) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                    return Err(err).with_context(|| format!("remove {}", cases.display()));
                }
                _ => {}
            }
            let ctx = facts::Ctx {
                dir,
                cfg,
                yaml: fs::read_to_string(config_path(dir))
                    .ok()
                    .and_then(|raw| facts::parse_yaml(&raw).ok())
                    .unwrap_or_default(),
                max_tier,
                runs: &runs,
                artifacts: &report,
                started,
                workflows: facts::load_workflows(dir),
            };
            failed.extend(laws::run(&judge, &ctx)?);
            // `cases.json` was removed above, so one here means bend ran.
            let report = laws_evaluated(&cases);
            bend_ran = report.is_some();
            let (checked, of) = report.unwrap_or((0, total));
            format!("laws evaluated: {checked}/{of}")
        }
        None => format!("laws evaluated: 0/{total} (laws layer off)"),
    };

    // A layer that took another's run ran no command of its own.
    let ran = runs.len() - shared.len();
    let vacuous = runs
        .iter()
        .filter(|run| run.outcome == Outcome::Vacuous)
        .count();
    let (count, short) = command_count(
        ran_commands(&runs, bend_ran, &shared),
        &enabled,
        &runs,
        bend_ran,
        max_tier,
        &shared,
    );
    if short {
        failed.push(count.clone());
    }
    println!();
    println!("{evaluated}");
    println!("{count}");
    if failed.is_empty() {
        if vacuous == 0 {
            println!("ok: {ran} command(s) passed");
        } else {
            println!(
                "ok: {} command(s) passed, {vacuous} vacuous (counted for no law)",
                ran - vacuous
            );
        }
        Ok(ExitCode::SUCCESS)
    } else {
        println!("fail: {} problem(s), {ran} command(s) run", failed.len());
        for f in &failed {
            println!("  - {f}");
        }
        Ok(ExitCode::from(1))
    }
}

/// Commands that ran: each run whose tool was found, and `bend` once when it
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "already in this crate when the strict table was turned on"
)]
/// judged the laws layer. A layer that took another layer's run ran none.
fn ran_commands(runs: &[Run], bend_ran: bool, shared: &[(&str, &str)]) -> usize {
    runs.iter()
        .filter(|run| run.outcome != Outcome::ToolMissing)
        .filter(|run| !shared.iter().any(|(layer, _)| *layer == run.layer))
        .count()
        + usize::from(bend_ran)
}

/// The test commands of the enabled layers in range, which decide whether a
/// layer runs its own or takes another layer's run ([`credit`]). A layer
/// that is off or above the range has no command here, so nothing takes its
/// run.
struct TestCommands {
    coverage_in_range: bool,
    coverage: Vec<String>,
    unit: Vec<String>,
    integration: Vec<String>,
}

impl TestCommands {
    fn new(enabled: &[&config::Layer], report: &Path) -> Self {
        let argv = |id: &str| {
            enabled
                .iter()
                .find(|layer| layer.id == id)
                .and_then(|layer| plan_for(layer, report))
                .and_then(|plan| match plan {
                    Plan::Chains(chains) => chains
                        .into_iter()
                        .flatten()
                        .find(|step| step.kind == Kind::Tool),
                    Plan::Mutation(_) => None,
                })
                .map(|step| step.argv)
                .unwrap_or_default()
        };
        Self {
            coverage_in_range: enabled.iter().any(|layer| layer.id == "coverage"),
            coverage: argv("coverage"),
            unit: argv("unit"),
            integration: argv("integration"),
        }
    }

    fn query<'a>(&'a self, layer: &'a str, runs: &[Run]) -> credit::Query<'a> {
        let outcome = |id: &str| {
            runs.iter()
                .find(|run| run.layer == id)
                .map(|run| match run.outcome {
                    Outcome::Passed => credit::Outcome::Passed,
                    Outcome::ToolMissing => credit::Outcome::ToolMissing,
                    // A test run is never Vacuous; if one were, it is no pass.
                    Outcome::Failed | Outcome::Vacuous => credit::Outcome::Failed,
                })
        };
        credit::Query {
            layer,
            coverage_in_range: self.coverage_in_range,
            coverage_argv: &self.coverage,
            coverage_outcome: outcome("coverage"),
            unit_argv: &self.unit,
            integration_argv: &self.integration,
            unit_outcome: outcome("unit"),
        }
    }

    /// The layer whose run `layer` takes, or `None` when it runs its own.
    /// Credit from coverage is only possible in range, and from `unit` only
    /// out of it, so the range names the source.
    fn shared_from(&self, layer: &str) -> Option<&'static str> {
        if credit::runs_itself(&self.query(layer, &[])) {
            None
        } else if self.coverage_in_range {
            Some("coverage")
        } else {
            Some("unit")
        }
    }

    /// The run `layer` records once `from` has run: `from`'s command, and the
    /// outcome [`credit::recorded_outcome`] gives. A source that did not run
    /// is not a pass.
    fn take(&self, layer: &str, from: &str, runs: &[Run]) -> Run {
        let outcome = match credit::recorded_outcome(&self.query(layer, runs)) {
            Some(credit::Outcome::Passed) => Outcome::Passed,
            Some(credit::Outcome::ToolMissing) => Outcome::ToolMissing,
            Some(credit::Outcome::Failed) | None => Outcome::Failed,
        };
        let source = runs.iter().find(|run| run.layer == from);
        Run {
            layer: layer.to_string(),
            cmd: source.map(|run| run.cmd.clone()).unwrap_or_default(),
            step: source.map(|run| run.step.clone()).unwrap_or_default(),
            outcome,
        }
    }
}

/// The `count:` line, and whether it is short: an enabled layer in range
/// ran no command (NotRun, a missing tool, or a dry run). Every run belongs
/// to one enabled layer, so fewer commands than layers is always short. A
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "already in this crate when the strict table was turned on"
)]
/// short count fails the gate.
fn command_count(
    ran: usize,
    enabled: &[&config::Layer],
    runs: &[Run],
    bend_ran: bool,
    max_tier: u8,
    shared: &[(&str, &str)],
) -> (String, bool) {
    let none: Vec<&str> = enabled
        .iter()
        .map(|layer| layer.id.as_str())
        .filter(|id| match *id {
            "laws" => !bend_ran,
            _ => !runs
                .iter()
                .any(|run| run.layer == *id && run.outcome != Outcome::ToolMissing),
        })
        .collect();
    let mut line = format!(
        "count: {ran} command(s) ran; {} of {} enabled layer(s) at tier <= {max_tier} ran one",
        enabled.len() - none.len(),
        enabled.len()
    );
    // A layer that took another's run is counted when that run happened,
    // with the run named.
    for from in ["coverage", "unit"] {
        let took: Vec<&str> = shared
            .iter()
            .filter(|(_, source)| *source == from)
            .map(|(layer, _)| *layer)
            .collect();
        if !took.is_empty() {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "already in this crate when the strict table was turned on"
            )]
            let _ = write!(line, "; {} took the {from} run", took.join(", "));
        }
    }
    if none.is_empty() {
        (line, false)
    } else {
        (format!("{line}; none ran for: {}", none.join(", ")), true)
    }
}

/// `(checked, total)` from the `cases.json` the laws layer just wrote. A law
/// is evaluated when `bend` got a case for it.
fn laws_evaluated(cases: &Path) -> Option<(usize, usize)> {
    let raw = fs::read_to_string(cases).ok()?;
    let report: serde_json::Value = serde_json::from_str(&raw).ok()?;
    #[expect(
        clippy::indexing_slicing,
        reason = "already in this crate when the strict table was turned on"
    )]
    let laws = report["laws"].as_array()?;
    let checked = laws.iter().filter(|law| law["status"] == "checked").count();
    Some((checked, laws.len()))
}

fn run_failure(run: &Run) -> Option<String> {
    match run.outcome {
        Outcome::Passed | Outcome::Vacuous => None,
        Outcome::Failed => Some(format!("{}: `{}` failed", run.layer, run.step)),
        Outcome::ToolMissing => {
            let missing = steps::MissingTool {
                binary: steps::tool_binary(std::slice::from_ref(&run.step))
                    .unwrap_or_else(|| run.step.clone()),
            };
            Some(format!("{}: {missing}", run.layer))
        }
    }
}

/// The steps that ran, joined, and the outcome of the last one.
fn finish(layer: &str, ran: &[String], outcome: Outcome) -> Run {
    Run {
        layer: layer.to_string(),
        cmd: ran.join(" && "),
        step: ran.last().cloned().unwrap_or_default(),
        outcome,
    }
}

fn run_chain(dir: &Path, layer: &str, chain: &[Step]) -> Result<Run> {
    let mut ran = Vec::new();
    let mut outcome = Outcome::Passed;
    for step in chain {
        ran.push(step.display());
        outcome = run_step(dir, step)?;
        if !steps::chain_continues(outcome) {
            break;
        }
    }
    Ok(finish(layer, &ran, outcome))
}

/// The tool pins a repo keeps at its root, beside `rust-tops.yaml`. `init`
/// does not write one. Adopters copy `config/rust-tops.lock.example`.
pub(crate) const TOOL_LOCK: &str = "rust-tops.lock";

/// The command whose first line must name the pinned cargo-crap.
const CRAP_VERSION: [&str; 3] = ["cargo", "crap", "--version"];

/// The `[tools]` version that `dir/rust-tops.lock` pins for `tool`. No lock,
/// or no line for `tool`, is `None`. A lock that does not parse is an error.
fn tool_pin(dir: &Path, tool: &str) -> Result<Option<String>> {
    let path = dir.join(TOOL_LOCK);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("read {}", path.display())),
    };
    let lock: toml::Table =
        toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    Ok(lock
        .get("tools")
        .and_then(|tools| tools.get(tool))
        .and_then(toml::Value::as_str)
        .map(str::to_string))
}

/// The crap chain, run only when `cargo crap --version` prints
/// `cargo-crap <pin>`. It asks the binary that `cargo crap` runs, so another
/// version earlier on `PATH` fails the layer before a report is written. A
/// cargo-crap that is not there is left to the chain, which reports it
/// missing.
fn run_pinned_crap(dir: &Path, layer: &str, chain: &[Step], pin: &str) -> Result<Run> {
    let version = CRAP_VERSION.join(" ");
    let argv = CRAP_VERSION.map(String::from);
    if steps::check_tool(&argv).is_err() {
        return run_chain(dir, layer, chain);
    }
    let output = Command::new(CRAP_VERSION[0])
        .args(&CRAP_VERSION[1..])
        .current_dir(dir)
        .output()
        .with_context(|| format!("spawn: {version}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let printed = stdout.lines().next().unwrap_or_default().trim();
    let want = format!("cargo-crap {pin}");
    if !output.status.success() || printed != want {
        println!(
            "Failed [{layer}]: `{version}` printed `{printed}`, not `{want}` from {TOOL_LOCK}"
        );
        return Ok(finish(layer, &[version], Outcome::Failed));
    }
    let mut run = run_chain(dir, layer, chain)?;
    run.cmd = format!("{version} && {}", run.cmd);
    Ok(run)
}

/// `cargo-mutants` is looked up before the diff, so an empty diff cannot
/// hide a missing tool. A failed diff is `Failed`; nothing waives it. An
/// empty one is `Vacuous` and `cargo mutants` does not run.
fn run_mutation(dir: &Path, layer: &str, plan: &MutationPlan) -> Result<Run> {
    if let Err(missing) = steps::check_tool(&plan.mutants.argv) {
        println!("{missing}");
        return Ok(finish(
            layer,
            &[plan.mutants.display()],
            Outcome::ToolMissing,
        ));
    }
    let mut ran = vec![plan.diff.display()];
    let diff_ok = run_step(dir, &plan.diff)? == Outcome::Passed;
    let diff = steps::judge_diff(diff_ok, &plan.diff_path);
    let Some(mutants) = plan.after_diff(diff) else {
        if diff == Outcome::Vacuous && has_origin_main(dir) {
            println!(
                "Vacuous: the diff from `git merge-base origin/main HEAD` is empty; \
                 cargo mutants did not run and no law counts this layer"
            );
        } else if diff == Outcome::Vacuous {
            println!(
                "Vacuous: no origin/main; the mutation diff is empty; \
                 cargo mutants did not run and no law counts this layer"
            );
        } else {
            println!("Failed: the mutation diff did not succeed; a failed diff is never waived");
        }
        return Ok(finish(layer, &ran, diff));
    };
    ran.push(mutants.display());
    let outcome = run_step(dir, mutants)?;
    Ok(finish(layer, &ran, outcome))
}

/// Whether `refs/remotes/origin/main` exists, for the Vacuous wording only.
fn has_origin_main(dir: &Path) -> bool {
    Command::new("git")
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            "refs/remotes/origin/main",
        ])
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn run_step(dir: &Path, step: &Step) -> Result<Outcome> {
    match step.kind {
        Kind::Artifact => {
            return Ok(step.artifact_path().map_or(Outcome::Failed, |path| {
                steps::artifact_outcome(&dir.join(path))
            }));
        }
        Kind::Tool => {
            if let Err(missing) = steps::check_tool(&step.argv) {
                println!("{missing}");
                return Ok(Outcome::ToolMissing);
            }
        }
        Kind::Command => {}
    }
    let Some((program, args)) = step.program() else {
        return Ok(Outcome::Failed);
    };
    match Command::new(program).args(args).current_dir(dir).status() {
        Ok(status) if status.success() => Ok(Outcome::Passed),
        Ok(_) => Ok(Outcome::Failed),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "{}",
                steps::MissingTool {
                    binary: program.to_string()
                }
            );
            Ok(Outcome::ToolMissing)
        }
        Err(err) => Err(err).with_context(|| format!("spawn: {}", step.display())),
    }
}

fn tool(argv: &[&str]) -> Vec<Step> {
    vec![Step::tool(argv.iter().copied())]
}

/// `report` is [`laws::artifact_dir`]: the coverage LCOV, the CRAP report
/// and the mutation diff are written there.
#[expect(clippy::too_many_lines, reason = "over the kit line threshold")]
fn plan_for(layer: &config::Layer, report: &Path) -> Option<Plan> {
    let chains = match layer.id.as_str() {
        "static-lint" => vec![
            tool(&["cargo", "fmt", "--all", "--", "--check"]),
            tool(&[
                "cargo",
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ]),
            tool(&["cargo", "deny", "check"]),
        ],
        "unit" | "integration" => vec![tool(&[
            "cargo",
            "nextest",
            "run",
            "--workspace",
            "--all-features",
            "--profile",
            "ci",
        ])],
        "doc" => vec![tool(&[
            "cargo",
            "test",
            "--doc",
            "--workspace",
            "--all-features",
        ])],
        "coverage" => vec![steps::coverage_steps(report)],
        "crap" => {
            let lcov = report.join(steps::LCOV).display().to_string();
            let crap = report.join(steps::CRAP).display().to_string();
            vec![vec![
                Step::tool([
                    "cargo",
                    "crap",
                    "--lcov",
                    lcov.as_str(),
                    "--format",
                    "json",
                    "--output",
                    crap.as_str(),
                ]),
                Step::artifact(crap),
            ]]
        }
        "mutation" => return Some(Plan::Mutation(MutationPlan::new(report))),
        "property" => vec![tool(&[
            "cargo",
            "nextest",
            "run",
            "--workspace",
            "--all-features",
            "--profile",
            "ci",
            "--no-tests=fail",
            "property",
        ])],
        "security-advisory" => vec![tool(&["cargo", "deny", "check", "advisories"])],
        "formal-kani" => vec![tool(&["cargo", "kani", "--workspace"])],
        _ => return None,
    };
    Some(Plan::Chains(chains))
}

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

    fn layer(id: &str) -> config::Layer {
        serde_saphyr::from_str(&format!(
            "{{ id: {id}, enabled: true, tier: 0, runner: x }}"
        ))
        .expect("layer")
    }

    fn chains(id: &str) -> Vec<Vec<Step>> {
        match plan_for(&layer(id), Path::new("/r")) {
            Some(Plan::Chains(chains)) => chains,
            _ => panic!("{id} has no chains"),
        }
    }

    fn tool_step(chain: &[Step]) -> &Step {
        chain
            .iter()
            .find(|step| step.kind == Kind::Tool)
            .expect("a tool step")
    }

    #[test]
    fn the_g0_strings_name_the_tool_they_run() {
        let coverage = "mkdir -p target/coverage && cargo llvm-cov nextest --workspace --all-features --profile ci --lcov --output-path target/coverage/lcov.info && test -s target/coverage/lcov.info";
        let mutation = "sh -c 'git diff origin/main...HEAD --unified=0 > /tmp/rust-tops.diff; cargo mutants --in-diff /tmp/rust-tops.diff -j 2'";
        assert_eq!(
            steps::tool_binary(&[coverage.to_string()]).as_deref(),
            Some("cargo-llvm-cov")
        );
        assert_eq!(
            steps::tool_binary(&[mutation.to_string()]).as_deref(),
            Some("cargo-mutants")
        );
    }

    #[test]
    fn coverage_and_crap_check_their_own_binary() {
        let coverage = chains("coverage");
        assert_eq!(coverage.len(), 1);
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_1285_44 = coverage[0].clone();
        assert_eq!(
            steps::tool_binary(&tool_step(&tops_idx_1285_44).argv).as_deref(),
            Some("cargo-llvm-cov")
        );
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_1293_13 = coverage[0].clone();
        assert_eq!(
            tops_idx_1293_13.last().map(|step| step.kind),
            Some(Kind::Artifact)
        );
        let crap = chains("crap");
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_1302_44 = crap[0].clone();
        assert_eq!(
            steps::tool_binary(&tool_step(&tops_idx_1302_44).argv).as_deref(),
            Some("cargo-crap")
        );
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_1310_13 = crap[0].clone();
        assert_eq!(
            tops_idx_1310_13.last().and_then(Step::artifact_path),
            Some(Path::new("/r/crap.json"))
        );
    }

    #[test]
    fn coverage_and_crap_write_under_the_artifact_dir() {
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let coverage: Vec<String> = chains("coverage")[0].iter().map(Step::display).collect();
        assert_eq!(
            coverage,
            [
                "mkdir -p /r",
                "cargo llvm-cov nextest --workspace --all-features --profile ci --lcov --output-path /r/lcov.info",
                "test -s /r/lcov.info",
            ]
        );
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let crap: Vec<String> = chains("crap")[0].iter().map(Step::display).collect();
        assert_eq!(
            crap,
            [
                "cargo crap --lcov /r/lcov.info --format json --output /r/crap.json",
                "test -s /r/crap.json",
            ]
        );
    }

    fn argv(words: &str) -> Vec<String> {
        words.split_whitespace().map(str::to_string).collect()
    }

    const COVERAGE: &str = "cargo llvm-cov nextest --workspace --all-features --profile ci --lcov";
    const UNIT: &str = "cargo nextest run --workspace --all-features --profile ci";

    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "already in this crate when the strict table was turned on"
    )]
    fn tests_with(in_range: bool, coverage: &str, integration: &str) -> TestCommands {
        TestCommands {
            coverage_in_range: in_range,
            coverage: argv(coverage),
            unit: argv(UNIT),
            integration: argv(integration),
        }
    }

    fn run(layer: &str, outcome: Outcome) -> Run {
        Run {
            layer: layer.to_string(),
            cmd: format!("{layer} cmd"),
            step: format!("{layer} step"),
            outcome,
        }
    }

    #[test]
    fn the_shipped_test_commands_share_one_coverage_run() {
        let layers: Vec<config::Layer> = ["unit", "integration", "coverage", "property"]
            .into_iter()
            .map(layer)
            .collect();
        let enabled: Vec<&config::Layer> = layers.iter().collect();
        let tests = TestCommands::new(&enabled, Path::new("/r"));
        assert!(tests.coverage_in_range);
        assert!(
            credit::coverage_grants_credit(&tests.coverage),
            "{:?}",
            tests.coverage
        );
        assert_eq!(tests.unit, tests.integration);
        for id in ["unit", "integration", "property"] {
            assert_eq!(tests.shared_from(id), Some("coverage"), "{id}");
        }
        for id in ["coverage", "doc", "static-lint", "crap"] {
            assert_eq!(tests.shared_from(id), None, "{id}");
        }
        // Out of range, nothing names a coverage command and none is shared.
        let unit_only: Vec<&config::Layer> = enabled
            .iter()
            .copied()
            .filter(|l| l.id != "coverage")
            .collect();
        let tests = TestCommands::new(&unit_only, Path::new("/r"));
        assert!(!tests.coverage_in_range);
        assert!(tests.coverage.is_empty());
        assert_eq!(tests.shared_from("integration"), Some("unit"));
        assert_eq!(tests.shared_from("unit"), None);
        assert_eq!(tests.shared_from("property"), None);
    }

    #[test]
    fn a_coverage_command_missing_a_flag_shares_nothing() {
        for missing in ["--workspace", "--all-features", "--profile ci"] {
            let coverage = COVERAGE.replace(&format!(" {missing}"), "");
            let tests = tests_with(true, &coverage, UNIT);
            for id in ["unit", "integration", "property"] {
                assert_eq!(tests.shared_from(id), None, "{missing}: {id}");
            }
        }
        let tests = tests_with(
            true,
            &COVERAGE.replace("--profile ci", "--profile default"),
            UNIT,
        );
        assert_eq!(tests.shared_from("unit"), None);
    }

    #[test]
    fn integration_takes_unit_only_when_its_command_is_unit_s() {
        let tests = tests_with(false, COVERAGE, UNIT);
        assert_eq!(tests.shared_from("integration"), Some("unit"));
        let tests = tests_with(false, COVERAGE, &format!("{UNIT} --test it"));
        assert_eq!(tests.shared_from("integration"), None);
        // Unit off or out of range: no unit command, so integration runs.
        let mut tests = tests_with(false, COVERAGE, UNIT);
        tests.unit.clear();
        assert_eq!(tests.shared_from("integration"), None);
    }

    #[test]
    fn a_shared_layer_records_its_source_run_and_no_source_is_no_pass() {
        let tests = tests_with(true, COVERAGE, UNIT);
        for outcome in [Outcome::Passed, Outcome::Failed, Outcome::ToolMissing] {
            let took = tests.take("unit", "coverage", &[run("coverage", outcome)]);
            assert_eq!(took.outcome, outcome);
            assert_eq!(took.layer, "unit");
            assert_eq!(took.cmd, "coverage cmd");
            assert_eq!(took.step, "coverage step");
        }
        let took = tests.take("unit", "coverage", &[run("coverage", Outcome::Vacuous)]);
        assert_eq!(took.outcome, Outcome::Failed);
        let took = tests.take("property", "coverage", &[run("unit", Outcome::Passed)]);
        assert_eq!(took.outcome, Outcome::Failed);
        assert_eq!(took.cmd, "");
        let tests = tests_with(false, COVERAGE, UNIT);
        let took = tests.take("integration", "unit", &[run("unit", Outcome::Failed)]);
        assert_eq!(took.outcome, Outcome::Failed);
        assert_eq!(took.cmd, "unit cmd");
    }

    #[test]
    fn a_shared_run_is_not_a_command_and_its_source_is_named() {
        let runs = [
            run("coverage", Outcome::Passed),
            run("unit", Outcome::Passed),
            run("doc", Outcome::ToolMissing),
        ];
        assert_eq!(ran_commands(&runs, false, &[("unit", "coverage")]), 1);
        assert_eq!(ran_commands(&runs, true, &[]), 3);
        let layers: Vec<config::Layer> =
            ["coverage", "unit", "doc"].into_iter().map(layer).collect();
        let enabled: Vec<&config::Layer> = layers.iter().collect();
        let (line, short) = command_count(1, &enabled, &runs, false, 1, &[("unit", "coverage")]);
        assert!(short);
        assert_eq!(
            line,
            "count: 1 command(s) ran; 2 of 3 enabled layer(s) at tier <= 1 ran one; unit took the coverage run; none ran for: doc"
        );
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let (line, short) =
            command_count(2, &enabled[..2], &runs, false, 1, &[("unit", "coverage")]);
        assert!(!short);
        assert!(
            line.ends_with("ran one; unit took the coverage run"),
            "{line}"
        );
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let (line, _) = command_count(2, &enabled[..2], &runs, false, 1, &[("unit", "unit")]);
        assert!(line.ends_with("ran one; unit took the unit run"), "{line}");
    }

    #[test]
    fn the_mutation_diff_uses_merge_base_and_the_report_dir() {
        let report = Path::new("/work/target/rust-tops");
        let Some(Plan::Mutation(plan)) = plan_for(&layer("mutation"), report) else {
            panic!("mutation has no plan");
        };
        assert_eq!(plan.diff_path, report.join("rust-tops.diff"));
        let script = plan.diff.argv.join(" ");
        assert!(
            script.contains("git merge-base origin/main HEAD"),
            "{script}"
        );
        assert!(script.contains("refs/remotes/origin/main"), "{script}");
        assert!(script.contains("&& git diff"), "{script}");
        assert!(!script.contains("mutants"), "{script}");
        assert!(!script.contains("/tmp"), "{script}");
        assert!(plan
            .mutants
            .argv
            .contains(&"/work/target/rust-tops/rust-tops.diff".to_string()));
        assert_eq!(
            steps::tool_binary(&plan.mutants.argv).as_deref(),
            Some("cargo-mutants")
        );
    }

    #[test]
    fn no_mapped_layer_writes_under_tmp() {
        for id in [
            "static-lint",
            "unit",
            "integration",
            "doc",
            "coverage",
            "crap",
            "property",
            "security-advisory",
        ] {
            for chain in chains(id) {
                for step in chain {
                    assert!(!step.display().contains("/tmp"), "{id}: {}", step.display());
                }
            }
        }
    }

    #[test]
    fn unmapped_layers_have_no_plan() {
        for id in [
            "acceptance",
            "fuzz",
            "miri",
            "sanitizer",
            "bench-criterion",
            "bench-callgrind",
            "density-audit",
            "laws",
        ] {
            assert!(plan_for(&layer(id), Path::new("/r")).is_none(), "{id}");
        }
    }

    #[test]
    fn formal_kani_runs_cargo_kani() {
        let kani = chains("formal-kani");
        assert_eq!(kani.len(), 1);
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let step = tool_step(&kani[0]);
        assert_eq!(step.argv, ["cargo", "kani", "--workspace"]);
        assert_eq!(
            steps::tool_binary(&step.argv).as_deref(),
            Some("cargo-kani")
        );
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
    fn a_failed_diff_is_failed_even_over_a_stale_diff() {
        let dir = std::env::temp_dir().join(format!("cargo-tops-diff-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("mkdir");
        let stale = dir.join("stale.diff");
        fs::write(&stale, "+changed\n").expect("write");
        let empty = dir.join("empty.diff");
        fs::write(&empty, "").expect("write");
        assert_eq!(steps::judge_diff(false, &stale), Outcome::Failed);
        assert_eq!(steps::judge_diff(false, &empty), Outcome::Failed);
        assert_eq!(steps::judge_diff(true, &empty), Outcome::Vacuous);
        assert_eq!(steps::judge_diff(true, &stale), Outcome::Passed);
        assert_eq!(
            steps::judge_diff(true, &dir.join("absent")),
            Outcome::Failed
        );
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&dir);
        assert!(!Outcome::Vacuous.fails_the_gate());
        assert!(!Outcome::Vacuous.counts_for_a_law());
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
