//! `cargo tops gate` with the `laws` layer, against a `cargo tops init` repo.
//!
//! `bend` is a POSIX sh stub, as in `scripts/test-bend-gate.sh`. It refuses
//! exactly the case constructors the fixture's own `LAWS.bend` maps to
//! `Refuse`, stops at the first one like `bend` does, and otherwise prints
//! one `Allow{}` per law. Cargo and its plugins are stubs that pass and
//! leave the artifacts a real run records. Fresh-init fixtures are git repos
//! with one commit and no remote; the mutation tests add a local `origin`
//! where a case needs one. The
//! `#[ignore]` test at the end
//! swaps the stub for the real pinned Bend; CI runs it in `bend-gate-matrix`.
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Every class `cargo tops init --class` offers.
const INIT_CLASSES: [&str; 5] = [
    "library-core",
    "parser-codec",
    "no-std-embedded",
    "unsafe-kernel",
    "binary-cli",
];

/// Passes, and writes the file after `--output-path` / `--output` the way
/// `cargo llvm-cov` and `cargo crap` do. Logs its argv to `$CARGO_STUB_LOG`.
const CARGO_STUB: &str = r#"#!/bin/sh
if [ -n "${CARGO_STUB_LOG:-}" ]; then
  printf '%s\n' "$*" >> "$CARGO_STUB_LOG"
fi
prev=
for arg in "$@"; do
  case $prev in
    --output-path | --output)
      mkdir -p "$(dirname "$arg")"
      printf 'stub report\n' > "$arg"
      ;;
  esac
  prev=$arg
done
exit 0
"#;

const PLUGINS: [&str; 5] = [
    "cargo-deny",
    "cargo-nextest",
    "cargo-llvm-cov",
    "cargo-crap",
    "cargo-mutants",
];

/// Constructors that `LAWS.bend` claims `Refuse` for, from its one-argument
/// `law` lines.
fn refused_constructors(pack: &str) -> Vec<String> {
    let refused: Vec<String> = pack
        .lines()
        .filter_map(|line| {
            let claim = line
                .trim()
                .strip_prefix('{')?
                .strip_suffix(" == Refuse{} : Verdict}")?;
            let arg = claim.split_once('(')?.1.strip_suffix("{})")?;
            (!arg.contains(',')).then(|| arg.to_string())
        })
        .collect();
    assert!(
        refused.len() >= 40,
        "read only {} refusing laws",
        refused.len()
    );
    refused
}

fn bend_stub(pack: &str) -> String {
    let arms: String = refused_constructors(pack)
        .iter()
        .map(|ctor| {
            format!(
                "    *\"(Laws.{ctor}{{}})) == True{{}}\"*) [ -n \"$refuse\" ] || refuse=$law ;;\n"
            )
        })
        .collect();
    format!(
        r#"#!/bin/sh
if [ -n "${{BEND_STUB_LOG:-}}" ]; then
  printf '%s\n' "$*" >> "$BEND_STUB_LOG"
fi
if [ "$1" = version ]; then
  echo "bend 2.0.36"
  exit 0
fi
n=0
law=
refuse=
while IFS= read -r line || [ -n "$line" ]; do
  case $line in
    "law case_"*)
      n=$((n + 1))
      law=${{line#law case_}}
      law=${{law%:}}
      ;;
{arms}  esac
done < "$1"
if [ -n "$refuse" ]; then
  printf 'SOME PROOFS FAIL\nLocation: case_%s\n' "$refuse"
  exit 1
fi
out=
i=0
while [ "$i" -lt "$n" ]; do
  out="$out${{out:+, }}LAWS.Allow{{}}"
  i=$((i + 1))
done
printf '(%s)\n' "$out"
"#
    )
}

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    bin: PathBuf,
    home: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stub(bin: &Path, name: &str, body: &str) {
    let path = bin.join(name);
    fs::write(&path, body).unwrap_or_else(|err| panic!("write stub {name}: {err}"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|err| panic!("chmod stub {name}: {err}"));
    }
}

/// A fresh `cargo tops init --class <class>` repo, with Cargo and plugin
/// stubs on the fixture's `bin`.
fn init_fixture(name: &str, class: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!("cargo-tops-laws-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let fixture = Fixture {
        repo: root.join("repo"),
        bin: root.join("bin"),
        home: root.join("home"),
        root,
    };
    for dir in [&fixture.bin, &fixture.home] {
        fs::create_dir_all(dir).unwrap_or_else(|err| panic!("mkdir {}: {err}", dir.display()));
    }
    let init = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(["init", "--class", class, "--name", "fixture", "--path"])
        .arg(&fixture.repo)
        .output()
        .expect("spawn cargo-tops init");
    assert!(
        init.status.success(),
        "init: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    stub(&fixture.bin, "cargo", CARGO_STUB);
    for plugin in PLUGINS {
        stub(&fixture.bin, plugin, "#!/bin/sh\nexit 0\n");
    }
    fixture
}

/// A library-core repo with every layer but `static-lint` and `laws` off.
fn fixture(name: &str) -> Fixture {
    fixture_with(name, &["static-lint", "laws"])
}

/// A library-core repo with only the `keep` layers on.
fn fixture_with(name: &str, keep: &[&str]) -> Fixture {
    let fixture = init_fixture(name, "library-core");
    let yaml_path = fixture.repo.join("rust-tops.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read rust-tops.yaml");
    let only: Vec<String> = yaml
        .lines()
        .map(|line| {
            let kept = keep.iter().any(|id| line.contains(&format!("id: {id},")));
            if line.contains("- { id:") && !kept {
                line.replace("enabled: true", "enabled: false")
            } else {
                line.to_string()
            }
        })
        .collect();
    fs::write(&yaml_path, only.join("\n") + "\n").expect("write rust-tops.yaml");
    fixture
}

fn git(fixture: &Fixture, dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("HOME", &fixture.home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Whether the adopter's one commit is pushed to a local `origin`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Pushed,
    None,
}

/// Turn the init repo into an adopter's: a crate with a `Cargo.lock`, `git
/// init`, and one commit of everything. With [`Origin::Pushed`] that commit
/// is `origin/main`. Either way the mutation diff is empty.
fn adopt(fixture: &Fixture, origin: Origin) {
    let repo = &fixture.repo;
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");
    fs::create_dir_all(repo.join("src")).expect("mkdir src");
    fs::write(repo.join("src/lib.rs"), "pub fn one() -> u8 {\n    1\n}\n").expect("write lib.rs");
    fs::write(
        repo.join("Cargo.lock"),
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.lock");
    fs::write(repo.join(".gitignore"), "/target\n").expect("write .gitignore");
    git(fixture, repo, &["init", "-q"]);
    git(fixture, repo, &["add", "-A"]);
    git(fixture, repo, &["commit", "-q", "-m", "adopt rust-tops"]);
    if origin == Origin::Pushed {
        let bare = fixture.root.join("origin.git");
        git(
            fixture,
            &fixture.root,
            &["init", "-q", "--bare", "origin.git"],
        );
        let url = bare.to_string_lossy().into_owned();
        git(fixture, repo, &["remote", "add", "origin", &url]);
        git(fixture, repo, &["push", "-q", "origin", "main"]);
    }
}

/// `origin/main` names a commit with no history in common with `HEAD`, so
/// `git merge-base origin/main HEAD` fails.
fn unrelated_origin(fixture: &Fixture) {
    let out = Command::new("git")
        .args([
            "commit-tree",
            "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
            "-m",
            "unrelated",
        ])
        .current_dir(&fixture.repo)
        .env("HOME", &fixture.home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .expect("spawn git commit-tree");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let orphan = String::from_utf8_lossy(&out.stdout).trim().to_string();
    git(
        fixture,
        &fixture.repo,
        &["update-ref", "refs/remotes/origin/main", &orphan],
    );
}

/// Plant a non-empty diff where a failed run must not read it.
fn stale_diff(fixture: &Fixture) {
    let report = fixture.repo.join("target/rust-tops");
    fs::create_dir_all(&report).expect("mkdir report");
    fs::write(report.join("rust-tops.diff"), "+stale\n").expect("write stale diff");
}

/// A second commit, not pushed: the diff from `origin/main` is not empty.
fn change_lib(fixture: &Fixture) {
    fs::write(
        fixture.repo.join("src/lib.rs"),
        "pub fn one() -> u8 {\n    2 - 1\n}\n",
    )
    .expect("write lib.rs");
    git(
        fixture,
        &fixture.repo,
        &["commit", "-q", "-am", "change one"],
    );
}

fn cargo_log(fixture: &Fixture) -> String {
    fs::read_to_string(fixture.root.join("cargo.log")).unwrap_or_default()
}

/// Whether the cargo stub ran subcommand `sub`.
fn cargo_ran(fixture: &Fixture, sub: &str) -> bool {
    cargo_log(fixture)
        .lines()
        .any(|line| line.split_whitespace().next() == Some(sub))
}

/// `laws evaluated: N/53` from gate output.
fn laws_evaluated(text: &str) -> (usize, usize) {
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix("laws evaluated: "))
        .unwrap_or_else(|| panic!("no `laws evaluated` line: {text}"));
    let ratio = line.split_whitespace().next().expect("N/M");
    let (n, m) = ratio.split_once('/').expect("N/M");
    (n.parse().expect("N"), m.parse().expect("M"))
}

fn stub_bend(fixture: &Fixture) {
    let pack = fs::read_to_string(fixture.repo.join("LAWS.bend")).expect("LAWS.bend");
    stub(&fixture.bin, "bend", &bend_stub(&pack));
}

fn unpin_toolchain(fixture: &Fixture) {
    fs::remove_file(fixture.repo.join("rust-toolchain.toml")).expect("remove rust-toolchain.toml");
}

/// The stub dir, then the inherited `PATH` minus any directory holding a
/// real `bend`.
fn path_without_bend(bin: &Path) -> OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let dirs = std::iter::once(bin.to_path_buf())
        .chain(std::env::split_paths(&inherited).filter(|dir| !dir.join("bend").exists()));
    std::env::join_paths(dirs).expect("join PATH")
}

/// The stub dir, then the inherited `PATH` minus any directory holding a
/// real `bend`, `cargo` or plugin, so a stub removed from `bin` is absent.
fn path_with_only_stubs(bin: &Path) -> OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let real = |dir: &PathBuf| {
        ["bend", "cargo"]
            .iter()
            .chain(PLUGINS.iter())
            .any(|name| dir.join(name).exists())
    };
    let dirs = std::iter::once(bin.to_path_buf())
        .chain(std::env::split_paths(&inherited).filter(|dir| !real(dir)));
    std::env::join_paths(dirs).expect("join PATH")
}

/// The stub dir, then the inherited `PATH` with its real `bend`.
fn path_with_bend(bin: &Path) -> OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let dirs = std::iter::once(bin.to_path_buf()).chain(std::env::split_paths(&inherited));
    std::env::join_paths(dirs).expect("join PATH")
}

fn gate(fixture: &Fixture, path: OsString) -> (Output, String) {
    gate_at(fixture, path, None)
}

fn gate_at(fixture: &Fixture, path: OsString, cargo_target: Option<&Path>) -> (Output, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cargo-tops"));
    cmd.args(["gate", "--path"])
        .arg(&fixture.repo)
        .env("PATH", path)
        .env("HOME", &fixture.home)
        .env("BEND_STUB_LOG", fixture.root.join("bend.log"))
        .env("CARGO_STUB_LOG", fixture.root.join("cargo.log"))
        .env_remove("RUST_TOPS_ROOT");
    match cargo_target {
        Some(dir) => {
            cmd.env("CARGO_TARGET_DIR", dir);
        }
        None => {
            cmd.env_remove("CARGO_TARGET_DIR");
        }
    }
    let out = cmd.output().expect("spawn cargo-tops gate");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out, text)
}

fn cases_json(fixture: &Fixture) -> serde_json::Value {
    let raw =
        fs::read_to_string(fixture.repo.join("target/rust-tops/cases.json")).expect("cases.json");
    serde_json::from_str(&raw).expect("cases.json is json")
}

fn law<'v>(cases: &'v serde_json::Value, name: &str) -> &'v serde_json::Value {
    cases["laws"]
        .as_array()
        .and_then(|laws| laws.iter().find(|entry| entry["law"] == name))
        .unwrap_or_else(|| panic!("{name} missing from cases.json"))
}

/// A fresh init repo for `class`, `git init` plus one commit and no remote,
/// passes `gate` with every layer it enables, and the self-fail laws chose
/// their passing facts. Unmapped layers are skipped, not failed; with no
/// `origin/main` the mutation diff is empty, so the layer is `Vacuous`.
fn expect_fresh_init_passes(class: &str, path: impl Fn(&Path) -> OsString, stubbed: bool) {
    let fixture = init_fixture(&format!("fresh-{class}-{stubbed}"), class);
    adopt(&fixture, Origin::None);
    if stubbed {
        stub_bend(&fixture);
    }
    let (out, text) = gate(&fixture, path(&fixture.bin));
    assert_eq!(out.status.code(), Some(0), "{class}: {text}");
    assert!(
        text.contains("skip acceptance: no local command mapped"),
        "{class}: {text}"
    );
    assert!(text.contains("Vacuous: the diff from"), "{class}: {text}");
    assert!(
        text.contains("1 vacuous (counted for no law)"),
        "{class}: {text}"
    );
    assert!(
        !cargo_ran(&fixture, "mutants"),
        "{class}: cargo mutants ran on an empty diff"
    );
    let cases = cases_json(&fixture);
    assert_eq!(cases["passed"], true, "{class}");
    let checked = cases["laws"]
        .as_array()
        .expect("laws")
        .iter()
        .filter(|law| law["status"] == "checked")
        .count();
    assert_eq!(laws_evaluated(&text), (checked, 53), "{class}: {text}");
    assert!(checked > 0, "{class}");
    assert_eq!(
        law(&cases, "baseline_green_before_mutants")["status"],
        "not-evaluated",
        "{class}: a vacuous mutation run counts for no law"
    );
    for (name, case) in [
        ("clippy_toolchain_pinned", "ClippyPinned"),
        ("ci_matrix_matches_class", "ClassJobPresent"),
        ("drop_in_is_a_floor_not_a_ceiling", "DropInFloor"),
        ("record_only_artifacts_are_nonempty", "RecordNonempty"),
        ("crap_wires_coverage", "CrapWired"),
        ("lock_honesty", "LockHonest"),
        ("missing_tool_not_green", "ToolPresent"),
    ] {
        let entry = law(&cases, name);
        assert_eq!(entry["case"], case, "{class} {name}: {entry}");
        assert!(
            matches!(entry["verdict"].as_str(), Some("Allow" | "Ack")),
            "{class} {name}: {entry}"
        );
    }
    assert_eq!(
        law(&cases, "evidence_not_self_served")["status"],
        "not-evaluated"
    );
}

#[test]
fn init_writes_the_laws_layer_the_bend_kit_and_the_toolchain_pin() {
    let fixture = fixture("init");
    let yaml = fs::read_to_string(fixture.repo.join("rust-tops.yaml")).expect("yaml");
    assert!(yaml.contains("version: \"1.1.0\""));
    assert!(yaml.contains("msrv: \"1.88\""));
    assert!(yaml.contains("{ id: laws,              enabled: true,  tier: 0, runner: bend,"));
    let doc: serde_json::Value = serde_saphyr::from_str(&yaml).expect("yaml parses");
    assert_eq!(
        doc["exemptions"],
        serde_json::json!([]),
        "init wrote an exemption"
    );
    for rel in ["LAWS.bend", "laws/bend.lock", "scripts/fetch-bend.sh"] {
        assert!(fixture.repo.join(rel).is_file(), "init did not write {rel}");
    }
    let toolchain: toml::Table = fs::read_to_string(fixture.repo.join("rust-toolchain.toml"))
        .expect("rust-toolchain.toml")
        .parse()
        .expect("toml");
    assert_eq!(toolchain["toolchain"]["channel"].as_str(), Some("1.88"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(fixture.repo.join("scripts/fetch-bend.sh"))
            .expect("stat")
            .permissions()
            .mode();
        assert_ne!(mode & 0o111, 0, "fetch-bend.sh is not executable");
    }
}

#[test]
fn the_stub_refuses_what_the_pack_refuses() {
    let refused = refused_constructors(include_str!("../templates/LAWS.bend"));
    for ctor in ["FromImpl", "ClippyUnpinned", "ClassJobMissing", "LockDrift"] {
        assert!(refused.iter().any(|r| r == ctor), "{ctor}");
    }
    for ctor in ["WriteOnlyField", "ClippyPinned", "ClassJobPresent"] {
        assert!(!refused.iter().any(|r| r == ctor), "{ctor} is not a Refuse");
    }
}

#[test]
fn fresh_init_then_gate_passes_for_every_class() {
    for class in INIT_CLASSES {
        expect_fresh_init_passes(class, path_without_bend, true);
    }
}

#[test]
fn a_refusing_case_fails_the_gate() {
    let fixture = fixture("refuse");
    unpin_toolchain(&fixture);
    stub_bend(&fixture);
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("laws: bend refused clippy_toolchain_pinned"),
        "{text}"
    );
    let program =
        fs::read_to_string(fixture.repo.join("target/rust-tops/cases.bend")).expect("cases.bend");
    assert!(program.contains("Laws.clippy_pin_verdict(Laws.ClippyUnpinned{})"));
    let cases = cases_json(&fixture);
    assert_eq!(cases["passed"], false);
    assert_eq!(law(&cases, "clippy_toolchain_pinned")["verdict"], "Refuse");
    assert_eq!(law(&cases, "single_deny_site")["verdict"], "unreported");
}

#[test]
fn a_non_refusing_case_passes_and_lists_not_evaluated_laws() {
    let fixture = fixture("allow");
    stub_bend(&fixture);
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(0), "{text}");
    let cases = cases_json(&fixture);
    assert_eq!(cases["passed"], true);
    assert_eq!(
        law(&cases, "clippy_toolchain_pinned")["case"],
        "ClippyPinned"
    );
    assert_eq!(law(&cases, "clippy_toolchain_pinned")["verdict"], "Allow");
    let evidence = law(&cases, "evidence_not_self_served");
    assert_eq!(evidence["status"], "not-evaluated");
    assert_eq!(evidence["verdict"], serde_json::Value::Null);
    assert_eq!(evidence["reason_code"], "no_fact_reader");
    assert_eq!(
        law(&cases, "perf_without_runner_is_not_gate")["reason_code"],
        "run_dependent"
    );
    assert_eq!(
        law(&cases, "clippy_toolchain_pinned")["reason_code"],
        serde_json::Value::Null
    );
    let program =
        fs::read_to_string(fixture.repo.join("target/rust-tops/cases.bend")).expect("cases.bend");
    assert!(
        !program.contains("evidence_verdict"),
        "a not-evaluated law reached the program"
    );
    assert!(
        text.contains("not-evaluated (not checked, not passed)"),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|line| line.trim_start().starts_with("not-evaluated")
                && line.contains("evidence_not_self_served")),
        "{text}"
    );
}

#[test]
fn a_failed_fetch_fails_the_gate() {
    let fixture = fixture("fetch");
    stub(&fixture.bin, "curl", "#!/bin/sh\nexit 22\n");
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains(
            "sh scripts/fetch-bend.sh (Bend 2.0.36 from laws/bend.lock is not installed)"
        ),
        "{text}"
    );
    assert!(
        text.contains("laws: scripts/fetch-bend.sh failed"),
        "{text}"
    );
    assert!(
        !text.contains("==> [static-lint]"),
        "layers ran before the judge was installed: {text}"
    );
    assert!(
        !fixture.home.join(".bend").exists(),
        "a failed fetch left $HOME/.bend"
    );
}

#[test]
fn a_disabled_laws_layer_never_calls_bend() {
    let fixture = fixture("off");
    let yaml_path = fixture.repo.join("rust-tops.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("yaml");
    let off = yaml.replace(
        "{ id: laws,              enabled: true,",
        "{ id: laws,              enabled: false,",
    );
    assert_ne!(off, yaml);
    fs::write(&yaml_path, off).expect("write yaml");
    stub_bend(&fixture);
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("laws evaluated: 0/53 (laws layer off)"),
        "{text}"
    );
    assert!(
        !fixture.root.join("bend.log").exists(),
        "bend was called with laws off"
    );
    assert!(!fixture.repo.join("target/rust-tops").exists());
}

#[test]
fn laws_report_follows_cargo_target_dir() {
    let fixture = fixture("target-dir");
    stub_bend(&fixture);
    let custom = fixture.root.join("custom-target");
    let (out, text) = gate_at(&fixture, path_without_bend(&fixture.bin), Some(&custom));
    assert_eq!(out.status.code(), Some(0), "{text}");
    let cases = custom.join("rust-tops/cases.json");
    assert!(
        cases.is_file(),
        "cases.json was not written under CARGO_TARGET_DIR: {text}"
    );
    assert!(!fixture.repo.join("target/rust-tops").exists());
    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(cases).expect("read")).expect("json");
    assert_eq!(parsed["passed"], true);
}

#[test]
fn a_missing_coverage_tool_is_not_on_path_and_does_not_pass() {
    let fixture = fixture_with("no-llvm-cov", &["static-lint", "coverage", "laws"]);
    adopt(&fixture, Origin::Pushed);
    stub_bend(&fixture);
    fs::remove_file(fixture.bin.join("cargo-llvm-cov")).expect("remove stub");
    let (out, text) = gate(&fixture, path_with_only_stubs(&fixture.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("coverage: `cargo-llvm-cov` not on PATH"),
        "{text}"
    );
    assert!(
        !text.contains("`mkdir`"),
        "mkdir was taken for the tool: {text}"
    );
    assert!(
        !cargo_ran(&fixture, "llvm-cov"),
        "cargo llvm-cov ran without cargo-llvm-cov: {}",
        cargo_log(&fixture)
    );
}

#[test]
fn a_missing_mutants_tool_is_not_on_path_even_when_the_diff_is_empty() {
    let fixture = fixture_with("no-mutants", &["static-lint", "mutation", "laws"]);
    adopt(&fixture, Origin::Pushed);
    stub_bend(&fixture);
    fs::remove_file(fixture.bin.join("cargo-mutants")).expect("remove stub");
    let (out, text) = gate(&fixture, path_with_only_stubs(&fixture.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("mutation: `cargo-mutants` not on PATH"),
        "{text}"
    );
    assert!(!text.contains("`sh`"), "sh was taken for the tool: {text}");
    assert!(!text.contains("Vacuous"), "{text}");
}

#[test]
fn a_failed_diff_is_failed_with_no_escape() {
    // Not a git work tree. A stale, non-empty diff from an earlier run must
    // not rescue it.
    let plain = fixture_with("diff-no-git", &["static-lint", "mutation", "laws"]);
    stub_bend(&plain);
    stale_diff(&plain);
    let (out, text) = gate(&plain, path_without_bend(&plain.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("a failed diff is never waived"), "{text}");
    assert!(text.contains("mutation: `sh -c mkdir -p"), "{text}");
    assert!(!text.contains("Vacuous"), "{text}");
    assert!(
        !cargo_ran(&plain, "mutants"),
        "cargo mutants ran after a failed diff"
    );

    // `origin/main` exists, but `git merge-base origin/main HEAD` fails.
    let unrelated = fixture_with("diff-unrelated", &["static-lint", "mutation", "laws"]);
    adopt(&unrelated, Origin::None);
    unrelated_origin(&unrelated);
    stub_bend(&unrelated);
    stale_diff(&unrelated);
    let (out, text) = gate(&unrelated, path_without_bend(&unrelated.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("a failed diff is never waived"), "{text}");
    assert!(!text.contains("Vacuous"), "{text}");
    assert!(
        !cargo_ran(&unrelated, "mutants"),
        "cargo mutants ran after a failed merge-base"
    );
}

#[test]
fn no_origin_main_is_an_empty_diff_and_vacuous() {
    // `git init` plus one commit and no remote. A stale diff is replaced by
    // the empty one, not read.
    let fixture = fixture_with("diff-no-origin", &["static-lint", "mutation", "laws"]);
    adopt(&fixture, Origin::None);
    stub_bend(&fixture);
    stale_diff(&fixture);
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("Vacuous: the diff from"), "{text}");
    assert!(!text.contains("a failed diff is never waived"), "{text}");
    assert!(
        !cargo_ran(&fixture, "mutants"),
        "cargo mutants ran with no origin/main"
    );
    let diff = fixture.repo.join("target/rust-tops/rust-tops.diff");
    assert_eq!(fs::metadata(&diff).map(|m| m.len()).ok(), Some(0));
    let cases = cases_json(&fixture);
    assert_eq!(
        law(&cases, "baseline_green_before_mutants")["status"],
        "not-evaluated"
    );
}

#[test]
fn an_empty_diff_is_vacuous_and_counts_for_no_law() {
    let fixture = fixture_with("diff-empty", &["static-lint", "mutation", "laws"]);
    adopt(&fixture, Origin::Pushed);
    stub_bend(&fixture);
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("Vacuous: the diff from"), "{text}");
    assert!(
        text.contains("ok: 3 command(s) passed, 1 vacuous (counted for no law)"),
        "{text}"
    );
    assert!(
        !cargo_ran(&fixture, "mutants"),
        "cargo mutants ran on an empty diff"
    );
    let cases = cases_json(&fixture);
    let baseline = law(&cases, "baseline_green_before_mutants");
    assert_eq!(baseline["status"], "not-evaluated", "{baseline}");
    assert_eq!(baseline["reason_code"], "run_dependent", "{baseline}");
    let diff = fixture.repo.join("target/rust-tops/rust-tops.diff");
    assert_eq!(fs::metadata(&diff).map(|m| m.len()).ok(), Some(0));
}

#[test]
fn a_real_diff_runs_mutants_on_the_report_dir_diff() {
    let fixture = fixture_with("diff-real", &["static-lint", "mutation", "laws"]);
    adopt(&fixture, Origin::Pushed);
    change_lib(&fixture);
    stub_bend(&fixture);
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(0), "{text}");
    let diff = fixture.repo.join("target/rust-tops/rust-tops.diff");
    let body = fs::read_to_string(&diff).expect("diff under the report dir");
    assert!(body.contains("src/lib.rs"), "{body}");
    let log = cargo_log(&fixture);
    let want = format!("mutants --in-diff {} -j 2", diff.display());
    assert!(log.lines().any(|line| line == want), "{log}");
    assert!(!log.contains("/tmp/rust-tops.diff"), "{log}");
    assert!(!text.contains("/tmp/rust-tops.diff"), "{text}");
    let cases = cases_json(&fixture);
    assert_eq!(
        law(&cases, "baseline_green_before_mutants")["case"],
        "BaselineGreen"
    );
    assert_eq!(law(&cases, "missing_tool_not_green")["case"], "ToolPresent");
}

/// Real Bend 2.0.36 on `PATH`: a refusing repo fails, and a fresh init of
/// every class passes.
#[test]
#[ignore = "needs the pinned Bend 2.0.36 on PATH; CI runs it in bend-gate-matrix"]
fn the_pinned_bend_judges_the_case_file() {
    let version = Command::new("bend")
        .arg("version")
        .output()
        .expect("bend on PATH");
    assert!(
        String::from_utf8_lossy(&version.stdout).starts_with("bend 2.0.36"),
        "need Bend 2.0.36 on PATH"
    );

    let refused = fixture("real-refuse");
    unpin_toolchain(&refused);
    let (out, text) = gate(&refused, path_with_bend(&refused.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("laws: bend refused clippy_toolchain_pinned"),
        "{text}"
    );
    assert!(text.contains("SOME PROOFS FAIL"), "{text}");

    for class in INIT_CLASSES {
        expect_fresh_init_passes(class, path_with_bend, false);
    }
}
