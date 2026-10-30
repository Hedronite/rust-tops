use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::SystemTime;

mod config;
mod facts;
mod laws;
mod profile_data;
mod profiles;
mod schema;
mod skills;
mod steps;

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

fn init(dir: &Path, class: &str, name: Option<&str>, force: bool, skills: bool) -> Result<()> {
    let crate_name = name
        .map(str::to_string)
        .or_else(|| dir_name(dir))
        .unwrap_or_else(|| "unnamed".to_string());
    // An unknown class fails here, before any directory or file is written.
    let yaml = with_init_exemptions(&profiles::profile_yaml(class, &crate_name)?);
    let msrv = facts::parse_yaml(&yaml)
        .ok()
        .and_then(|doc| doc["crate"]["msrv"].as_str().map(str::to_string))
        .ok_or_else(|| anyhow!("the {class} profile has no crate.msrv"))?;

    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    fs::create_dir_all(dir.join(".config"))?;
    fs::create_dir_all(dir.join(".cargo"))?;
    fs::create_dir_all(dir.join(".github/workflows"))?;
    write_file(dir.join("rust-tops.yaml"), &yaml, force)?;
    write_file(
        dir.join("rust-toolchain.toml"),
        &profiles::rust_toolchain_toml(&msrv),
        force,
    )?;
    write_file(dir.join("AGENTS.md"), profiles::AGENTS_MD, force)?;
    write_file(dir.join("clippy.toml"), profiles::CLIPPY_TOML, force)?;
    write_file(
        dir.join(".config/nextest.toml"),
        profiles::NEXTEST_TOML,
        force,
    )?;
    write_file(
        dir.join(".cargo/mutants.toml"),
        profiles::MUTANTS_TOML,
        force,
    )?;
    write_file(
        dir.join(".cargo-crap.toml"),
        profiles::CARGO_CRAP_TOML,
        force,
    )?;
    write_file(dir.join("deny.toml"), profiles::DENY_TOML, force)?;
    write_file(
        dir.join(".github/workflows/rust-tops.yml"),
        &profiles::workflow_yml(class),
        force,
    )?;
    write_file(dir.join(laws::PACK), profiles::LAWS_BEND, force)?;
    write_file(dir.join(laws::LOCK), profiles::BEND_LOCK, force)?;
    if write_file(dir.join(laws::FETCH), profiles::FETCH_BEND_SH, force)? {
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

/// Returns whether the file was written (false when it existed and `force` was off).
///
/// A symlink at `path` is refused, with or without `force`, and so is a
/// dangling one: the write never follows a link. `force` removes the old
/// file and writes a new one with `create_new`, which fails rather than
/// follow a link that appears in between.
pub(crate) fn write_file(path: PathBuf, contents: &str, force: bool) -> Result<bool> {
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(anyhow!(
                "refusing to write {}: it is a symlink, and init does not follow one",
                path.display()
            ));
        }
        Ok(_) if !force => {
            println!("skip existing {}", path.display());
            return Ok(false);
        }
        Ok(_) => fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?,
        // Nothing there, or nothing that can be read: `create_new` below
        // either makes the file or fails, and never follows a link.
        Err(_) => {}
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .with_context(|| format!("create {}", path.display()))?;
    std::io::Write::write_all(&mut file, contents.as_bytes())
        .with_context(|| format!("write {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(true)
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

fn gate(dir: &Path, cfg: &TopsConfig, max_tier: u8, dry_run: bool) -> Result<ExitCode> {
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

    // Steps run with `dir` as the working directory, so the diff path must
    // not be relative to the caller's.
    let report = std::path::absolute(laws::report_dir(dir))
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
    for layer in enabled.iter().filter(|l| l.id != "laws") {
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
        for chain in chains {
            let shown: Vec<String> = chain.iter().map(Step::display).collect();
            println!("\n==> [{}] {}", layer.id, shown.join(" && "));
            listed += 1;
            if !dry_run {
                runs.push(run_chain(dir, &layer.id, &chain)?);
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
        let (count, _) = command_count(0, &enabled, &[], false, max_tier);
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

    let mut failed: Vec<String> = not_run;
    failed.extend(runs.iter().filter_map(run_failure));
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

    let ran = runs.len();
    let vacuous = runs
        .iter()
        .filter(|run| run.outcome == Outcome::Vacuous)
        .count();
    let (count, short) = command_count(
        ran_commands(&runs, bend_ran),
        &enabled,
        &runs,
        bend_ran,
        max_tier,
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
/// judged the laws layer.
fn ran_commands(runs: &[Run], bend_ran: bool) -> usize {
    runs.iter()
        .filter(|run| run.outcome != Outcome::ToolMissing)
        .count()
        + usize::from(bend_ran)
}

/// The `count:` line, and whether it is short: an enabled layer in range
/// ran no command (NotRun, a missing tool, or a dry run). Every run belongs
/// to one enabled layer, so fewer commands than layers is always short. A
/// short count fails the gate.
fn command_count(
    ran: usize,
    enabled: &[&config::Layer],
    runs: &[Run],
    bend_ran: bool,
    max_tier: u8,
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
    let line = format!(
        "count: {ran} command(s) ran; {} of {} enabled layer(s) at tier <= {max_tier} ran one",
        enabled.len() - none.len(),
        enabled.len()
    );
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
        "coverage" => vec![steps::coverage_steps()],
        "crap" => vec![vec![
            Step::tool([
                "cargo",
                "crap",
                "--lcov",
                "target/coverage/lcov.info",
                "--format",
                "json",
                "--output",
                "target/crap.json",
            ]),
            Step::artifact("target/crap.json"),
        ]],
        "mutation" => return Some(Plan::Mutation(MutationPlan::new(report))),
        "property" => vec![tool(&[
            "cargo",
            "nextest",
            "run",
            "--workspace",
            "--all-features",
            "--profile",
            "ci",
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
fn with_init_exemptions(yaml: &str) -> String {
    let mut block = String::new();
    match steps::init_exemptions() {
        [] => block.push_str("exemptions: []\n"),
        entries => {
            block.push_str("exemptions:\n");
            for entry in entries {
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
        let coverage = "mkdir -p target/coverage && cargo llvm-cov nextest --workspace --all-features --lcov --output-path target/coverage/lcov.info && test -s target/coverage/lcov.info";
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
        assert_eq!(
            steps::tool_binary(&tool_step(&coverage[0]).argv).as_deref(),
            Some("cargo-llvm-cov")
        );
        assert_eq!(
            coverage[0].last().map(|step| step.kind),
            Some(Kind::Artifact)
        );
        let crap = chains("crap");
        assert_eq!(
            steps::tool_binary(&tool_step(&crap[0]).argv).as_deref(),
            Some("cargo-crap")
        );
        assert_eq!(
            crap[0].last().and_then(Step::artifact_path),
            Some(Path::new("target/crap.json"))
        );
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
                doc["layers"]
                    .as_array()
                    .and_then(|layers| layers.iter().find(|l| l["id"] == id))
                    .cloned()
                    .unwrap_or_else(|| panic!("{class}: no {id} layer"))
            };
            assert_eq!(layer("acceptance")["enabled"], false, "{class}");
            assert_eq!(layer("formal-kani")["tier"], 4, "{class}");
            assert_eq!(layer("laws")["enabled"], false, "{class}");
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
        let _ = fs::remove_dir_all(&dir);
        assert!(!Outcome::Vacuous.fails_the_gate());
        assert!(!Outcome::Vacuous.counts_for_a_law());
    }

    #[test]
    fn init_yaml_has_no_exemption_for_every_class() {
        for class in PROFILE_CLASSES {
            let yaml = init_yaml(class);
            let doc = facts::parse_yaml(&yaml).expect("init yaml parses");
            assert_eq!(doc["exemptions"], serde_json::json!([]), "{class}");
            assert!(!yaml.contains("generated protobuf"), "{class}");
            assert!(doc["layers"].as_array().is_some_and(|l| !l.is_empty()));
        }
    }

    #[test]
    fn the_exemptions_block_is_replaced_in_place() {
        let yaml = "a: 1\nexemptions:\n  - layer: crap\n    expires: \"2027-01-01\"\nb: 2\n";
        assert_eq!(with_init_exemptions(yaml), "a: 1\nexemptions: []\nb: 2\n");
        assert_eq!(with_init_exemptions("a: 1\n"), "a: 1\nexemptions: []\n");
    }
}
