//! Case selection for the `laws` layer (protocol 1.1.0, §3.1).
//!
//! Each [`LawFact`] names one law in `LAWS.bend`, its verdict function, and
//! the two constructors of its case type: `met` when the adopter shows the
//! fact the law asks for, `unmet` when it does not. `choose` reads files or
//! this gate run and returns that fact, or `NotEvaluated` when there is
//! nothing to read. Which constructor `LAWS.bend` refuses is `bend`'s answer;
//! nothing in this module knows it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use serde_json::Value;

use crate::config::TopsConfig;
use crate::profiles;

/// What happened to one command `gate` mapped for a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed,
    ToolMissing,
}

/// One command `gate` ran, or could not run, for a layer.
pub struct Run {
    pub layer: String,
    pub cmd: String,
    pub outcome: Outcome,
}

pub struct Workflow {
    pub rel: String,
    pub doc: Value,
}

/// `.github/workflows/*.yml` in the adopter repo.
pub enum Workflows {
    Absent,
    Parsed(Vec<Workflow>),
    Broken(String),
}

pub struct Ctx<'a> {
    pub dir: &'a Path,
    pub cfg: &'a TopsConfig,
    /// `rust-tops.yaml` as a tree, for keys `TopsConfig` does not type.
    pub yaml: Value,
    pub max_tier: u8,
    pub runs: &'a [Run],
    pub started: SystemTime,
    pub workflows: Workflows,
}

/// `cases.json` `reason_code` for a law this release cannot read.
pub const NO_FACT_READER: &str = "no_fact_reader";

/// `cases.json` `reason_code` for a law whose reader had nothing to read
/// on this run (a layer did not run, a file was absent, a tier had no match).
pub const RUN_DEPENDENT: &str = "run_dependent";

pub enum Choice {
    Fact { met: bool, read: Vec<String> },
    NotEvaluated { reason: String, code: &'static str },
}

pub struct LawFact {
    pub law: &'static str,
    pub fun: &'static str,
    pub met: &'static str,
    pub unmet: &'static str,
    pub choose: fn(&Ctx) -> Choice,
}

impl LawFact {
    pub fn ctor(&self, met: bool) -> &'static str {
        if met {
            self.met
        } else {
            self.unmet
        }
    }
}

/// Every law this slice can read a fact for, in `LAWS.bend` order.
pub const FACTS: &[LawFact] = &[
    LawFact {
        law: "ci_matrix_matches_class",
        fun: "class_job_verdict",
        met: "ClassJobPresent",
        unmet: "ClassJobMissing",
        choose: class_jobs,
    },
    LawFact {
        law: "fmt_is_tier0",
        fun: "fmt_verdict",
        met: "FmtTier0",
        unmet: "FmtUngated",
        choose: fmt_is_tier0,
    },
    LawFact {
        law: "lock_honesty",
        fun: "lock_verdict",
        met: "LockHonest",
        unmet: "LockDrift",
        choose: lock_honesty,
    },
    LawFact {
        law: "msrv_enforced",
        fun: "msrv_job_verdict",
        met: "MsrvJobPresent",
        unmet: "MsrvJobMissing",
        choose: msrv_enforced,
    },
    LawFact {
        law: "clippy_toolchain_pinned",
        fun: "clippy_pin_verdict",
        met: "ClippyPinned",
        unmet: "ClippyUnpinned",
        choose: clippy_toolchain_pinned,
    },
    LawFact {
        law: "no_pedantic_group",
        fun: "pedantic_verdict",
        met: "PerLint",
        unmet: "PedanticGroup",
        choose: no_pedantic_group,
    },
    LawFact {
        law: "baseline_green_before_mutants",
        fun: "baseline_verdict",
        met: "BaselineGreen",
        unmet: "BaselineSkip",
        choose: baseline_green,
    },
    LawFact {
        law: "record_only_artifacts_are_nonempty",
        fun: "record_verdict",
        met: "RecordNonempty",
        unmet: "RecordEmpty",
        choose: record_nonempty,
    },
    LawFact {
        law: "missing_tool_not_green",
        fun: "tool_verdict",
        met: "ToolPresent",
        unmet: "ToolMissing",
        choose: tool_present,
    },
    LawFact {
        law: "crap_wires_coverage",
        fun: "crap_wire_verdict",
        met: "CrapWired",
        unmet: "CrapUnwired",
        choose: crap_wires_coverage,
    },
    LawFact {
        law: "coverage_features_match_workspace",
        fun: "coverage_feat_verdict",
        met: "CoverageMatch",
        unmet: "CoverageNarrow",
        choose: coverage_features,
    },
    LawFact {
        law: "perf_without_runner_is_not_gate",
        fun: "perf_verdict",
        met: "PerfMeasured",
        unmet: "PerfNoRunner",
        choose: perf_runner,
    },
    LawFact {
        law: "artifact_check_is_its_own_step",
        fun: "artifact_step_verdict",
        met: "ArtifactOwnStep",
        unmet: "ArtifactFolded",
        choose: artifact_own_step,
    },
    LawFact {
        law: "drop_in_is_a_floor_not_a_ceiling",
        fun: "drop_in_verdict",
        met: "DropInFloor",
        unmet: "DropInCeiling",
        choose: drop_in_floor,
    },
    LawFact {
        law: "single_deny_site",
        fun: "deny_verdict",
        met: "DenySingle",
        unmet: "DenyDouble",
        choose: single_deny_site,
    },
];

pub fn find(law: &str) -> Option<&'static LawFact> {
    FACTS.iter().find(|fact| fact.law == law)
}

/// Why a law with no [`LawFact`] is `not-evaluated`.
pub fn not_evaluated(law: &str) -> &'static str {
    match law {
        "evidence_not_self_served" => {
            "intent is not readable: no file shows that a test was taken from the implementation"
        }
        "report_names_its_argv" => {
            "the only gate report is cargo-tops' own; a case chosen from it would grade the grader"
        }
        "mutant_kill_without_scar" => {
            "a scar is an edit to a production fn; reading it needs the diff and a Rust parser"
        }
        "every_cfg_is_seen_by_ci" | "every_cfg_target_has_a_lane" => {
            "cfg sites live in Rust source; no source scanner in this slice"
        }
        "job_declares_merge_semantics" => {
            "no written convention names blocks_merge, cadence, or die in a workflow"
        }
        "sysdeps_before_all_features" => {
            "which headers a build.rs probes is not in any file this gate reads"
        }
        "class_declared_before_edit"
        | "no_sealed_class_data_in_side_channels"
        | "measurement_is_isolated" => "no fact to read yet",
        "no_source_text_assertions"
        | "contract_lives_near_its_owner"
        | "no_write_only_state"
        | "pub_contract_doc_example"
        | "zeroize_confidential_buffers"
        | "constant_time_tag_cmp"
        | "library_public_oracle"
        | "fuzz_untrusted_ingress"
        | "no_unowned_hardening_todo"
        | "cap_is_named_or_it_is_a_bug" => "source law: needs a Rust parser (second slice)",
        _ => "no fact for this law in this release",
    }
}

fn fact(met: bool, read: Vec<String>) -> Choice {
    Choice::Fact { met, read }
}

fn not_evaluated_owned(reason: String) -> Choice {
    Choice::NotEvaluated {
        reason,
        code: RUN_DEPENDENT,
    }
}

fn not_evaluated_because(reason: &str) -> Choice {
    not_evaluated_owned(reason.to_string())
}

/// Run `body` on the parsed workflows, or say why there are none.
fn with_workflows(ctx: &Ctx, body: impl FnOnce(&[Workflow], Vec<String>) -> Choice) -> Choice {
    match &ctx.workflows {
        Workflows::Absent => not_evaluated_because("no workflow under .github/workflows"),
        Workflows::Broken(err) => not_evaluated_owned(format!("workflow did not parse: {err}")),
        Workflows::Parsed(wfs) => body(wfs, wfs.iter().map(|wf| wf.rel.clone()).collect()),
    }
}

pub fn parse_yaml(raw: &str) -> Result<Value, String> {
    serde_saphyr::from_str_with_options(
        raw,
        serde_saphyr::options! {
            strict_booleans: true,
        },
    )
    .map_err(|err| err.to_string())
}

pub fn load_workflows(dir: &Path) -> Workflows {
    let Ok(entries) = fs::read_dir(dir.join(".github/workflows")) else {
        return Workflows::Absent;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("yml" | "yaml")
            )
        })
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Workflows::Absent;
    }
    let mut parsed = Vec::with_capacity(paths.len());
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let rel = format!(".github/workflows/{name}");
        let doc = fs::read_to_string(&path)
            .map_err(|err| err.to_string())
            .and_then(|raw| parse_yaml(&raw));
        match doc {
            Ok(doc) => parsed.push(Workflow { rel, doc }),
            Err(err) => return Workflows::Broken(format!("{rel}: {err}")),
        }
    }
    Workflows::Parsed(parsed)
}

fn yaml_at<'v>(root: &'v Value, keys: &[&str]) -> Option<&'v Value> {
    keys.iter().try_fold(root, |node, key| node.get(key))
}

fn jobs(wf: &Workflow) -> impl Iterator<Item = (&str, &Value)> {
    wf.doc
        .get("jobs")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(id, job)| (id.as_str(), job))
}

fn steps(job: &Value) -> &[Value] {
    job.get("steps")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// Job id and `name:`, lowercased, for keyword matches.
fn job_labels(wfs: &[Workflow]) -> Vec<String> {
    wfs.iter()
        .flat_map(jobs)
        .map(|(id, job)| {
            let name = job.get("name").and_then(Value::as_str).unwrap_or("");
            format!("{id} {name}").to_lowercase()
        })
        .collect()
}

fn job_segments(job: &Value) -> Vec<Vec<String>> {
    steps(job)
        .iter()
        .filter_map(|step| step.get("run").and_then(Value::as_str))
        .flat_map(segments)
        .collect()
}

fn wf_segments(wfs: &[Workflow]) -> Vec<Vec<String>> {
    wfs.iter()
        .flat_map(jobs)
        .flat_map(|(_, job)| job_segments(job))
        .collect()
}

/// Shell commands in a `run:` block: comment lines dropped, backslash
/// continuations joined, then split on `&&`, `||`, `;` and `|`.
pub fn segments(run: &str) -> Vec<Vec<String>> {
    let mut logical = Vec::new();
    let mut current = String::new();
    for line in run.lines() {
        let trimmed = line.trim();
        if current.is_empty() && trimmed.starts_with('#') {
            continue;
        }
        match trimmed.strip_suffix('\\') {
            Some(head) => {
                current.push_str(head);
                current.push(' ');
            }
            None => {
                current.push_str(trimmed);
                logical.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        logical.push(current);
    }
    logical
        .iter()
        .flat_map(|line| {
            line.replace("&&", ";")
                .replace("||", ";")
                .split([';', '|'])
                .map(|part| {
                    part.split_whitespace()
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .filter(|tokens| !tokens.is_empty())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// `cargo [+toolchain] <sub>`, after any `VAR=value` prefixes.
fn cargo_sub(segment: &[String]) -> Option<&str> {
    let mut tokens = segment.iter().skip_while(|token| token.contains('='));
    if tokens.next()? != "cargo" {
        return None;
    }
    tokens
        .find(|token| !token.starts_with('+') && !token.starts_with('-'))
        .map(String::as_str)
}

fn has(segment: &[String], flag: &str) -> bool {
    segment.iter().any(|token| token == flag)
}

/// Tokens of a flags string or TOML array text: quotes, commas and brackets
/// are separators.
fn flag_tokens(raw: &str) -> Vec<String> {
    raw.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | '[' | ']'))
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

const ENABLE: &[&str] = &["-W", "--warn", "-D", "--deny", "-F", "--forbid"];
const DENY: &[&str] = &["-D", "--deny", "-F", "--forbid"];

/// `-D lint`, `-Dlint`, `--deny lint`, `--deny=lint`, for each level given.
fn sets_level(tokens: &[String], levels: &[&str], lint: &str) -> bool {
    tokens.iter().enumerate().any(|(i, token)| {
        levels.iter().any(|level| {
            let joined = if level.starts_with("--") {
                format!("{level}={lint}")
            } else {
                format!("{level}{lint}")
            };
            *token == joined
                || (token == level && tokens.get(i + 1).is_some_and(|next| next == lint))
        })
    })
}

fn read_text(dir: &Path, rel: &str, read: &mut Vec<String>) -> Option<String> {
    let raw = fs::read_to_string(dir.join(rel)).ok()?;
    read.push(rel.to_string());
    Some(raw)
}

fn read_toml(dir: &Path, rel: &str, read: &mut Vec<String>) -> Option<toml::Table> {
    read_text(dir, rel, read)?.parse::<toml::Table>().ok()
}

/// `[lints.<tool>]` or `[workspace.lints.<tool>]` sets one of `names` to one
/// of `levels`, as a string or as `{ level = ... }`.
fn manifest_lint(manifest: &toml::Table, tool: &str, names: &[&str], levels: &[&str]) -> bool {
    let tables = [
        manifest.get("lints"),
        manifest.get("workspace").and_then(|ws| ws.get("lints")),
    ];
    tables
        .iter()
        .flatten()
        .filter_map(|lints| lints.get(tool))
        .any(|lints| {
            names
                .iter()
                .filter_map(|name| lints.get(name))
                .any(|value| {
                    let level = value
                        .as_str()
                        .or_else(|| value.get("level").and_then(toml::Value::as_str));
                    level.is_some_and(|level| levels.contains(&level))
                })
        })
}

/// Every `rustflags` value in a Cargo config, as flag tokens.
fn rustflags_tokens(table: &toml::Table) -> Vec<String> {
    fn walk(value: &toml::Value, under_rustflags: bool, out: &mut Vec<String>) {
        match value {
            toml::Value::String(s) if under_rustflags => out.extend(flag_tokens(s)),
            toml::Value::Array(items) => items
                .iter()
                .for_each(|item| walk(item, under_rustflags, out)),
            toml::Value::Table(table) => {
                for (key, child) in table {
                    walk(child, under_rustflags || key == "rustflags", out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(&toml::Value::Table(table.clone()), false, &mut out);
    out
}

const CARGO_CONFIGS: [&str; 2] = [".cargo/config.toml", ".cargo/config"];

fn cargo_config_tokens(dir: &Path, read: &mut Vec<String>) -> Vec<String> {
    CARGO_CONFIGS
        .iter()
        .filter_map(|rel| read_toml(dir, rel, read))
        .flat_map(|table| rustflags_tokens(&table))
        .collect()
}

fn layer_ran(ctx: &Ctx, layer: &str) -> bool {
    ctx.runs
        .iter()
        .any(|run| run.layer == layer && run.outcome != Outcome::ToolMissing)
}

/// H-06. Jobs a class must show, from §4 ("required" in the fuzz, Miri,
/// Kani and Callgrind columns; loom for concurrency-runtime) on top of the
/// §17 jobs every PR runs.
pub fn class_keywords(class: &str) -> Option<&'static [&'static str]> {
    Some(match class {
        "library-core" | "library-util" | "binary-cli" | "binary-service" | "proc-macro"
        | "workspace-root" => &["clippy", "nextest", "coverage"],
        "parser-codec" => &["clippy", "nextest", "coverage", "fuzz"],
        "concurrency-runtime" => &["clippy", "nextest", "coverage", "miri", "loom"],
        "unsafe-kernel" => &["clippy", "nextest", "coverage", "miri", "kani", "callgrind"],
        "ffi-bridge" => &["clippy", "nextest", "coverage", "fuzz", "miri"],
        "no-std-embedded" => &["clippy", "nextest", "coverage", "miri"],
        _ => return None,
    })
}

fn class_jobs(ctx: &Ctx) -> Choice {
    let Some(keywords) = class_keywords(&ctx.cfg.crate_.class) else {
        return not_evaluated_owned(format!("class {} has no row in §4", ctx.cfg.crate_.class));
    };
    with_workflows(ctx, |wfs, mut read| {
        let labels = job_labels(wfs);
        let met = keywords
            .iter()
            .all(|word| labels.iter().any(|label| label.contains(word)));
        read.insert(0, "rust-tops.yaml".into());
        fact(met, read)
    })
}

/// H-22. `static-lint` is an enabled tier-0 layer with `fmt_check`, and every
/// workflow set, when there is one, runs `cargo fmt --check`.
fn fmt_is_tier0(ctx: &Ctx) -> Choice {
    let tier0 =
        ctx.cfg.layers.iter().any(|layer| {
            layer.id == "static-lint" && layer.enabled && layer.tier.unwrap_or(0) == 0
        });
    let fmt_check =
        yaml_at(&ctx.yaml, &["gates", "lint", "fmt_check"]).and_then(Value::as_bool) != Some(false);
    let mut read = vec!["rust-tops.yaml".to_string()];
    let in_ci = match &ctx.workflows {
        Workflows::Absent => true,
        Workflows::Broken(err) => {
            return not_evaluated_owned(format!("workflow did not parse: {err}"))
        }
        Workflows::Parsed(wfs) => {
            read.extend(wfs.iter().map(|wf| wf.rel.clone()));
            wf_segments(wfs)
                .iter()
                .any(|seg| cargo_sub(seg) == Some("fmt") && has(seg, "--check"))
        }
    };
    fact(tier0 && fmt_check && in_ci, read)
}

fn git(dir: &Path, args: &[&str]) -> Option<(i32, String)> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    Some((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
}

/// H-23. The nearest `Cargo.lock` is tracked and still equals `HEAD` after
/// this gate's Cargo commands, which rewrite a stale lock.
fn lock_honesty(ctx: &Ctx) -> Choice {
    let Some((0, top)) = git(ctx.dir, &["rev-parse", "--show-toplevel"]) else {
        return not_evaluated_because("not a git work tree");
    };
    let top = PathBuf::from(top.trim());
    let dir = fs::canonicalize(ctx.dir).unwrap_or_else(|_| ctx.dir.to_path_buf());
    let lock = dir
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(&top))
        .map(|ancestor| ancestor.join("Cargo.lock"))
        .find(|path| path.is_file());
    let Some(lock) = lock else {
        return fact(false, vec!["Cargo.lock (absent)".into()]);
    };
    let rel = lock
        .strip_prefix(&top)
        .unwrap_or(&lock)
        .to_string_lossy()
        .into_owned();
    let read = vec![rel.clone()];
    match git(&top, &["ls-files", "--error-unmatch", "--", &rel]) {
        Some((0, _)) => {}
        Some(_) => return fact(false, read),
        None => return not_evaluated_because("git did not run"),
    }
    match git(&top, &["diff", "--quiet", "HEAD", "--", &rel]) {
        Some((0, _)) => fact(true, read),
        Some((1, _)) => fact(false, read),
        _ => not_evaluated_because("git diff HEAD did not answer (no commit yet?)"),
    }
}

/// H-24. Some workflow job is an MSRV job.
fn msrv_enforced(ctx: &Ctx) -> Choice {
    with_workflows(ctx, |wfs, read| {
        fact(
            job_labels(wfs).iter().any(|label| label.contains("msrv")),
            read,
        )
    })
}

/// `1.85`, `1.85.0`, or a dated channel such as `nightly-2026-10-01`.
fn versioned(channel: &str) -> bool {
    let channel = channel.trim();
    if let Some(date) = ["stable-", "beta-", "nightly-"]
        .iter()
        .find_map(|p| channel.strip_prefix(p))
    {
        return date.len() == 10
            && date.bytes().enumerate().all(|(i, b)| {
                if i == 4 || i == 7 {
                    b == b'-'
                } else {
                    b.is_ascii_digit()
                }
            });
    }
    let parts: Vec<&str> = channel.split('.').collect();
    (2..=3).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn toolchain_channel(raw: &str) -> Option<String> {
    if let Ok(table) = raw.parse::<toml::Table>() {
        return table
            .get("toolchain")?
            .get("channel")?
            .as_str()
            .map(str::to_string);
    }
    raw.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
}

/// A toolchain step with a versioned `toolchain:` input or a versioned
/// `dtolnay/rust-toolchain@<ref>`.
fn job_pins_toolchain(job: &Value) -> bool {
    steps(job).iter().any(|step| {
        let input = yaml_at(step, &["with", "toolchain"]).and_then(Value::as_str);
        let reference = step
            .get("uses")
            .and_then(Value::as_str)
            .and_then(|uses| uses.strip_prefix("dtolnay/rust-toolchain@"));
        input.is_some_and(versioned) || reference.is_some_and(versioned)
    })
}

/// H-25. A rust-toolchain file names a version; without one, every workflow
/// job that runs clippy installs a versioned toolchain.
fn clippy_toolchain_pinned(ctx: &Ctx) -> Choice {
    for name in ["rust-toolchain.toml", "rust-toolchain"] {
        if let Ok(raw) = fs::read_to_string(ctx.dir.join(name)) {
            let pinned = toolchain_channel(&raw).as_deref().is_some_and(versioned);
            return fact(pinned, vec![name.to_string()]);
        }
    }
    let absent = "rust-toolchain.toml (absent)".to_string();
    match &ctx.workflows {
        Workflows::Absent => fact(false, vec![absent, ".github/workflows (absent)".into()]),
        Workflows::Broken(err) => not_evaluated_owned(format!("workflow did not parse: {err}")),
        Workflows::Parsed(wfs) => {
            let clippy_jobs: Vec<&Value> = wfs
                .iter()
                .flat_map(jobs)
                .map(|(_, job)| job)
                .filter(|job| {
                    job_segments(job)
                        .iter()
                        .any(|seg| cargo_sub(seg) == Some("clippy"))
                })
                .collect();
            let pinned =
                !clippy_jobs.is_empty() && clippy_jobs.iter().all(|job| job_pins_toolchain(job));
            let mut read = vec![absent];
            read.extend(wfs.iter().map(|wf| wf.rel.clone()));
            fact(pinned, read)
        }
    }
}

const GROUPS: [&str; 2] = ["clippy::pedantic", "clippy::restriction"];

/// H-26. No `clippy::pedantic` or `clippy::restriction` turned on as a group
/// in `rust-tops.yaml`, `Cargo.toml` lints, Cargo config rustflags, or a
/// workflow command. Crate attributes in source are not read.
fn no_pedantic_group(ctx: &Ctx) -> Choice {
    let mut read = vec!["rust-tops.yaml".to_string()];
    let mut group = yaml_at(&ctx.yaml, &["gates", "lint", "clippy_deny"])
        .and_then(Value::as_array)
        .is_some_and(|lints| {
            lints
                .iter()
                .any(|lint| lint.as_str().is_some_and(|s| GROUPS.contains(&s)))
        });
    if let Some(manifest) = read_toml(ctx.dir, "Cargo.toml", &mut read) {
        group |= manifest_lint(
            &manifest,
            "clippy",
            &["pedantic", "restriction"],
            &["warn", "deny", "forbid"],
        );
    }
    let flags = cargo_config_tokens(ctx.dir, &mut read);
    group |= GROUPS.iter().any(|g| sets_level(&flags, ENABLE, g));
    match &ctx.workflows {
        Workflows::Absent => {}
        Workflows::Broken(err) => {
            return not_evaluated_owned(format!("workflow did not parse: {err}"))
        }
        Workflows::Parsed(wfs) => {
            read.extend(wfs.iter().map(|wf| wf.rel.clone()));
            group |= wf_segments(wfs)
                .iter()
                .any(|seg| GROUPS.iter().any(|g| sets_level(seg, ENABLE, g)));
        }
    }
    fact(!group, read)
}

fn skips_baseline(tokens: &[String]) -> bool {
    tokens
        .windows(2)
        .any(|pair| pair[0] == "--baseline" && pair[1] == "skip")
        || tokens.iter().any(|token| token == "--baseline=skip")
}

/// H-30. No `cargo mutants` argv this gate ran or a workflow runs skips the
/// baseline.
fn baseline_green(ctx: &Ctx) -> Choice {
    let mut argvs: Vec<Vec<String>> = ctx
        .runs
        .iter()
        .filter(|run| run.layer == "mutation" && run.outcome != Outcome::ToolMissing)
        .map(|run| flag_tokens(&run.cmd))
        .collect();
    let mut read = Vec::new();
    if !argvs.is_empty() {
        read.push("gate: mutation layer".to_string());
    }
    match &ctx.workflows {
        Workflows::Absent => {}
        Workflows::Broken(err) => {
            return not_evaluated_owned(format!("workflow did not parse: {err}"))
        }
        Workflows::Parsed(wfs) => {
            let found: Vec<Vec<String>> = wf_segments(wfs)
                .into_iter()
                .filter(|seg| cargo_sub(seg) == Some("mutants"))
                .collect();
            if !found.is_empty() {
                read.extend(wfs.iter().map(|wf| wf.rel.clone()));
                argvs.extend(found);
            }
        }
    }
    if argvs.is_empty() {
        return not_evaluated_because("no cargo mutants command ran or appears in a workflow");
    }
    fact(!argvs.iter().any(|argv| skips_baseline(argv)), read)
}

const ARTIFACTS: [(&str, &str); 2] = [
    ("coverage", "target/coverage/lcov.info"),
    ("crap", "target/crap.json"),
];

/// H-35. Each artifact a coverage or crap run in this gate records is
/// present and nonempty.
fn record_nonempty(ctx: &Ctx) -> Choice {
    let read: Vec<String> = ARTIFACTS
        .iter()
        .filter(|(layer, _)| layer_ran(ctx, layer))
        .map(|(_, path)| path.to_string())
        .collect();
    if read.is_empty() {
        return not_evaluated_because("neither the coverage nor the crap layer ran in this gate");
    }
    let met = read
        .iter()
        .all(|path| fs::metadata(ctx.dir.join(path)).is_ok_and(|meta| meta.len() > 0));
    fact(met, read)
}

/// H-37. Every tool a command in this gate needed was on `PATH`.
fn tool_present(ctx: &Ctx) -> Choice {
    if ctx.runs.is_empty() {
        return not_evaluated_because("no command ran in this gate");
    }
    let met = ctx
        .runs
        .iter()
        .all(|run| run.outcome != Outcome::ToolMissing);
    fact(met, vec!["gate: tool lookup".into()])
}

/// H-43. The crap run read an LCOV that this gate's coverage run wrote.
fn crap_wires_coverage(ctx: &Ctx) -> Choice {
    if !layer_ran(ctx, "crap") {
        return not_evaluated_because("the crap layer did not run in this gate");
    }
    let lcov = ARTIFACTS[0].1;
    // Two seconds of slack for filesystems that store whole-second mtimes.
    let since = ctx
        .started
        .checked_sub(Duration::from_secs(2))
        .unwrap_or(ctx.started);
    let fresh = fs::metadata(ctx.dir.join(lcov))
        .and_then(|meta| meta.modified())
        .is_ok_and(|modified| modified >= since);
    let covered = ctx
        .runs
        .iter()
        .any(|run| run.layer == "coverage" && run.outcome == Outcome::Passed);
    fact(
        covered && fresh,
        vec![lcov.into(), "gate: coverage layer".into()],
    )
}

/// H-45. Workflow coverage runs with every `--all-features` / `--workspace`
/// flag a workflow test run uses.
fn coverage_features(ctx: &Ctx) -> Choice {
    with_workflows(ctx, |wfs, read| {
        let segs = wf_segments(wfs);
        let coverage: Vec<&Vec<String>> = segs
            .iter()
            .filter(|seg| cargo_sub(seg) == Some("llvm-cov"))
            .collect();
        let tests: Vec<&Vec<String>> = segs
            .iter()
            .filter(|seg| match cargo_sub(seg) {
                Some("nextest") => true,
                Some("test") => !has(seg, "--doc"),
                _ => false,
            })
            .collect();
        if coverage.is_empty() || tests.is_empty() {
            return not_evaluated_because(
                "workflows lack a cargo llvm-cov run or a test run to compare",
            );
        }
        let narrow = ["--all-features", "--workspace"].iter().any(|flag| {
            tests.iter().any(|seg| has(seg, flag)) && coverage.iter().any(|seg| !has(seg, flag))
        });
        fact(!narrow, read)
    })
}

const PERF_LAYERS: [&str; 2] = ["bench-criterion", "bench-callgrind"];

/// H-46. Every enabled performance layer in range ran a command that passed.
fn perf_runner(ctx: &Ctx) -> Choice {
    let layers: Vec<&str> = ctx
        .cfg
        .layers
        .iter()
        .filter(|layer| layer.enabled && layer.tier.unwrap_or(0) <= ctx.max_tier)
        .map(|layer| layer.id.as_str())
        .filter(|id| PERF_LAYERS.contains(id))
        .collect();
    if layers.is_empty() {
        return not_evaluated_because("no performance layer is enabled at or below this tier");
    }
    let measured = layers.iter().all(|id| {
        ctx.runs
            .iter()
            .any(|run| run.layer == *id && run.outcome == Outcome::Passed)
    });
    fact(
        measured,
        vec!["rust-tops.yaml".into(), "gate: performance layers".into()],
    )
}

/// H-50. No `test -s` is an argument of a cargo command (a backslash
/// continuation folds it in), and at least one stands on its own.
fn artifact_own_step(ctx: &Ctx) -> Choice {
    with_workflows(ctx, |wfs, read| {
        let segs = wf_segments(wfs);
        let is_check = |pair: &[String]| pair[0] == "test" && pair[1] == "-s";
        let folded = segs
            .iter()
            .any(|seg| cargo_sub(seg).is_some() && seg.windows(2).any(is_check));
        let own = segs.iter().any(|seg| seg.len() >= 2 && is_check(&seg[..2]));
        if !folded && !own {
            return not_evaluated_because("no test -s artifact check in a workflow");
        }
        fact(!folded, read)
    })
}

/// The job ids of the drop-in workflow `cargo tops init` writes.
pub fn floor_jobs() -> Vec<String> {
    parse_yaml(profiles::WORKFLOW_YML)
        .ok()
        .and_then(|doc| {
            doc.get("jobs")
                .and_then(Value::as_object)
                .map(|jobs| jobs.keys().cloned().collect())
        })
        .unwrap_or_default()
}

/// H-52. Every job of the drop-in workflow is still a job somewhere.
fn drop_in_floor(ctx: &Ctx) -> Choice {
    let floor = floor_jobs();
    if floor.is_empty() {
        return not_evaluated_because("the drop-in workflow template did not parse");
    }
    with_workflows(ctx, |wfs, read| {
        let present: Vec<&str> = wfs.iter().flat_map(jobs).map(|(id, _)| id).collect();
        fact(floor.iter().all(|id| present.contains(&id.as_str())), read)
    })
}

fn env_values<'v>(wf: &'v Workflow, key: &'v str) -> impl Iterator<Item = &'v str> {
    let job_values = jobs(wf).flat_map(|(_, job)| {
        std::iter::once(job.get("env")).chain(steps(job).iter().map(|step| step.get("env")))
    });
    std::iter::once(wf.doc.get("env"))
        .chain(job_values)
        .flatten()
        .filter_map(move |env| env.get(key))
        .filter_map(Value::as_str)
}

/// H-53. `-D warnings` has no second site: not in workflow `RUSTFLAGS`, Cargo
/// config rustflags, or `Cargo.toml` `[lints.rust] warnings`.
fn single_deny_site(ctx: &Ctx) -> Choice {
    let mut read = Vec::new();
    let mut extra = false;
    if let Some(manifest) = read_toml(ctx.dir, "Cargo.toml", &mut read) {
        extra |= manifest_lint(&manifest, "rust", &["warnings"], &["deny", "forbid"]);
    }
    extra |= sets_level(&cargo_config_tokens(ctx.dir, &mut read), DENY, "warnings");
    match &ctx.workflows {
        Workflows::Absent => {}
        Workflows::Broken(err) => {
            return not_evaluated_owned(format!("workflow did not parse: {err}"))
        }
        Workflows::Parsed(wfs) => {
            read.extend(wfs.iter().map(|wf| wf.rel.clone()));
            extra |= wfs.iter().any(|wf| {
                env_values(wf, "RUSTFLAGS")
                    .any(|flags| sets_level(&flag_tokens(flags), DENY, "warnings"))
            });
        }
    }
    if read.is_empty() {
        return not_evaluated_because("no Cargo.toml, Cargo config, or workflow to read");
    }
    fact(!extra, read)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn every_fact_names_a_law_its_function_and_both_constructors() {
        let pack = profiles::LAWS_BEND;
        for fact in FACTS {
            assert!(
                pack.contains(&format!("\nlaw {}:\n", fact.law)),
                "{} is not a law",
                fact.law
            );
            assert!(
                pack.contains(&format!("{{{}(", fact.fun)),
                "law {} does not call {}",
                fact.law,
                fact.fun
            );
            let def = format!("def {}(x: ", fact.fun);
            let typ = pack
                .split(&def)
                .nth(1)
                .and_then(|rest| rest.split(')').next())
                .unwrap_or_else(|| panic!("{} has no one-argument def", fact.fun));
            let block = pack
                .split(&format!("type {typ} is Data:\n"))
                .nth(1)
                .and_then(|rest| rest.split("\n\n").next())
                .unwrap_or_else(|| panic!("type {typ} is missing"));
            let ctors: Vec<&str> = block
                .lines()
                .map(|line| line.trim().trim_end_matches("{}"))
                .collect();
            assert_eq!(ctors.len(), 2, "{typ} is not a two-constructor case");
            assert!(
                ctors.contains(&fact.met) && ctors.contains(&fact.unmet),
                "{typ} is {ctors:?}, not {}/{}",
                fact.met,
                fact.unmet
            );
            assert_ne!(fact.met, fact.unmet);
        }
    }

    #[test]
    fn facts_follow_pack_order() {
        let order: Vec<&str> = profiles::LAWS_BEND
            .lines()
            .filter_map(|line| {
                line.strip_prefix("law ")
                    .and_then(|rest| rest.strip_suffix(':'))
            })
            .collect();
        let positions: Vec<usize> = FACTS
            .iter()
            .map(|fact| {
                order
                    .iter()
                    .position(|law| *law == fact.law)
                    .expect("law in pack")
            })
            .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "FACTS out of LAWS.bend order"
        );
    }

    #[test]
    fn evidence_not_self_served_has_no_fact() {
        assert!(find("evidence_not_self_served").is_none());
        assert!(not_evaluated("evidence_not_self_served").starts_with("intent is not readable"));
    }

    #[test]
    fn segments_join_continuations_and_split_separators() {
        let run = "mkdir -p target/coverage\ncargo llvm-cov nextest --workspace \\\n  --lcov --output-path x\ntest -s x\n# comment\ncargo crap --lcov x && test -s y";
        assert_eq!(
            segments(run),
            vec![
                seg("mkdir -p target/coverage"),
                seg("cargo llvm-cov nextest --workspace --lcov --output-path x"),
                seg("test -s x"),
                seg("cargo crap --lcov x"),
                seg("test -s y"),
            ]
        );
    }

    #[test]
    fn a_continued_test_s_is_cargo_argv() {
        let segs = segments("cargo llvm-cov --lcov --output-path x \\\n  test -s x");
        assert_eq!(segs.len(), 1);
        assert_eq!(cargo_sub(&segs[0]), Some("llvm-cov"));
        assert!(segs[0]
            .windows(2)
            .any(|pair| pair[0] == "test" && pair[1] == "-s"));
    }

    #[test]
    fn cargo_sub_skips_toolchain_flags_and_env_prefixes() {
        assert_eq!(
            cargo_sub(&seg("cargo +nightly fmt --all -- --check")),
            Some("fmt")
        );
        assert_eq!(
            cargo_sub(&seg("RUSTFLAGS=-Dwarnings cargo build")),
            Some("build")
        );
        assert_eq!(cargo_sub(&seg("cargo --locked check")), Some("check"));
        assert_eq!(cargo_sub(&seg("test -s x")), None);
    }

    #[test]
    fn lint_levels_in_every_spelling() {
        for flags in [
            "-D warnings",
            "-Dwarnings",
            "--deny warnings",
            "--deny=warnings",
            "-F warnings",
        ] {
            assert!(sets_level(&flag_tokens(flags), DENY, "warnings"), "{flags}");
        }
        assert!(!sets_level(&flag_tokens("-W warnings"), DENY, "warnings"));
        assert!(!sets_level(
            &flag_tokens("-A clippy::pedantic"),
            ENABLE,
            "clippy::pedantic"
        ));
        assert!(sets_level(
            &flag_tokens(r#"["-W", "clippy::pedantic"]"#),
            ENABLE,
            "clippy::pedantic"
        ));
    }

    #[test]
    fn versioned_channels() {
        for pinned in ["1.85", "1.99.1", "nightly-2026-10-01", "stable-2026-01-31"] {
            assert!(versioned(pinned), "{pinned}");
        }
        for loose in [
            "stable",
            "nightly",
            "beta",
            "1",
            "1.x",
            "nightly-2026-1-1",
            "{MSRV}",
        ] {
            assert!(!versioned(loose), "{loose}");
        }
    }

    #[test]
    fn toolchain_channel_reads_toml_and_legacy_files() {
        assert_eq!(
            toolchain_channel("[toolchain]\nchannel = \"1.99.1\"\n").as_deref(),
            Some("1.99.1")
        );
        assert_eq!(
            toolchain_channel("# pin\n1.85.0\n").as_deref(),
            Some("1.85.0")
        );
        assert_eq!(toolchain_channel("[toolchain]\ncomponents = []\n"), None);
    }

    #[test]
    fn manifest_lints_as_strings_and_tables() {
        let manifest: toml::Table =
            "[workspace.lints.clippy]\npedantic = { level = \"warn\", priority = -1 }\n[lints.rust]\nwarnings = \"deny\"\n"
                .parse()
                .expect("toml");
        assert!(manifest_lint(
            &manifest,
            "clippy",
            &["pedantic"],
            &["warn", "deny", "forbid"]
        ));
        assert!(manifest_lint(
            &manifest,
            "rust",
            &["warnings"],
            &["deny", "forbid"]
        ));
        let allowed: toml::Table = "[lints.clippy]\npedantic = \"allow\"\n"
            .parse()
            .expect("toml");
        assert!(!manifest_lint(
            &allowed,
            "clippy",
            &["pedantic"],
            &["warn", "deny", "forbid"]
        ));
    }

    #[test]
    fn rustflags_under_any_table() {
        let config: toml::Table =
            "[build]\nrustflags = [\"-D\", \"warnings\"]\n[target.x86_64-unknown-linux-gnu]\nrustflags = \"-W clippy::pedantic\"\n[env]\nOTHER = \"-D warnings\"\n"
                .parse()
                .expect("toml");
        assert_eq!(
            rustflags_tokens(&config),
            seg("-D warnings -W clippy::pedantic")
        );
    }

    #[test]
    fn drop_in_template_parses_to_its_floor_jobs() {
        let floor = floor_jobs();
        for id in [
            "fmt-clippy-deny",
            "nextest",
            "msrv",
            "feature-matrix",
            "coverage-crap",
        ] {
            assert!(
                floor.iter().any(|job| job == id),
                "{id} missing from {floor:?}"
            );
        }
    }

    #[test]
    fn baseline_skip_spellings() {
        assert!(skips_baseline(&seg("cargo mutants --baseline skip")));
        assert!(skips_baseline(&seg("cargo mutants --baseline=skip")));
        assert!(!skips_baseline(&seg("cargo mutants --in-diff d")));
    }

    #[test]
    fn every_init_class_has_a_section_4_row() {
        for class in [
            "library-core",
            "parser-codec",
            "no-std-embedded",
            "unsafe-kernel",
            "binary-cli",
        ] {
            assert!(class_keywords(class).is_some(), "{class}");
        }
        assert!(class_keywords("made-up").is_none());
    }
}
