use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::SystemTime;

mod config;
mod facts;
mod laws;
mod profiles;

#[cfg(test)]
mod parse_oracle;

use config::TopsConfig;
use facts::{Outcome, Run};

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
        #[arg(long, value_enum, default_value_t = CrateClass::LibraryCore)]
        class: CrateClass,
        /// Directory to initialize (default: cwd)
        #[arg(long, default_value = ".")]
        path: PathBuf,
        /// Crate name written into rust-tops.yaml (default: directory name)
        #[arg(long)]
        name: Option<String>,
        /// Overwrite existing protocol files
        #[arg(long)]
        force: bool,
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

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CrateClass {
    LibraryCore,
    ParserCodec,
    NoStdEmbedded,
    UnsafeKernel,
    BinaryCli,
}

impl CrateClass {
    fn as_str(self) -> &'static str {
        match self {
            CrateClass::LibraryCore => "library-core",
            CrateClass::ParserCodec => "parser-codec",
            CrateClass::NoStdEmbedded => "no-std-embedded",
            CrateClass::UnsafeKernel => "unsafe-kernel",
            CrateClass::BinaryCli => "binary-cli",
        }
    }
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
    let cli = Cli::parse();
    match cli.command {
        Commands::Init {
            class,
            path,
            name,
            force,
        } => {
            init(&path, class, name.as_deref(), force)?;
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

fn load_config(path: &Path) -> Result<TopsConfig> {
    let file = config_path(path);
    let raw = fs::read_to_string(&file)
        .with_context(|| format!("missing {}; run `cargo tops init`", file.display()))?;
    // YAML 1.2 booleans: only true/false. Unquoted yes/no/on/off/y/n stay strings.
    let cfg: TopsConfig = serde_saphyr::from_str_with_options(
        &raw,
        serde_saphyr::options! {
            strict_booleans: true,
        },
    )
    .with_context(|| format!("invalid YAML: {}", file.display()))?;
    Ok(cfg)
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
    if cfg.gates.crap.threshold_agent < 1.0 {
        return Err(anyhow!("gates.crap.threshold_agent must be >= 1"));
    }
    if cfg.gates.mutation.unexplained_survivors_max != 0 && cfg.crate_.class == "library-core" {
        return Err(anyhow!(
            "library-core cannot raise unexplained_survivors_max above 0"
        ));
    }
    Ok(())
}

fn init(dir: &Path, class: CrateClass, name: Option<&str>, force: bool) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    fs::create_dir_all(dir.join(".config"))?;
    fs::create_dir_all(dir.join(".cargo"))?;
    fs::create_dir_all(dir.join(".github/workflows"))?;

    let crate_name = name
        .map(str::to_string)
        .or_else(|| {
            dir.canonicalize()
                .ok()
                .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        })
        .unwrap_or_else(|| "unnamed".to_string());

    let yaml = profiles::profile_yaml(class.as_str(), &crate_name);
    let msrv = facts::parse_yaml(&yaml)
        .ok()
        .and_then(|doc| doc["crate"]["msrv"].as_str().map(str::to_string))
        .ok_or_else(|| anyhow!("the {} profile has no crate.msrv", class.as_str()))?;
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
        &profiles::workflow_yml(class.as_str()),
        force,
    )?;
    write_file(dir.join(laws::PACK), profiles::LAWS_BEND, force)?;
    write_file(dir.join(laws::LOCK), profiles::BEND_LOCK, force)?;
    if write_file(dir.join(laws::FETCH), profiles::FETCH_BEND_SH, force)? {
        make_executable(&dir.join(laws::FETCH))?;
    }

    println!(
        "initialized rust-tops for {crate_name} ({}) in {}",
        class.as_str(),
        dir.display()
    );
    println!("next: cargo tops check && cargo tops gate");
    Ok(())
}

/// Returns whether the file was written (false when it existed and `force` was off).
fn write_file(path: PathBuf, contents: &str, force: bool) -> Result<bool> {
    if path.exists() && !force {
        println!("skip existing {}", path.display());
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, contents).with_context(|| format!("write {}", path.display()))?;
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

    let started = SystemTime::now();
    let mut runs: Vec<Run> = Vec::new();
    for layer in enabled.iter().filter(|l| l.id != "laws") {
        let cmds = commands_for(layer, cfg);
        if cmds.is_empty() {
            println!("skip {}: no local command mapped", layer.id);
            continue;
        }
        for cmd in cmds {
            println!("\n==> [{}] {cmd}", layer.id);
            let outcome = if dry_run {
                Outcome::Passed
            } else if !tool_available(&cmd) {
                Outcome::ToolMissing
            } else if run_shell(dir, &cmd)? {
                Outcome::Passed
            } else {
                Outcome::Failed
            };
            runs.push(Run {
                layer: layer.id.clone(),
                cmd,
                outcome,
            });
        }
    }
    let ran = runs.len();

    if dry_run {
        if laws_on {
            println!(
                "\n==> [laws] sh {} (only if Bend from {} is not installed)",
                laws::FETCH,
                laws::LOCK
            );
            println!("\n==> [laws] bend {}/cases.bend", laws::OUT_DIR);
        }
        println!();
        println!("dry-run: {ran} command(s) listed");
        return Ok(ExitCode::SUCCESS);
    }

    let mut failed: Vec<String> = runs.iter().filter_map(run_failure).collect();
    if let Some(judge) = judge {
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
    }

    println!();
    if failed.is_empty() {
        println!("ok: {ran} command(s) passed");
        Ok(ExitCode::SUCCESS)
    } else {
        println!("fail: {} problem(s), {ran} command(s) run", failed.len());
        for f in &failed {
            println!("  - {f}");
        }
        Ok(ExitCode::from(1))
    }
}

fn run_failure(run: &Run) -> Option<String> {
    match run.outcome {
        Outcome::Passed => None,
        Outcome::Failed => Some(format!("{}: `{}` failed", run.layer, run.cmd)),
        Outcome::ToolMissing => Some(format!(
            "{}: `{}` not on PATH (install it; cargo-tops does not vendor engines)",
            run.layer,
            first_word(&run.cmd)
        )),
    }
}

fn commands_for(layer: &config::Layer, _cfg: &TopsConfig) -> Vec<String> {
    match layer.id.as_str() {
        "static-lint" => vec![
            "cargo fmt --all -- --check".into(),
            "cargo clippy --workspace --all-targets --all-features -- -D warnings".into(),
            "cargo deny check".into(),
        ],
        "unit" | "integration" => vec!["cargo nextest run --workspace --all-features --profile ci".into()],
        "doc" => vec!["cargo test --doc --workspace --all-features".into()],
        "coverage" => vec![
            "mkdir -p target/coverage && cargo llvm-cov nextest --workspace --all-features --lcov --output-path target/coverage/lcov.info && test -s target/coverage/lcov.info".into(),
        ],
        "crap" => vec![
            "cargo crap --lcov target/coverage/lcov.info --format json --output target/crap.json && test -s target/crap.json".into(),
        ],
        "mutation" => vec![
            "sh -c 'git diff origin/main...HEAD --unified=0 > /tmp/rust-tops.diff; cargo mutants --in-diff /tmp/rust-tops.diff -j 2'".into(),
        ],
        "property" => vec!["cargo nextest run --workspace --all-features --profile ci property".into()],
        "security-advisory" => vec!["cargo deny check advisories".into()],
        _ => Vec::new(),
    }
}

fn first_word(cmd: &str) -> String {
    if cmd.starts_with("sh -c") {
        return "sh".into();
    }
    cmd.split_whitespace()
        .find(|w| *w != "cargo" && !w.starts_with('+'))
        .unwrap_or("cargo")
        .to_string()
}

fn tool_available(cmd: &str) -> bool {
    // cargo subcommands: cargo-foo must exist unless it is a built-in.
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return false;
    }
    if parts[0] == "sh" {
        return which("sh");
    }
    if parts[0] != "cargo" {
        return which(parts[0]);
    }
    if parts.len() < 2 {
        return which("cargo");
    }
    let sub = parts[1].trim_start_matches('+'); // cargo +nightly ...
    let sub = if sub.starts_with('+') || parts[1].starts_with('+') {
        parts.get(2).copied().unwrap_or("")
    } else {
        parts[1]
    };
    match sub {
        "fmt" | "clippy" | "test" | "build" | "check" | "bench" => which("cargo"),
        "nextest" => which("cargo-nextest"),
        "llvm-cov" => which("cargo-llvm-cov"),
        "crap" => which("cargo-crap") || which("cargo-crappy"),
        "mutants" => which("cargo-mutants"),
        "deny" => which("cargo-deny"),
        "fuzz" => which("cargo-fuzz"),
        "kani" => which("cargo-kani") || which("kani"),
        _ => which("cargo"),
    }
}

fn which(bin: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {bin} >/dev/null 2>&1")])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_shell(dir: &Path, cmd: &str) -> Result<bool> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .status()
        .with_context(|| format!("spawn: {cmd}"))?;
    Ok(status.success())
}
