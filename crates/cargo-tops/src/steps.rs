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
#![allow(dead_code, reason = "main.rs wires these items")]

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

const DIFF_NAME: &str = "rust-tops.diff";

/// What a step is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// [`tool_binary`] must be on `PATH`. A miss does not pass the layer.
    Tool,
    /// Run `argv`. A non-zero status is [`Outcome::Failed`].
    Command,
    /// `test -s`: the artifact exists and is non-empty.
    Artifact,
}

/// One argv invocation in a layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub argv: Vec<String>,
    pub kind: Kind,
}

impl Step {
    #[must_use]
    pub fn new(kind: Kind, argv: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            kind,
            argv: argv.into_iter().map(Into::into).collect(),
        }
    }

    #[must_use]
    pub fn tool(argv: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::new(Kind::Tool, argv)
    }

    #[must_use]
    pub fn command(argv: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::new(Kind::Command, argv)
    }

    /// `test -s <path>`.
    #[must_use]
    pub fn artifact(path: impl Into<String>) -> Self {
        let path = path.into();
        Self::new(Kind::Artifact, ["test", "-s", path.as_str()])
    }

    /// Program and its arguments. Empty argv is `None`.
    #[must_use]
    pub fn program(&self) -> Option<(&str, &[String])> {
        let (program, args) = self.argv.split_first()?;
        Some((program.as_str(), args))
    }

    /// Path a [`Kind::Artifact`] step checks.
    #[must_use]
    pub fn artifact_path(&self) -> Option<&Path> {
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
    pub fn display(&self) -> String {
        self.argv.join(" ")
    }
}

/// How a step or a mutation diff finished.
///
/// [`Failed`](Self::Failed) and [`Vacuous`](Self::Vacuous) are different.
/// Vacuous counts for no law.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed,
    ToolMissing,
    /// The diff command succeeded and the diff is empty.
    Vacuous,
}

impl Outcome {
    /// `Some(true)` pass, `Some(false)` fail, `None` when no law may count it.
    #[must_use]
    pub fn law_verdict(self) -> Option<bool> {
        match self {
            Self::Passed => Some(true),
            Self::Failed | Self::ToolMissing => Some(false),
            Self::Vacuous => None,
        }
    }

    /// Vacuous is not a pass and not a fail, so no law counts it.
    #[must_use]
    pub fn counts_for_a_law(self) -> bool {
        self.law_verdict().is_some()
    }

    /// Gate failure. Vacuous does not fail the gate.
    #[must_use]
    pub fn fails_the_gate(self) -> bool {
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
pub struct MissingTool {
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
pub fn check_tool(argv: &[String]) -> Result<(), MissingTool> {
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
pub fn tool_binary(argv: &[String]) -> Option<String> {
    let tokens = tokens_from_argv(argv);
    best_binary(&split_chain(&tokens))
}

/// Coverage is `mkdir`, then `cargo llvm-cov`, then `test -s`.
/// The tool step names `cargo-llvm-cov`, not `mkdir`.
#[must_use]
pub fn coverage_steps() -> Vec<Step> {
    vec![
        Step::command(["mkdir", "-p", "target/coverage"]),
        Step::tool([
            "cargo",
            "llvm-cov",
            "nextest",
            "--workspace",
            "--all-features",
            "--lcov",
            "--output-path",
            "target/coverage/lcov.info",
        ]),
        Step::artifact("target/coverage/lcov.info"),
    ]
}

/// `test -s` is true when the path exists and its size is greater than zero.
/// An empty artifact is [`Outcome::Failed`], not [`Outcome::Vacuous`].
#[must_use]
pub fn artifact_outcome(path: &Path) -> Outcome {
    let present = fs::metadata(path).is_ok_and(|meta| meta.len() > 0);
    if present {
        Outcome::Passed
    } else {
        Outcome::Failed
    }
}

/// A failed or vacuous outcome stops the `&&` chain. Only a pass continues.
#[must_use]
pub fn chain_continues(outcome: Outcome) -> bool {
    outcome == Outcome::Passed
}

/// One layer id and the date the exemption stops.
///
/// Not a list of layers. [`init_exemptions`] is what `init` writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exemption {
    pub layer: String,
    pub expires: String,
}

/// Why [`Exemption::new`] refused a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExemptionError {
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
    pub fn new(layer: impl AsRef<str>, expires: impl AsRef<str>) -> Result<Self, ExemptionError> {
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
pub fn init_exemptions() -> &'static [Exemption] {
    &[]
}

/// Classify a mutation diff. A failed command is [`Outcome::Failed`] even
/// when the diff is empty. There is no waiver parameter and no config key
/// is read. A successful empty diff is [`Outcome::Vacuous`].
#[must_use]
pub fn diff_outcome(command_ok: bool, empty: bool) -> Outcome {
    if !command_ok {
        return Outcome::Failed;
    }
    if empty {
        Outcome::Vacuous
    } else {
        Outcome::Passed
    }
}

/// Read the diff file only after the command succeeded.
///
/// A failed command is [`Outcome::Failed`] even if a stale diff is sitting
/// at `diff_path`. A successful empty file is [`Outcome::Vacuous`].
#[must_use]
pub fn judge_diff(command_ok: bool, diff_path: &Path) -> Outcome {
    if !command_ok {
        return diff_outcome(false, false);
    }
    match fs::metadata(diff_path) {
        Ok(meta) if meta.len() == 0 => diff_outcome(true, true),
        Ok(_) => diff_outcome(true, false),
        Err(_) => diff_outcome(false, false),
    }
}

/// Mutation diff command, then the `cargo mutants` tool step.
///
/// The diff command uses `git merge-base` and `&&`, and writes
/// `<report_dir>/rust-tops.diff`. Run it with the project as the working
/// directory. [`MutationPlan::after_diff`] runs mutants only after a
/// non-empty diff; a failed diff is not waived, and an empty diff is not
/// handed to `cargo mutants`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationPlan {
    pub diff: Step,
    pub diff_path: PathBuf,
    pub mutants: Step,
}

impl MutationPlan {
    #[must_use]
    pub fn new(report_dir: &Path) -> Self {
        let diff_path = report_dir.join(DIFF_NAME);
        let diff_arg = diff_path.display().to_string();
        let script = mutation_diff_script(report_dir, &diff_path);
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
        }
    }

    /// Mutants runs only when the diff passed. Failed and Vacuous stop.
    #[must_use]
    pub fn after_diff(&self, outcome: Outcome) -> Option<&Step> {
        chain_continues(outcome).then_some(&self.mutants)
    }
}

/// Not a git work tree, or no commit: the command fails. No `origin/main`:
/// it writes an empty diff, so the layer is Vacuous. `origin/main` with no
/// merge-base: it fails. Otherwise the diff from the merge-base to `HEAD`.
fn mutation_diff_script(report_dir: &Path, diff_path: &Path) -> String {
    let dir = shell_quote(report_dir);
    let file = shell_quote(diff_path);
    format!(
        "mkdir -p {dir} && git rev-parse --verify --quiet HEAD >/dev/null \
         && if git rev-parse --verify --quiet refs/remotes/origin/main >/dev/null; \
         then base=\"$(git merge-base origin/main HEAD)\" \
         && git diff \"$base\" HEAD --unified=0 > {file}; \
         else : > {file}; fi"
    )
}

fn shell_quote(path: &Path) -> String {
    let text = path.display().to_string().replace('\'', "'\\''");
    format!("'{text}'")
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

fn valid_expiry(text: &str) -> bool {
    let bytes = text.as_bytes();
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
    if argv.len() >= 3 && argv[0] == "sh" && argv[1] == "-c" {
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
        let is_shell = tokens[index] == "sh"
            && tokens.get(index + 1).map(String::as_str) == Some("-c")
            && index + 2 < tokens.len();
        if is_shell {
            out.extend(tokenize(&tokens[index + 2]));
            index += 3;
            changed = true;
        } else {
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
