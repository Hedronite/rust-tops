//! Argv steps for `cargo tops gate`.
//!
//! A layer is a list of [`Step`] values chained with `&&`, not one shell
//! string. [`tool_binary`] names the binary the command runs. `mkdir` and
//! `sh` are not that binary when the command runs `cargo llvm-cov` or
//! `cargo mutants`.
//!
//! A mutation diff that failed is [`Outcome::Failed`]. No config key waives
//! it. A diff that succeeded and is empty is [`Outcome::Vacuous`]: print it,
//! and count it for no law. [`init_exemptions`] is empty; `init` writes none.
//!
//! `main.rs` is the caller. This allow holds until that wiring references
//! the items.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
#![allow(dead_code, reason = "main.rs wires these items")]

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DIFF_NAME: &str = "rust-tops.diff";

/// The coverage layer's LCOV, under the gate's artifact directory.
pub(crate) const LCOV: &str = "lcov.info";
/// The coverage layer's per-file summary (`llvm.coverage.json.export`),
/// under the gate's artifact directory. The floors are judged on it.
pub(crate) const SUMMARY: &str = "coverage.json";
/// The crap layer's report, under the gate's artifact directory.
pub(crate) const CRAP: &str = "crap.json";

/// What a step is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// [`tool_binary`] must be on `PATH`. A miss does not pass the layer.
    Tool,
    /// Run `argv`. A non-zero status is [`Outcome::Failed`].
    Command,
    /// `test -s`: the artifact exists and is non-empty.
    Artifact,
}

/// One argv invocation in a layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    pub argv: Vec<String>,
    pub kind: Kind,
}

impl Step {
    #[must_use]
    pub(crate) fn new(kind: Kind, argv: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            kind,
            argv: argv.into_iter().map(Into::into).collect(),
        }
    }

    #[must_use]
    pub(crate) fn tool(argv: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::new(Kind::Tool, argv)
    }

    #[must_use]
    pub(crate) fn command(argv: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::new(Kind::Command, argv)
    }

    /// `test -s <path>`.
    #[must_use]
    pub(crate) fn artifact(path: impl Into<String>) -> Self {
        let path = path.into();
        Self::new(Kind::Artifact, ["test", "-s", path.as_str()])
    }

    /// Program and its arguments. Empty argv is `None`.
    #[must_use]
    pub(crate) fn program(&self) -> Option<(&str, &[String])> {
        let (program, args) = self.argv.split_first()?;
        Some((program.as_str(), args))
    }

    /// Path a [`Kind::Artifact`] step checks.
    #[must_use]
    pub(crate) fn artifact_path(&self) -> Option<&Path> {
        if self.kind != Kind::Artifact {
            return None;
        }
        let path = match self.argv.as_slice() {
            [test, flag, path, ..] if test == "test" && flag == "-s" => path,
            [.., path] => path,
            [] => return None,
        };
        Some(Path::new(path))
    }

    #[must_use]
    pub(crate) fn display(&self) -> String {
        self.argv.join(" ")
    }
}

/// How a step or a mutation diff finished.
///
/// [`Failed`](Self::Failed) and [`Vacuous`](Self::Vacuous) are different.
/// Vacuous counts for no law.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Passed,
    Failed,
    ToolMissing,
    /// The diff command succeeded and the diff is empty.
    Vacuous,
}

impl Outcome {
    /// `Some(true)` pass, `Some(false)` fail, `None` when no law may count it.
    #[must_use]
    pub(crate) fn law_verdict(self) -> Option<bool> {
        match self {
            Self::Passed => Some(true),
            Self::Failed | Self::ToolMissing => Some(false),
            Self::Vacuous => None,
        }
    }

    /// Vacuous is not a pass and not a fail, so no law counts it.
    #[must_use]
    pub(crate) fn counts_for_a_law(self) -> bool {
        self.law_verdict().is_some()
    }

    /// Gate failure. Vacuous does not fail the gate.
    #[must_use]
    pub(crate) fn fails_the_gate(self) -> bool {
        matches!(self, Self::Failed | Self::ToolMissing)
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Passed => "passed",
            Self::Failed => "Failed",
            Self::ToolMissing => "not on PATH",
            Self::Vacuous => "Vacuous",
        })
    }
}

/// The binary a tool step names was not on `PATH`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MissingTool {
    pub binary: String,
}

impl fmt::Display for MissingTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let binary = &self.binary;
        write!(
            f,
            "`{binary}` not on PATH (install it; cargo-tops does not vendor engines)"
        )
    }
}

impl std::error::Error for MissingTool {}

/// `Ok` when the binary [`tool_binary`] names is on `PATH`.
///
/// Call this for [`Kind::Tool`]. The name is the binary the command runs,
/// not the first word of a shell string.
pub(crate) fn check_tool(argv: &[String]) -> Result<(), MissingTool> {
    let binary = tool_binary(argv).unwrap_or_else(|| "command".to_string());
    if binary_present(&binary) {
        Ok(())
    } else {
        Err(MissingTool { binary })
    }
}

/// Binary the command actually runs.
///
/// Walks `&&` and `;` segments and unwraps `sh -c`. A leading `mkdir` or
/// `sh` is not the result when a later segment runs `cargo llvm-cov` or
/// `cargo mutants`.
#[must_use]
pub(crate) fn tool_binary(argv: &[String]) -> Option<String> {
    let tokens = tokens_from_argv(argv);
    best_binary(&split_chain(&tokens))
}

/// Coverage is `mkdir`, then `cargo llvm-cov`, then `test -s`, with
/// [`LCOV`] under `dir`, the gate's artifact directory, then the per-file
/// [`SUMMARY`] of the same run (`cargo llvm-cov report`, which reads that
/// run's profiles and runs no test) and its `test -s`.
/// The tool step names `cargo-llvm-cov`, not `mkdir`. It runs the suite with
/// the same `--workspace`, `--all-features` and `--profile ci` as `unit`, so
/// its run can stand for `unit`, `integration` and `property`. The run is
/// workspace-wide even for a crate's instance, so a test in any member
/// counts; the floors judge only that crate's files (`scope`).
#[must_use]
pub(crate) fn coverage_steps(dir: &Path) -> Vec<Step> {
    let dir_shown = dir.display().to_string();
    let lcov = dir.join(LCOV).display().to_string();
    let summary = dir.join(SUMMARY).display().to_string();
    vec![
        Step::command(["mkdir", "-p", dir_shown.as_str()]),
        Step::tool([
            "cargo",
            "llvm-cov",
            "nextest",
            "--workspace",
            "--all-features",
            "--profile",
            "ci",
            "--lcov",
            "--output-path",
            lcov.as_str(),
        ]),
        Step::artifact(lcov),
        Step::tool([
            "cargo",
            "llvm-cov",
            "report",
            "--json",
            "--summary-only",
            "--output-path",
            summary.as_str(),
        ]),
        Step::artifact(summary),
    ]
}

/// `test -s` is true when the path exists and its size is greater than zero.
/// An empty artifact is [`Outcome::Failed`], not [`Outcome::Vacuous`].
#[must_use]
pub(crate) fn artifact_outcome(path: &Path) -> Outcome {
    let present = fs::metadata(path).is_ok_and(|meta| meta.len() > 0);
    if present {
        Outcome::Passed
    } else {
        Outcome::Failed
    }
}

/// A failed or vacuous outcome stops the `&&` chain. Only a pass continues.
#[must_use]
pub(crate) fn chain_continues(outcome: Outcome) -> bool {
    outcome == Outcome::Passed
}

/// One layer id and the date the exemption stops.
///
/// Not a list of layers. [`init_exemptions`] is what `init` writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Exemption {
    pub layer: String,
    pub expires: String,
}

/// Why [`Exemption::new`] refused a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExemptionError {
    /// Missing layer id, or more than one.
    Layer,
    /// Expiry is not a real `YYYY-MM-DD` date.
    Expiry,
}

impl fmt::Display for ExemptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Layer => "an exemption names one layer id",
            Self::Expiry => "an exemption needs an expiry date (YYYY-MM-DD)",
        })
    }
}

impl std::error::Error for ExemptionError {}

impl Exemption {
    /// `layer` is one id such as `mutation`. `expires` is `YYYY-MM-DD`.
    pub(crate) fn new(
        layer: impl AsRef<str>,
        expires: impl AsRef<str>,
    ) -> Result<Self, ExemptionError> {
        let layer = layer.as_ref();
        if !one_layer(layer) {
            return Err(ExemptionError::Layer);
        }
        let expires = expires.as_ref();
        if !valid_expiry(expires) {
            return Err(ExemptionError::Expiry);
        }
        Ok(Self {
            layer: layer.to_string(),
            expires: expires.to_string(),
        })
    }
}

/// Exemptions `init` writes. Always none.
#[must_use]
pub(crate) fn init_exemptions() -> &'static [Exemption] {
    &[]
}

#[expect(
    clippy::fn_params_excessive_bools,
    reason = "already in this crate when the strict table was turned on"
)]
/// Classify a mutation diff. A failed command is [`Outcome::Failed`] even
/// when the diff is empty. There is no waiver parameter and no config key
/// is read. A successful empty diff is [`Outcome::Vacuous`].
#[must_use]
pub(crate) fn diff_outcome(command_ok: bool, empty: bool) -> Outcome {
    if !command_ok {
        return Outcome::Failed;
    }
    if empty {
        Outcome::Vacuous
    } else {
        Outcome::Passed
    }
}

#[expect(
    clippy::fn_params_excessive_bools,
    reason = "already in this crate when the strict table was turned on"
)]
/// Read the diff file only after the command succeeded.
///
/// A failed command is [`Outcome::Failed`] even if a stale diff is sitting
/// at `diff_path`. A successful empty file is [`Outcome::Vacuous`].
#[must_use]
pub(crate) fn judge_diff(command_ok: bool, diff_path: &Path) -> Outcome {
    if !command_ok {
        return diff_outcome(false, false);
    }
    match fs::metadata(diff_path) {
        Ok(meta) if meta.len() == 0 => diff_outcome(true, true),
        Ok(_) => diff_outcome(true, false),
        Err(_) => diff_outcome(false, false),
    }
}

/// Where the mutation base came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BaseSource {
    /// `--base <ref>`.
    Flag,
    /// `gates.base_ref` in `rust-tops.yaml`.
    Key,
    /// The remote's default branch, `refs/remotes/origin/HEAD`.
    OriginHead,
}

impl BaseSource {
    /// How a message names the source.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Flag => "--base",
            Self::Key => "gates.base_ref",
            Self::OriginHead => "origin/HEAD",
        }
    }

    /// How `report.json` names the source.
    pub(crate) fn report_name(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::Key => "key",
            Self::OriginHead => "origin/HEAD",
        }
    }
}

/// The ref the mutation diff starts from, and what became of looking for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Base {
    /// A ref that names a commit.
    Found { source: BaseSource, name: String },
    /// A ref a flag or the config named that is not a commit: the layer
    /// fails, naming it.
    Unresolved { source: BaseSource, name: String },
    /// Nothing named a base and the remote has no default branch. `tried`
    /// has one line per source; `ci` is whether `CI` was set when the plan
    /// was made, which decides whether the layer fails or is vacuous.
    Missing { tried: Vec<String>, ci: bool },
}

impl Base {
    /// A base nothing was asked for: the layer that would use it is off.
    pub(crate) fn unused() -> Self {
        Self::Missing {
            tried: Vec::new(),
            ci: false,
        }
    }
}

/// The base for the mutation diff: `flag`, else `key`, else the remote's
/// default branch (`origin/HEAD`). A named base is never replaced by a later
/// source, so one that does not resolve is [`Base::Unresolved`].
pub(crate) fn resolve_base(dir: &Path, flag: Option<&str>, key: Option<&str>) -> Base {
    let named = flag
        .map(|name| (BaseSource::Flag, name))
        .or_else(|| key.map(|name| (BaseSource::Key, name)));
    match named {
        Some((source, name)) => {
            let name = name.to_string();
            if names_a_commit(dir, &name) {
                Base::Found { source, name }
            } else {
                Base::Unresolved { source, name }
            }
        }
        None => match origin_head(dir) {
            Ok(name) => Base::Found {
                source: BaseSource::OriginHead,
                name,
            },
            Err(note) => Base::Missing {
                tried: vec![
                    "--base: not given".to_string(),
                    "gates.base_ref: not set".to_string(),
                    note,
                ],
                ci: ci_active(std::env::var("CI").ok().as_deref()),
            },
        },
    }
}

/// The remote's default branch as a ref, or why there is none.
fn origin_head(dir: &Path) -> Result<String, String> {
    let named = git_line(
        dir,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    );
    match named {
        Some(name) if names_a_commit(dir, &name) => Ok(name),
        Some(name) => Err(format!(
            "origin/HEAD: points at {name}, which is not a commit"
        )),
        // Not a symbolic ref. A direct ref that names a commit is a base.
        None if names_a_commit(dir, "origin/HEAD") => Ok("origin/HEAD".to_string()),
        None => Err("origin/HEAD: not set".to_string()),
    }
}

/// Why the mutation layer stopped before `cargo mutants`, and its outcome.
/// This is the one place the words live: stdout, the report's `detail` and
/// its `problems` all print what this returns. `diff` is the outcome of the
/// diff command, or `Failed` for a base that was never tried. `None` means
/// the diff passed and mutants runs.
pub(crate) fn stop_note(base: &Base, diff: Outcome) -> Option<(Outcome, String)> {
    match (base, diff) {
        (Base::Unresolved { source, name }, _) => Some((
            Outcome::Failed,
            format!(
                "{} names `{name}`, which is not a commit; the mutation diff cannot start there",
                source.label()
            ),
        )),
        (Base::Missing { tried, ci: true }, Outcome::Vacuous) => Some(no_base_in_ci(tried)),
        (Base::Missing { tried, ci: false }, Outcome::Vacuous) => Some(no_base_outside_ci(tried)),
        (Base::Found { name, .. }, Outcome::Vacuous) => Some((
            Outcome::Vacuous,
            format!(
                "the diff from `git merge-base {name} HEAD` is empty; \
                 cargo mutants did not run and no law counts this layer"
            ),
        )),
        (Base::Found { name, .. }, Outcome::Failed) => Some((
            Outcome::Failed,
            format!(
                "the mutation diff did not succeed: `{name}` and HEAD share no history, \
                 as in a shallow checkout (use `fetch-depth: 0`); a failed diff is never waived"
            ),
        )),
        (Base::Missing { .. }, Outcome::Failed) => Some((
            Outcome::Failed,
            "the mutation diff did not succeed; a failed diff is never waived".to_string(),
        )),
        _ => None,
    }
}

/// No source gave a base under CI: a base that was never fetched must not
/// read as a clean diff.
fn no_base_in_ci(tried: &[String]) -> (Outcome, String) {
    (
        Outcome::Failed,
        format!(
            "no mutation base ({}); under CI that is never a clean diff. \
             Check out with `fetch-depth: 0` and pass `--base <ref>`, or set `gates.base_ref`",
            tried.join("; ")
        ),
    )
}

/// No source gave a base outside CI: the diff is empty and the layer vacuous.
fn no_base_outside_ci(tried: &[String]) -> (Outcome, String) {
    (
        Outcome::Vacuous,
        format!(
            "no mutation base ({}); the mutation diff is empty; \
             cargo mutants did not run and no law counts this layer",
            tried.join("; ")
        ),
    )
}

/// The first line git prints for `args`, when it succeeds with one.
fn git_line(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next().unwrap_or_default().trim();
    (out.status.success() && !line.is_empty()).then(|| line.to_string())
}

/// Whether `name` is a ref that names a commit. A name that starts with `-`
/// is never one: it would be read as an option.
fn names_a_commit(dir: &Path, name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && git_line(
            dir,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{name}^{{commit}}"),
            ],
        )
        .is_some()
}

/// Whether `CI` says this is a CI run: set, not empty, and not `false` or
/// `0` in any case.
pub(crate) fn ci_active(value: Option<&str>) -> bool {
    value
        .map(str::trim)
        .is_some_and(|text| !text.is_empty() && text != "0" && !text.eq_ignore_ascii_case("false"))
}

/// Mutation diff command, then the `cargo mutants` tool step.
///
/// The diff command writes `<report_dir>/rust-tops.diff`: the changes from
/// `git merge-base <base> HEAD` to `HEAD`. Run it with the project as the
/// working directory. [`MutationPlan::after_diff`] runs mutants only after a
/// non-empty diff; a failed diff is not waived, and an empty diff is not
/// handed to `cargo mutants`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MutationPlan {
    pub diff: Step,
    pub diff_path: PathBuf,
    pub mutants: Step,
    pub base: Base,
}

impl MutationPlan {
    #[must_use]
    pub(crate) fn new(report_dir: &Path, base: Base) -> Self {
        let diff_path = report_dir.join(DIFF_NAME);
        let diff_arg = diff_path.display().to_string();
        let from = match &base {
            Base::Found { name, .. } | Base::Unresolved { name, .. } => Some(name.as_str()),
            Base::Missing { .. } => None,
        };
        let script = mutation_diff_script(report_dir, &diff_path, from);
        let diff = Step::command(["sh", "-c", script.as_str()]);
        let mutants = Step::tool([
            "cargo",
            "mutants",
            "--in-diff",
            diff_arg.as_str(),
            "-j",
            "2",
        ]);
        Self {
            diff,
            diff_path,
            mutants,
            base,
        }
    }

    /// Mutants runs only when the diff passed. Failed and Vacuous stop.
    #[must_use]
    pub(crate) fn after_diff(&self, outcome: Outcome) -> Option<&Step> {
        chain_continues(outcome).then_some(&self.mutants)
    }
}

/// Not a git work tree, or no commit: the command fails. With a base: the
/// diff from `git merge-base <base> HEAD` to `HEAD`, and no merge-base is a
/// failure. With none: an empty diff, so the layer is Vacuous.
fn mutation_diff_script(report_dir: &Path, diff_path: &Path, base: Option<&str>) -> String {
    let dir = shell_quote(&report_dir.display().to_string());
    let file = shell_quote(&diff_path.display().to_string());
    let head = format!("mkdir -p {dir} && git rev-parse --verify --quiet HEAD >/dev/null");
    match base {
        Some(name) => format!(
            "{head} && base=\"$(git merge-base {} HEAD)\" \
             && git diff \"$base\" HEAD --unified=0 > {file}",
            shell_quote(name)
        ),
        None => format!("{head} && : > {file}"),
    }
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn one_layer(layer: &str) -> bool {
    !layer.is_empty()
        && !layer.starts_with('-')
        && !layer.ends_with('-')
        && !layer.contains("--")
        && layer
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Whether `text` is a real `YYYY-MM-DD` day.
pub(crate) fn valid_expiry(text: &str) -> bool {
    let bytes = text.as_bytes();
    #[expect(
        clippy::indexing_slicing,
        reason = "already in this crate when the strict table was turned on"
    )]
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let Ok(year) = text[0..4].parse::<i32>() else {
        return false;
    };
    let Ok(month) = text[5..7].parse::<u8>() else {
        return false;
    };
    let Ok(day) = text[8..10].parse::<u8>() else {
        return false;
    };
    let max = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap(year) => 29,
        2 => 28,
        _ => return false,
    };
    (1..=max).contains(&day)
}

fn leap(year: i32) -> bool {
    let by4 = year % 4 == 0;
    let by100 = year % 100 == 0;
    let by400 = year % 400 == 0;
    by4 && (!by100 || by400)
}

fn binary_present(binary: &str) -> bool {
    on_path(binary) || aliases(binary).iter().any(|name| on_path(name))
}

fn aliases(binary: &str) -> &'static [&'static str] {
    match binary {
        "cargo-crap" => &["cargo-crappy"],
        "cargo-kani" => &["kani"],
        _ => &[],
    }
}

fn on_path(bin: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| is_executable(&dir.join(bin)))
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn tokens_from_argv(argv: &[String]) -> Vec<String> {
    #[expect(
        clippy::indexing_slicing,
        reason = "already in this crate when the strict table was turned on"
    )]
    if argv.len() >= 3 && argv[0] == "sh" && argv[1] == "-c" {
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let mut tokens = tokenize(&argv[2]);
        tokens.extend(argv.iter().skip(3).cloned());
        return unwrap_all(tokens);
    }
    let mut tokens = Vec::new();
    for arg in argv {
        if contains_shell_syntax(arg) {
            tokens.extend(tokenize(arg));
        } else {
            tokens.push(arg.clone());
        }
    }
    unwrap_all(tokens)
}

fn contains_shell_syntax(arg: &str) -> bool {
    arg.contains([' ', '\t', '\n', '&', ';', '|', '\'', '"', '$', '>', '<'])
}

fn unwrap_all(mut tokens: Vec<String>) -> Vec<String> {
    for _ in 0..8 {
        let (next, changed) = unwrap_once(&tokens);
        if !changed {
            return next;
        }
        tokens = next;
    }
    tokens
}

fn unwrap_once(tokens: &[String]) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let mut changed = false;
    let mut index = 0;
    while index < tokens.len() {
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let is_shell = tokens[index] == "sh"
            && tokens.get(index + 1).map(String::as_str) == Some("-c")
            && index + 2 < tokens.len();
        if is_shell {
            #[expect(
                clippy::indexing_slicing,
                reason = "already in this crate when the strict table was turned on"
            )]
            out.extend(tokenize(&tokens[index + 2]));
            index += 3;
            changed = true;
        } else {
            #[expect(
                clippy::indexing_slicing,
                reason = "already in this crate when the strict table was turned on"
            )]
            out.push(tokens[index].clone());
            index += 1;
        }
    }
    (out, changed)
}

fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut buf = String::new();
    let mut chars = input.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                buf.push(c);
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            ' ' | '\t' | '\n' | '\r' => flush(&mut tokens, &mut buf),
            '&' if chars.peek() == Some(&'&') => {
                chars.next();
                flush(&mut tokens, &mut buf);
                tokens.push("&&".to_string());
            }
            '|' if chars.peek() == Some(&'|') => {
                chars.next();
                flush(&mut tokens, &mut buf);
                tokens.push("||".to_string());
            }
            ';' | '|' => {
                flush(&mut tokens, &mut buf);
                tokens.push(c.to_string());
            }
            _ => buf.push(c),
        }
    }
    flush(&mut tokens, &mut buf);
    tokens
}

fn flush(tokens: &mut Vec<String>, buf: &mut String) {
    if !buf.is_empty() {
        tokens.push(std::mem::take(buf));
    }
}

fn split_chain(tokens: &[String]) -> Vec<Vec<String>> {
    let mut segments = Vec::new();
    let mut current = Vec::new();
    for token in tokens {
        if matches!(token.as_str(), "&&" | "||" | ";" | "|") {
            if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }
        } else {
            current.push(token.clone());
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

fn best_binary(segments: &[Vec<String>]) -> Option<String> {
    let mut builtin = None;
    let mut other = None;
    for segment in segments {
        let Some(argv0) = command_word(segment) else {
            continue;
        };
        if argv0 == "cargo" {
            let name = cargo_binary(cargo_subcommand(segment).as_deref());
            if name == "cargo" {
                builtin.get_or_insert(name);
            } else {
                return Some(name);
            }
        } else if !is_prep(&argv0) {
            other.get_or_insert(argv0);
        }
    }
    builtin
        .or(other)
        .or_else(|| segments.iter().find_map(|segment| command_word(segment)))
}

fn command_word(segment: &[String]) -> Option<String> {
    segment.iter().find_map(|token| {
        if token.is_empty() || is_redirect(token) || is_env_assign(token) {
            None
        } else {
            Some(token.clone())
        }
    })
}

fn cargo_subcommand(segment: &[String]) -> Option<String> {
    let mut seen_cargo = false;
    for token in segment {
        if token.is_empty() || is_redirect(token) || is_env_assign(token) {
            continue;
        }
        if !seen_cargo {
            if token == "cargo" {
                seen_cargo = true;
            }
            continue;
        }
        if token.starts_with('+') || token.starts_with('-') {
            continue;
        }
        return Some(token.clone());
    }
    None
}

/// Built-ins (`fmt`, `clippy`, `test`, `build`, `check`, `bench`) and unknown
/// subcommands are `cargo`. External subcommands are `cargo-<sub>`.
fn cargo_binary(sub: Option<&str>) -> String {
    match sub {
        Some("nextest") => "cargo-nextest",
        Some("llvm-cov") => "cargo-llvm-cov",
        Some("crap") => "cargo-crap",
        Some("mutants") => "cargo-mutants",
        Some("deny") => "cargo-deny",
        Some("fuzz") => "cargo-fuzz",
        Some("kani") => "cargo-kani",
        _ => "cargo",
    }
    .to_string()
}

fn is_prep(argv0: &str) -> bool {
    matches!(argv0, "mkdir" | "sh" | "test")
}

fn is_redirect(token: &str) -> bool {
    if token.starts_with('>') || token.starts_with('<') {
        return true;
    }
    match token.find('>') {
        Some(at) if at > 0 => token[..at].chars().all(|c| c.is_ascii_digit()),
        _ => false,
    }
}

fn is_env_assign(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && !name.starts_with('-')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod expiry {
    use super::{Exemption, ExemptionError, Outcome};

    #[test]
    fn a_bad_layer_is_refused_and_each_refusal_says_why() {
        assert_eq!(
            Exemption::new("crap mutation", "2030-01-01"),
            Err(ExemptionError::Layer)
        );
        assert_eq!(Exemption::new("", "2030-01-01"), Err(ExemptionError::Layer));
        assert_eq!(
            ExemptionError::Layer.to_string(),
            "an exemption names one layer id"
        );
        assert_eq!(
            ExemptionError::Expiry.to_string(),
            "an exemption needs an expiry date (YYYY-MM-DD)"
        );
        assert_eq!(Outcome::Vacuous.to_string(), "Vacuous");
    }

    #[test]
    fn a_real_date_is_accepted_and_a_fake_one_is_not() {
        assert!(Exemption::new("mutation", "2024-02-29").is_ok());
        assert!(Exemption::new("mutation", "2023-02-29").is_err());
        assert!(Exemption::new("mutation", "2024-13-01").is_err());
        assert!(Exemption::new("mutation", "not-a-date").is_err());
    }
}

#[cfg(test)]
mod base {
    use super::{
        ci_active, git_line, mutation_diff_script, shell_quote, stop_note, Base, BaseSource,
        MutationPlan, Outcome,
    };
    use std::path::Path;

    #[test]
    fn ci_is_a_value_that_is_neither_empty_false_nor_zero() {
        for on in ["true", "1", "yes", "TRUE", " true "] {
            assert!(ci_active(Some(on)), "{on}");
        }
        for off in ["", " ", "false", "False", "FALSE", "0", " 0 "] {
            assert!(!ci_active(Some(off)), "{off:?}");
        }
        assert!(!ci_active(None));
    }

    #[test]
    fn each_source_has_a_label_and_a_report_name() {
        assert_eq!(BaseSource::Flag.label(), "--base");
        assert_eq!(BaseSource::Key.label(), "gates.base_ref");
        assert_eq!(BaseSource::OriginHead.label(), "origin/HEAD");
        assert_eq!(BaseSource::Flag.report_name(), "flag");
        assert_eq!(BaseSource::Key.report_name(), "key");
        assert_eq!(BaseSource::OriginHead.report_name(), "origin/HEAD");
    }

    #[test]
    fn a_name_is_quoted_for_the_shell() {
        assert_eq!(shell_quote("origin/main"), "'origin/main'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
        assert_eq!(shell_quote("$(x)"), "'$(x)'");
    }

    #[test]
    fn the_script_diffs_from_the_merge_base_or_writes_an_empty_file() {
        let report = Path::new("/r");
        let file = Path::new("/r/rust-tops.diff");
        let with = mutation_diff_script(report, file, Some("origin/dev"));
        assert_eq!(
            with,
            "mkdir -p '/r' && git rev-parse --verify --quiet HEAD >/dev/null \
             && base=\"$(git merge-base 'origin/dev' HEAD)\" \
             && git diff \"$base\" HEAD --unified=0 > '/r/rust-tops.diff'"
        );
        let without = mutation_diff_script(report, file, None);
        assert_eq!(
            without,
            "mkdir -p '/r' && git rev-parse --verify --quiet HEAD >/dev/null \
             && : > '/r/rust-tops.diff'"
        );
    }

    #[test]
    fn the_plan_keeps_its_base_and_an_unused_base_names_no_source() {
        let found = Base::Found {
            source: BaseSource::Key,
            name: "origin/dev".to_string(),
        };
        let plan = MutationPlan::new(Path::new("/r"), found.clone());
        assert_eq!(plan.base, found);
        assert!(plan.diff.argv.join(" ").contains("merge-base 'origin/dev'"));
        let none = MutationPlan::new(Path::new("/r"), Base::unused());
        assert_eq!(none.base, Base::unused());
        assert!(!none.diff.argv.join(" ").contains("merge-base"));
    }

    #[test]
    fn a_line_is_what_a_successful_git_prints_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("cargo-tops-git-line-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("mkdir");
        let git = |args: &[&str]| {
            git_line(&dir, args);
        };
        git(&["init", "-q"]);
        assert_eq!(
            git_line(&dir, &["rev-parse", "--git-dir"]).as_deref(),
            Some(".git")
        );
        // Success with no output is no line.
        assert_eq!(git_line(&dir, &["status", "--porcelain"]), None);
        // Failure with output is no line either: `diff --exit-code` prints
        // the change and exits 1.
        std::fs::write(dir.join("f"), "one\n").expect("write");
        let add = std::process::Command::new("git")
            .args(["add", "-N", "f"])
            .current_dir(&dir)
            .status()
            .expect("spawn git");
        assert!(add.success());
        assert_eq!(git_line(&dir, &["diff", "--exit-code"]), None);
        assert_eq!(
            git_line(&dir, &["status", "--porcelain"]).as_deref(),
            Some("A f")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    fn tried() -> Vec<String> {
        vec!["a".to_string(), "b".to_string()]
    }

    fn missing_in_ci() -> Base {
        Base::Missing {
            tried: tried(),
            ci: true,
        }
    }

    fn missing_outside_ci() -> Base {
        Base::Missing {
            tried: tried(),
            ci: false,
        }
    }

    fn found() -> Base {
        Base::Found {
            source: BaseSource::Flag,
            name: "origin/dev".to_string(),
        }
    }

    #[test]
    fn no_base_fails_under_ci_naming_the_remedy_and_is_vacuous_otherwise() {
        let (outcome, note) = stop_note(&missing_in_ci(), Outcome::Vacuous).expect("a stop");
        assert_eq!(outcome, Outcome::Failed);
        assert!(note.contains("(a; b)"), "{note}");
        assert!(
            note.contains("`fetch-depth: 0`") && note.contains("`--base <ref>`"),
            "{note}"
        );
        let (outcome, note) = stop_note(&missing_outside_ci(), Outcome::Vacuous).expect("a stop");
        assert_eq!(outcome, Outcome::Vacuous);
        assert!(
            note.contains("(a; b)") && !note.contains("fetch-depth"),
            "{note}"
        );
    }

    #[test]
    fn each_other_stop_has_its_outcome_and_words() {
        let unresolved = Base::Unresolved {
            source: BaseSource::Key,
            name: "nope".to_string(),
        };
        // A base that was never tried fails whatever the diff says.
        for diff in [Outcome::Passed, Outcome::Failed, Outcome::Vacuous] {
            let (outcome, note) = stop_note(&unresolved, diff).expect("a stop");
            assert_eq!(outcome, Outcome::Failed);
            assert!(note.starts_with("gates.base_ref names `nope`"), "{note}");
        }
        let (outcome, note) = stop_note(&found(), Outcome::Vacuous).expect("a stop");
        assert_eq!(outcome, Outcome::Vacuous);
        assert!(
            note.starts_with("the diff from `git merge-base origin/dev HEAD` is empty"),
            "{note}"
        );
        let (outcome, note) = stop_note(&found(), Outcome::Failed).expect("a stop");
        assert_eq!(outcome, Outcome::Failed);
        assert!(
            note.contains("`origin/dev`") && note.contains("fetch-depth: 0"),
            "{note}"
        );
        let (outcome, note) = stop_note(&missing_in_ci(), Outcome::Failed).expect("a stop");
        assert_eq!(outcome, Outcome::Failed);
        assert!(
            note.contains("never waived") && !note.contains("fetch-depth"),
            "{note}"
        );
    }

    #[test]
    fn a_passed_diff_does_not_stop_the_layer() {
        for base in [found(), missing_in_ci(), missing_outside_ci()] {
            assert_eq!(stop_note(&base, Outcome::Passed), None);
        }
    }
}
