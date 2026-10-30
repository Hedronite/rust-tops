//! `cargo tops gate` with the `laws` layer, against a `cargo tops init` repo.
//! `init` writes `laws` off (D3); the fixtures that judge laws turn it on.
//!
//! `bend` is a POSIX sh stub, as in `scripts/test-bend-gate.sh`. It refuses
//! exactly the case constructors the fixture's own `LAWS.bend` maps to
//! `Refuse`, stops at the first one like `bend` does, and otherwise prints
//! one `Allow{}` per law. Given a file with no `law` (the gate's `main.bend`
//! after a failed proof), it prints one verdict per call in `main`. Cargo and its plugins are stubs that pass and
//! leave the artifacts a real run records. Fresh-init fixtures are git repos
//! with one commit and no remote; the mutation tests add a local `origin`
//! where a case needs one. The
//! `#[ignore]` test at the end
//! swaps the stub for the real pinned Bend; CI runs it in `bend-gate-matrix`.
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Every class `cargo tops init --class` offers: one per file in `profiles/`.
const INIT_CLASSES: [&str; 8] = [
    "library-core",
    "parser-codec",
    "no-std-embedded",
    "unsafe-kernel",
    "binary-cli",
    "library-util",
    "ffi-bridge",
    "workspace-root",
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

const PLUGINS: [&str; 6] = [
    "cargo-deny",
    "cargo-nextest",
    "cargo-llvm-cov",
    "cargo-crap",
    "cargo-mutants",
    "cargo-kani",
];

/// Enabled layers `init` leaves above the default `--max-tier 1`.
const ABOVE_TIER_1: &str = "above --max-tier 1, not judged: fuzz, miri, sanitizer, formal-kani, bench-criterion, bench-callgrind, density-audit";

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
    let refused = refused_constructors(pack);
    let list = refused.join(" ");
    let arms: String = refused
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
if [ "$n" -eq 0 ]; then
  refused="{list}"
  out=
  for call in $(grep -o 'Laws\.[A-Za-z0-9_]*(Laws\.[A-Za-z0-9_]*{{}})' "$1"); do
    ctor=${{call#*(Laws.}}
    ctor=${{ctor%%\{{*}}
    case " $refused " in
      *" $ctor "*) out="$out${{out:+, }}LAWS.Refuse{{}}" ;;
      *) out="$out${{out:+, }}LAWS.Allow{{}}" ;;
    esac
  done
  printf '(%s)\n' "$out"
  exit 0
fi
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
    // No test fetches Bend from the network: a gate that falls through to
    // `fetch-bend.sh` fails at once instead of downloading.
    stub(&fixture.bin, "curl", "#!/bin/sh\nexit 22\n");
    fixture
}

/// A library-core repo with every layer but `static-lint` and `laws` off.
fn fixture(name: &str) -> Fixture {
    fixture_with(name, &["static-lint", "laws"])
}

/// A library-core repo with only the `keep` layers on, whatever `init`
/// wrote for them.
fn fixture_with(name: &str, keep: &[&str]) -> Fixture {
    let fixture = init_fixture(name, "library-core");
    edit_yaml(&fixture, |doc| {
        for layer in doc["layers"].as_array_mut().expect("layers") {
            let id = layer["id"].as_str().expect("layer id").to_string();
            layer["enabled"] = serde_json::Value::Bool(keep.contains(&id.as_str()));
        }
    });
    fixture
}

/// Rewrites the fixture's `rust-tops.yaml` through `edit`, as data. It is
/// written back as JSON, which is YAML.
fn edit_yaml(fixture: &Fixture, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = fixture.repo.join("rust-tops.yaml");
    let raw = fs::read_to_string(&path).expect("read rust-tops.yaml");
    let mut doc: serde_json::Value = serde_saphyr::from_str(&raw).expect("rust-tops.yaml parses");
    edit(&mut doc);
    let json = serde_json::to_string_pretty(&doc).expect("json");
    fs::write(&path, json + "\n").expect("write rust-tops.yaml");
}

/// Enable layer `id` as `init` wrote it.
fn turn_on(fixture: &Fixture, id: &str) {
    edit_yaml(fixture, |doc| {
        for layer in doc["layers"].as_array_mut().expect("layers") {
            if layer["id"] == id {
                layer["enabled"] = serde_json::Value::Bool(true);
            }
        }
    });
}

fn yaml_doc(fixture: &Fixture) -> serde_json::Value {
    let raw = fs::read_to_string(fixture.repo.join("rust-tops.yaml")).expect("yaml");
    serde_saphyr::from_str(&raw).expect("yaml parses")
}

/// The line the gate prints for the layers the yaml turns off, in yaml order.
fn disabled_line(fixture: &Fixture) -> String {
    let off: Vec<String> = yaml_doc(fixture)["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .filter(|layer| layer["enabled"] == false)
        .map(|layer| layer["id"].as_str().expect("layer id").to_string())
        .collect();
    format!("\nnot run (enabled: false): {}\n", off.join(", "))
}

fn layer<'a>(doc: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    doc["layers"]
        .as_array()
        .and_then(|layers| layers.iter().find(|layer| layer["id"] == id))
        .unwrap_or_else(|| panic!("no {id} layer"))
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

/// The test runs the cargo stub logged, in order: `nextest run ...` and
/// `llvm-cov nextest ...`.
fn nextest_runs(fixture: &Fixture) -> Vec<String> {
    cargo_log(fixture)
        .lines()
        .filter(|line| {
            let mut words = line.split_whitespace();
            match words.next() {
                Some("nextest") => true,
                Some("llvm-cov") => words.next() == Some("nextest"),
                _ => false,
            }
        })
        .map(str::to_string)
        .collect()
}

/// The cargo stub, except that it exits `code` (after logging) when its
/// argv matches the shell `case` pattern `pattern`.
fn stub_cargo_failing(fixture: &Fixture, pattern: &str, code: u8) {
    let body = CARGO_STUB.replacen(
        "#!/bin/sh\n",
        &format!(
            "#!/bin/sh\ncase \"$*\" in {pattern})\n  printf '%s\\n' \"$*\" >> \"$CARGO_STUB_LOG\"\n  exit {code}\n  ;;\nesac\n"
        ),
        1,
    );
    stub(&fixture.bin, "cargo", &body);
}

fn set_tier(fixture: &Fixture, id: &str, tier: u8) {
    edit_yaml(fixture, |doc| {
        for layer in doc["layers"].as_array_mut().expect("layers") {
            if layer["id"] == id {
                layer["tier"] = serde_json::Value::from(tier);
            }
        }
    });
}

/// The layers that run the test suite.
const TEST_LAYERS: [&str; 4] = ["unit", "integration", "coverage", "property"];
/// What the cargo stub logs for each test command the gate runs.
const UNIT_RUN: &str = "nextest run --workspace --all-features --profile ci";
const COVERAGE_RUN: &str = "llvm-cov nextest --workspace --all-features --profile ci --lcov --output-path target/coverage/lcov.info";
const PROPERTY_RUN: &str =
    "nextest run --workspace --all-features --profile ci --no-tests=fail property";

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

/// `(layers that ran a command, enabled layers in range)` from the gate's
/// `count: N command(s) ran; K of M enabled layer(s) ...` line.
fn count(text: &str) -> (usize, usize) {
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix("count: "))
        .unwrap_or_else(|| panic!("no `count` line: {text}"));
    let (_, layers) = line
        .split_once("; ")
        .expect("count: N command(s) ran; K of M");
    let mut words = layers.split_whitespace();
    let ran = words.next().expect("K").parse().expect("K");
    assert_eq!(words.next(), Some("of"), "{line}");
    let of = words.next().expect("M").parse().expect("M");
    (ran, of)
}

/// The proof refuses `clippy_toolchain_pinned` and stops there, as `bend`
/// does.
const PROOF_REFUSES_CLIPPY: &str = r#"  printf 'SOME PROOFS FAIL\nLocation: case_clippy_toolchain_pinned\n'
  exit 1"#;

/// One `Allow{}` per verdict call in the file.
const PRINT_ALL_ALLOW: &str = r#"out=
for call in $(grep -o 'Laws\.[A-Za-z0-9_]*(Laws\.[A-Za-z0-9_]*{})' "$1"); do
  out="$out${out:+, }LAWS.Allow{}"
done
printf '(%s)\n' "$out""#;

/// A `bend` that answers `cases.bend` with the sh lines `proof` and any
/// other file (the gate's `main.bend`) with `print`. It logs every run.
fn stub_bend_runs(fixture: &Fixture, proof: &str, print: &str) {
    let body = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$BEND_STUB_LOG\"\nif [ \"$1\" = version ]; then echo \"bend 2.0.36\"; exit 0; fi\nif [ \"$1\" = cases.bend ]; then\n{proof}\nfi\n{print}\n"
    );
    stub(&fixture.bin, "bend", &body);
}

/// The file each `bend` run judged, in order, from the stub's log.
fn bend_runs(fixture: &Fixture) -> Vec<String> {
    fs::read_to_string(fixture.root.join("bend.log"))
        .unwrap_or_default()
        .lines()
        .filter(|line| *line != "version")
        .map(str::to_string)
        .collect()
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
        ["bend", "cargo", "kani"]
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
    gate_with(fixture, path, cargo_target, &[])
}

/// `gate --path <repo>` plus `args`, such as `--max-tier 4`.
fn gate_with(
    fixture: &Fixture,
    path: OsString,
    cargo_target: Option<&Path>,
    args: &[&str],
) -> (Output, String) {
    let mut cmd = gate_command(fixture, path, cargo_target);
    cmd.args(args);
    run_gate(cmd)
}

/// The gate command every test runs. The stub `bend` is on `PATH`, which the
/// gate reads only with `CARGO_TOPS_BEND_PATH=1`, so that is set.
fn gate_command(fixture: &Fixture, path: OsString, cargo_target: Option<&Path>) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cargo-tops"));
    cmd.args(["gate", "--path"])
        .arg(&fixture.repo)
        .env("PATH", path)
        .env("HOME", &fixture.home)
        .env("CARGO_TOPS_BEND_PATH", "1")
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
    cmd
}

fn run_gate(mut cmd: Command) -> (Output, String) {
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

/// What every fresh-init gate run says, laws on or off: nothing NotRun,
/// `acceptance` listed as off and never run, nothing above tier 1 run, the
/// mutation layer `Vacuous` with no `origin/main`, and a count that is not
/// short.
fn expect_fresh_init_outcomes(class: &str, fixture: &Fixture, text: &str) {
    assert!(!text.contains("NotRun"), "{class}: {text}");
    assert!(!text.contains("[acceptance]"), "{class}: {text}");
    let off = disabled_line(fixture);
    assert!(off.contains(": acceptance, "), "{class}: {off}");
    assert!(text.contains(&off), "{class}: {text}");
    assert!(text.contains(ABOVE_TIER_1), "{class}: {text}");
    assert!(
        !cargo_ran(fixture, "kani"),
        "{class}: cargo kani ran at --max-tier 1"
    );
    assert!(
        text.contains("Vacuous: no origin/main; the mutation diff is empty"),
        "{class}: {text}"
    );
    assert!(!text.contains("Vacuous: the diff from"), "{class}: {text}");
    assert!(
        text.contains("1 vacuous (counted for no law)"),
        "{class}: {text}"
    );
    assert!(
        !cargo_ran(fixture, "mutants"),
        "{class}: cargo mutants ran on an empty diff"
    );
    let (ran, of) = count(text);
    assert_eq!(ran, of, "{class}: {text}");
    assert!(!text.contains("none ran for"), "{class}: {text}");
    // Coverage is in range, so its run is the one test run.
    assert_eq!(nextest_runs(fixture), [COVERAGE_RUN], "{class}");
    assert!(
        text.contains("; unit, integration, property took the coverage run\n"),
        "{class}: {text}"
    );
}

/// A fresh init repo for `class`, `git init` plus one commit and no remote,
/// passes `gate` as `init` wrote it. `laws` is off, so the gate neither
/// fetches nor calls Bend: `curl` fails and `bend` is on no `PATH` entry.
fn expect_fresh_init_passes(class: &str) {
    let fixture = init_fixture(&format!("fresh-{class}"), class);
    assert_eq!(
        layer(&yaml_doc(&fixture), "laws")["enabled"],
        false,
        "{class}"
    );
    adopt(&fixture, Origin::None);
    stub(&fixture.bin, "curl", "#!/bin/sh\nexit 22\n");
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(0), "{class}: {text}");
    expect_fresh_init_outcomes(class, &fixture, &text);
    assert!(
        text.contains("laws evaluated: 0/53 (laws layer off)"),
        "{class}: {text}"
    );
    assert!(!text.contains("fetch-bend"), "{class}: {text}");
    assert!(!text.contains("==> [laws]"), "{class}: {text}");
    assert!(
        !fixture.home.join(".bend").exists(),
        "{class}: Bend fetched"
    );
    assert!(!fixture.root.join("bend.log").exists(), "{class}: bend ran");
    assert!(
        !fixture.repo.join("target/rust-tops/cases.json").exists(),
        "{class}: laws judged with the layer off"
    );
}

/// A fresh init repo for `class` with `laws` turned on before the one
/// commit passes `gate`, and the self-fail laws chose their passing facts.
fn expect_fresh_init_with_laws_passes(
    class: &str,
    path: impl Fn(&Path) -> OsString,
    stubbed: bool,
) {
    let fixture = init_fixture(&format!("fresh-laws-{class}-{stubbed}"), class);
    turn_on(&fixture, "laws");
    adopt(&fixture, Origin::None);
    if stubbed {
        stub_bend(&fixture);
    }
    let (out, text) = gate(&fixture, path(&fixture.bin));
    assert_eq!(out.status.code(), Some(0), "{class}: {text}");
    expect_fresh_init_outcomes(class, &fixture, &text);
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
fn init_writes_the_laws_layer_off_the_bend_kit_and_the_toolchain_pin() {
    let fixture = fixture("init");
    let fresh = init_fixture("init-fresh", "library-core");
    let doc = yaml_doc(&fresh);
    let _ = fs::remove_dir_all(&fresh.root);
    assert_eq!(doc["protocol"]["version"], "1.1.0");
    assert_eq!(doc["crate"]["msrv"], "1.88");
    let laws = layer(&doc, "laws");
    assert_eq!(
        (&laws["enabled"], &laws["tier"], &laws["runner"]),
        (
            &serde_json::json!(false),
            &serde_json::json!(0),
            &serde_json::json!("bend")
        ),
        "init must write laws off (D3)"
    );
    let acceptance = layer(&doc, "acceptance");
    assert_eq!(
        (&acceptance["enabled"], &acceptance["tier"]),
        (&serde_json::json!(false), &serde_json::json!(1))
    );
    let kani = layer(&doc, "formal-kani");
    assert_eq!(
        (&kani["tier"], &kani["runner"]),
        (&serde_json::json!(4), &serde_json::json!("cargo-kani")),
        "formal-kani left tier 4"
    );
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
        expect_fresh_init_passes(class);
    }
}

#[test]
fn fresh_init_with_laws_on_then_gate_passes_for_every_class() {
    for class in INIT_CLASSES {
        expect_fresh_init_with_laws_passes(class, path_without_bend, true);
    }
}

#[test]
fn a_dry_run_runs_nothing_and_never_passes() {
    for class in INIT_CLASSES {
        let fixture = init_fixture(&format!("dry-{class}"), class);
        adopt(&fixture, Origin::None);
        let (out, text) = gate_with(
            &fixture,
            path_with_only_stubs(&fixture.bin),
            None,
            &["--dry-run"],
        );
        assert_eq!(out.status.code(), Some(1), "{class}: {text}");
        let (ran, of) = count(&text);
        assert_eq!(ran, 0, "{class}: {text}");
        assert!(of > 0, "{class}: {text}");
        assert!(
            text.contains("count: 0 command(s) ran; 0 of"),
            "{class}: {text}"
        );
        assert!(
            text.contains("none ran for: static-lint"),
            "{class}: {text}"
        );
        assert!(text.contains("a dry run is not a pass"), "{class}: {text}");
        assert!(!text.contains("\nok: "), "{class}: {text}");
        // `cargo metadata` finds the report dir; no layer's command ran.
        assert!(
            cargo_log(&fixture)
                .lines()
                .all(|line| line.starts_with("metadata ")),
            "{class}: a dry run ran a layer: {}",
            cargo_log(&fixture)
        );

        // The same repo, run for real: every enabled layer ran a command.
        let (out, text) = gate(&fixture, path_with_only_stubs(&fixture.bin));
        assert_eq!(out.status.code(), Some(0), "{class}: {text}");
        assert_eq!(count(&text), (of, of), "{class}: {text}");
    }
}

#[test]
fn a_layer_that_runs_no_command_makes_the_count_short() {
    // The cargo-llvm-cov stub is gone, so `coverage` runs no command.
    let fixture = fixture_with("short", &["static-lint", "coverage"]);
    adopt(&fixture, Origin::Pushed);
    fs::remove_file(fixture.bin.join("cargo-llvm-cov")).expect("remove stub");
    let (out, text) = gate(&fixture, path_with_only_stubs(&fixture.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("  - count: 3 command(s) ran; 1 of 2 enabled layer(s) at tier <= 1 ran one; none ran for: coverage"),
        "{text}"
    );
    assert!(!text.contains("\nok: "), "{text}");
}

#[test]
fn the_bend_run_is_the_laws_layer_s_command() {
    // Only `laws` is on. Bend ran, so the count is whole; the gate fails on
    // the law that wants static-lint, not on the count.
    let fixture = fixture_with("laws-only", &["laws"]);
    adopt(&fixture, Origin::None);
    stub_bend(&fixture);
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("count: 1 command(s) ran; 1 of 1 enabled layer(s) at tier <= 1 ran one\n"),
        "{text}"
    );
    assert!(!text.contains("none ran for"), "{text}");
    assert!(
        text.contains("  - laws: bend refused fmt_is_tier0"),
        "{text}"
    );
    assert!(text.contains("fail: 1 problem(s)"), "{text}");
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
    assert_eq!(law(&cases, "single_deny_site")["verdict"], "Allow");
    assert_eq!(bend_runs(&fixture), ["cases.bend", "main.bend"]);
}

#[test]
fn a_failed_proof_prints_every_refusal() {
    // Two facts refuse: the toolchain is unpinned (clippy_toolchain_pinned)
    // and static-lint is off (fmt_is_tier0). The proof stops at the first;
    // the main.bend run names every one.
    let fixture = fixture_with("refuse-all", &["laws"]);
    unpin_toolchain(&fixture);
    stub_bend(&fixture);
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert_eq!(bend_runs(&fixture), ["cases.bend", "main.bend"]);
    let cases = cases_json(&fixture);
    assert_eq!(cases["passed"], false);
    let checked: Vec<&serde_json::Value> = cases["laws"]
        .as_array()
        .expect("laws")
        .iter()
        .filter(|law| law["status"] == "checked")
        .collect();
    assert!(
        checked.iter().all(|law| law["verdict"] != "unreported"),
        "{cases}"
    );
    let refused: Vec<&str> = checked
        .iter()
        .filter(|law| law["verdict"] == "Refuse")
        .map(|law| law["law"].as_str().expect("law name"))
        .collect();
    assert!(refused.len() >= 2, "{refused:?}");
    assert!(refused.contains(&"clippy_toolchain_pinned"), "{refused:?}");
    assert!(refused.contains(&"fmt_is_tier0"), "{refused:?}");
    let failure = format!("  - laws: bend refused {} (", refused.join(", "));
    assert!(text.contains(&failure), "{failure}: {text}");
    for law in &refused {
        assert!(
            text.lines()
                .any(|line| line.trim_start().starts_with("Refuse") && line.contains(law)),
            "{law}: {text}"
        );
    }
    assert!(
        !text.contains("`unreported` laws were not judged"),
        "{text}"
    );

    // A second run finds the first run's main.bend, a regular file, and
    // writes it again: only a link is refused.
    let (out, again) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(1), "{again}");
    assert!(again.contains(&failure), "{failure}: {again}");
    assert_eq!(
        bend_runs(&fixture),
        ["cases.bend", "main.bend", "cases.bend", "main.bend"]
    );
}

#[test]
fn the_proof_run_decides_pass_or_fail() {
    // The proof refuses; main.bend prints only Allow. The gate still fails.
    let fixture = fixture("proof-decides");
    stub_bend_runs(&fixture, PROOF_REFUSES_CLIPPY, PRINT_ALL_ALLOW);
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert_eq!(bend_runs(&fixture), ["cases.bend", "main.bend"]);
    assert!(
        text.contains("laws: bend refused clippy_toolchain_pinned"),
        "{text}"
    );
    assert_eq!(cases_json(&fixture)["passed"], false);
}

#[test]
fn a_print_run_short_of_verdicts_leaves_later_laws_unreported() {
    // main.bend prints no verdict, so only the proof's refusal is known.
    let fixture = fixture("print-short");
    stub_bend_runs(&fixture, PROOF_REFUSES_CLIPPY, "printf '()\\n'");
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert_eq!(bend_runs(&fixture), ["cases.bend", "main.bend"]);
    assert!(
        text.contains("laws: bend stops at the first refusal; `unreported` laws were not judged"),
        "{text}"
    );
    let cases = cases_json(&fixture);
    assert_eq!(law(&cases, "clippy_toolchain_pinned")["verdict"], "Refuse");
    assert_eq!(law(&cases, "single_deny_site")["verdict"], "unreported");
}

#[test]
fn a_passing_proof_that_prints_too_few_verdicts_fails() {
    let fixture = fixture("proof-short");
    stub_bend_runs(
        &fixture,
        "  printf '(LAWS.Allow{})\\n'\n  exit 0",
        PRINT_ALL_ALLOW,
    );
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert_eq!(bend_runs(&fixture), ["cases.bend"]);
    assert!(
        text.contains("laws: bend printed 1 verdict(s) for "),
        "{text}"
    );
    assert_eq!(cases_json(&fixture)["passed"], false);
}

#[test]
fn a_failed_proof_with_no_checked_law_runs_bend_once() {
    // A pack with no law: nothing to print, so no main.bend run.
    let fixture = fixture("no-law");
    fs::write(fixture.repo.join("LAWS.bend"), "# no law\n").expect("write LAWS.bend");
    stub_bend_runs(&fixture, "  exit 1", PRINT_ALL_ALLOW);
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert_eq!(bend_runs(&fixture), ["cases.bend"]);
    assert!(text.contains("laws: bend exited"), "{text}");
}

#[test]
fn a_non_refusing_case_passes_and_lists_not_evaluated_laws() {
    let fixture = fixture("allow");
    stub_bend(&fixture);
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(0), "{text}");
    // A passing proof is the only run: no main.bend.
    assert_eq!(bend_runs(&fixture), ["cases.bend"]);
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
fn coverage_in_range_is_the_one_test_run_of_unit_integration_and_property() {
    let fixture = fixture_with("one-run", &TEST_LAYERS);
    let (out, text) = gate(&fixture, path_with_only_stubs(&fixture.bin));
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert_eq!(nextest_runs(&fixture), [COVERAGE_RUN], "{text}");
    for layer in ["unit", "integration", "property"] {
        assert!(
            text.contains(&format!(
                "\n==> [{layer}] no command of its own: it takes the coverage run's outcome\n"
            )),
            "{layer}: {text}"
        );
        assert!(
            text.contains(&format!(
                "\n[{layer}] took the coverage run's outcome: passed\n"
            )),
            "{layer}: {text}"
        );
        assert!(!text.contains(&format!("==> [{layer}] cargo")), "{text}");
    }
    assert!(
        text.contains("\ncount: 1 command(s) ran; 4 of 4 enabled layer(s) at tier <= 1 ran one; unit, integration, property took the coverage run\n"),
        "{text}"
    );
    assert!(text.contains("\nok: 1 command(s) passed\n"), "{text}");

    // A dry run lists the one command and still is not a pass.
    fs::remove_file(fixture.root.join("cargo.log")).expect("clear the cargo log");
    let (out, text) = gate_with(
        &fixture,
        path_with_only_stubs(&fixture.bin),
        None,
        &["--dry-run"],
    );
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("\n==> [unit] no command of its own: it takes the coverage run's outcome\n"),
        "{text}"
    );
    assert!(
        text.contains("dry-run: 1 command(s) listed, 0 run"),
        "{text}"
    );
    assert!(nextest_runs(&fixture).is_empty(), "{}", cargo_log(&fixture));
}

#[test]
fn a_missing_or_failed_coverage_run_is_no_pass_for_the_layers_that_took_it() {
    // Missing: no cargo-llvm-cov, so nothing runs the suite.
    let missing = fixture_with("one-run-missing", &TEST_LAYERS);
    fs::remove_file(missing.bin.join("cargo-llvm-cov")).expect("remove stub");
    let (out, text) = gate(&missing, path_with_only_stubs(&missing.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(nextest_runs(&missing).is_empty(), "{}", cargo_log(&missing));
    for layer in ["unit", "integration", "property"] {
        assert!(
            text.contains(&format!(
                "\n  - {layer}: no command of its own; the coverage run it took is not on PATH\n"
            )),
            "{layer}: {text}"
        );
    }
    assert!(
        text.contains("count: 0 command(s) ran; 0 of 4 enabled layer(s) at tier <= 1 ran one; unit, integration, property took the coverage run; none ran for: unit, integration, coverage, property"),
        "{text}"
    );
    assert!(!text.contains("\nok: "), "{text}");

    // Failed: the coverage run exits 1, and that is what the three take.
    let failed = fixture_with("one-run-failed", &TEST_LAYERS);
    stub_cargo_failing(&failed, "llvm-cov*", 1);
    let (out, text) = gate(&failed, path_with_only_stubs(&failed.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert_eq!(nextest_runs(&failed), [COVERAGE_RUN], "{text}");
    assert!(
        text.contains("\n  - coverage: `cargo llvm-cov nextest"),
        "{text}"
    );
    for layer in ["unit", "integration", "property"] {
        assert!(
            text.contains(&format!(
                "\n  - {layer}: no command of its own; the coverage run it took is Failed\n"
            )),
            "{layer}: {text}"
        );
        assert!(
            text.contains(&format!(
                "\n[{layer}] took the coverage run's outcome: Failed\n"
            )),
            "{layer}: {text}"
        );
    }
    assert!(
        text.contains("fail: 4 problem(s), 1 command(s) run"),
        "{text}"
    );
}

#[test]
fn below_coverage_unit_runs_once_integration_takes_it_and_property_runs_itself() {
    // Coverage above --max-tier, then coverage off: no coverage run either way.
    for (name, edit) in [("above", true), ("off", false)] {
        let fixture = fixture_with(&format!("one-run-{name}"), &TEST_LAYERS);
        if edit {
            set_tier(&fixture, "coverage", 2);
        } else {
            edit_yaml(&fixture, |doc| {
                for layer in doc["layers"].as_array_mut().expect("layers") {
                    if layer["id"] == "coverage" {
                        layer["enabled"] = serde_json::Value::Bool(false);
                    }
                }
            });
        }
        let (out, text) = gate(&fixture, path_with_only_stubs(&fixture.bin));
        assert_eq!(out.status.code(), Some(0), "{name}: {text}");
        assert_eq!(
            nextest_runs(&fixture),
            [UNIT_RUN, PROPERTY_RUN],
            "{name}: {text}"
        );
        assert!(
            text.contains(
                "\n==> [unit] cargo nextest run --workspace --all-features --profile ci\n"
            ),
            "{name}: {text}"
        );
        assert!(
            text.contains(
                "\n==> [integration] no command of its own: it takes the unit run's outcome\n"
            ),
            "{name}: {text}"
        );
        assert!(
            text.contains("\n[integration] took the unit run's outcome: passed\n"),
            "{name}: {text}"
        );
        assert!(
            text.contains("\n==> [property] cargo nextest run --workspace --all-features --profile ci --no-tests=fail property\n"),
            "{name}: {text}"
        );
        assert!(!text.contains("==> [coverage]"), "{name}: {text}");
        assert!(
            text.contains("\ncount: 2 command(s) ran; 3 of 3 enabled layer(s) at tier <= 1 ran one; integration took the unit run\n"),
            "{name}: {text}"
        );
        let skipped = if edit {
            "above --max-tier 1, not judged: coverage"
        } else {
            "not run (enabled: false): "
        };
        assert!(text.contains(skipped), "{name}: {text}");
    }

    // --max-tier below coverage's tier: only unit is in range, and it runs.
    let fixture = fixture_with("one-run-tier-0", &TEST_LAYERS);
    set_tier(&fixture, "coverage", 1);
    let (out, text) = gate_with(
        &fixture,
        path_with_only_stubs(&fixture.bin),
        None,
        &["--max-tier", "0"],
    );
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert_eq!(nextest_runs(&fixture), [UNIT_RUN], "{text}");
    assert!(
        text.contains("above --max-tier 0, not judged: integration, coverage, property"),
        "{text}"
    );
    assert!(
        text.contains("\ncount: 1 command(s) ran; 1 of 1 enabled layer(s) at tier <= 0 ran one\n"),
        "{text}"
    );
}

#[test]
fn a_property_filter_that_matches_nothing_fails() {
    // nextest exits 4 when `--no-tests=fail` finds no test; the stub does too.
    let fixture = fixture_with("one-run-empty-filter", &["property"]);
    stub_cargo_failing(&fixture, "*' --no-tests=fail property'", 4);
    let (out, text) = gate(&fixture, path_with_only_stubs(&fixture.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert_eq!(nextest_runs(&fixture), [PROPERTY_RUN], "{text}");
    assert!(
        text.contains("\n  - property: `cargo nextest run --workspace --all-features --profile ci --no-tests=fail property` failed\n"),
        "{text}"
    );
    assert!(!text.contains("\nok: "), "{text}");
}

#[test]
fn a_path_bend_runs_only_when_opted_in_and_the_log_names_it() {
    let fixture = fixture_with("path-bend", &["laws"]);
    adopt(&fixture, Origin::None);
    stub_bend(&fixture);
    stub(&fixture.bin, "curl", "#!/bin/sh\nexit 22\n");
    let mut cmd = gate_command(&fixture, path_without_bend(&fixture.bin), None);
    cmd.env_remove("CARGO_TOPS_BEND_PATH");
    let (out, text) = run_gate(cmd);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("laws: scripts/fetch-bend.sh failed"),
        "{text}"
    );
    assert!(!text.contains("bend PATH fallback"), "{text}");
    assert!(bend_runs(&fixture).is_empty(), "{:?}", bend_runs(&fixture));

    let (_, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert!(
        text.contains(&format!(
            "laws: bend PATH fallback {}\n",
            fixture.bin.join("bend").display()
        )),
        "{text}"
    );
    assert_eq!(
        bend_runs(&fixture).first().map(String::as_str),
        Some("cases.bend")
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
    assert_eq!(layer(&yaml_doc(&fixture), "laws")["enabled"], true);
    edit_yaml(&fixture, |doc| {
        for layer in doc["layers"].as_array_mut().expect("layers") {
            if layer["id"] == "laws" {
                layer["enabled"] = serde_json::Value::Bool(false);
            }
        }
    });
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
fn a_disabled_layer_is_printed_as_not_run() {
    // Only static-lint is on. Every other layer, in range or above it, is
    // named once as not run, and none of them is counted short.
    let fixture = fixture_with("disabled", &["static-lint"]);
    adopt(&fixture, Origin::None);
    let off = disabled_line(&fixture);
    for id in ["unit", "coverage", "mutation", "acceptance", "fuzz", "laws"] {
        assert!(
            off.contains(&format!(" {id},")) || off.ends_with(&format!(" {id}\n")),
            "{id}: {off}"
        );
    }
    for args in [&["--dry-run"][..], &[]] {
        let (out, text) = gate_with(&fixture, path_with_only_stubs(&fixture.bin), None, args);
        assert!(text.contains(&off), "{args:?}: {text}");
        assert_eq!(
            text.matches("not run (enabled: false)").count(),
            1,
            "{text}"
        );
        assert!(!text.contains("==> [unit]"), "{text}");
        assert!(!text.contains("above --max-tier"), "{text}");
        let code = if args.is_empty() { 0 } else { 1 };
        assert_eq!(out.status.code(), Some(code), "{args:?}: {text}");
    }
    let (_, text) = gate(&fixture, path_with_only_stubs(&fixture.bin));
    assert!(
        text.contains("count: 3 command(s) ran; 1 of 1 enabled layer(s) at tier <= 1 ran one\n"),
        "{text}"
    );

    // With every layer on, there is no such line.
    edit_yaml(&fixture, |doc| {
        for layer in doc["layers"].as_array_mut().expect("layers") {
            layer["enabled"] = serde_json::Value::Bool(true);
        }
    });
    let (_, text) = gate_with(
        &fixture,
        path_with_only_stubs(&fixture.bin),
        None,
        &["--dry-run"],
    );
    assert!(!text.contains("not run (enabled: false)"), "{text}");
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
    assert!(
        text.contains("Vacuous: no origin/main; the mutation diff is empty"),
        "{text}"
    );
    assert!(!text.contains("Vacuous: the diff from"), "{text}");
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
    assert!(
        text.contains("Vacuous: the diff from `git merge-base origin/main HEAD` is empty"),
        "{text}"
    );
    assert!(!text.contains("Vacuous: no origin/main"), "{text}");
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

#[test]
fn an_enabled_unmapped_layer_in_range_is_not_run_and_fails() {
    // `init` writes `acceptance` off. Turned on, it is in range at tier 1
    // and has no command: NotRun, and the gate exits 1. No exemption.
    let fixture = fixture_with("not-run", &["static-lint", "acceptance", "laws"]);
    adopt(&fixture, Origin::None);
    stub_bend(&fixture);
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("NotRun [acceptance]: enabled at tier 1 (<= --max-tier 1)"),
        "{text}"
    );
    assert!(
        text.contains("  - acceptance: NotRun (enabled at tier 1, no command mapped"),
        "{text}"
    );
    assert!(!text.contains("skip acceptance"), "{text}");
    assert!(text.contains("none ran for: acceptance"), "{text}");
    let yaml = fs::read_to_string(fixture.repo.join("rust-tops.yaml")).expect("yaml");
    let doc: serde_json::Value = serde_saphyr::from_str(&yaml).expect("yaml parses");
    assert_eq!(doc["exemptions"], serde_json::json!([]));

    // A dry run lists it and exits 1 too.
    let (out, text) = gate_with(
        &fixture,
        path_without_bend(&fixture.bin),
        None,
        &["--dry-run"],
    );
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("1 layer(s) NotRun, so the gate would fail"),
        "{text}"
    );
}

#[test]
fn a_layer_above_max_tier_is_not_judged_and_in_range_it_is() {
    let fixture = fixture_with("above-tier", &["static-lint", "fuzz", "laws"]);
    adopt(&fixture, Origin::None);
    stub_bend(&fixture);
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("above --max-tier 1, not judged: fuzz"),
        "{text}"
    );
    assert!(!text.contains("NotRun"), "{text}");

    let (out, text) = gate_with(
        &fixture,
        path_without_bend(&fixture.bin),
        None,
        &["--max-tier", "3"],
    );
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("NotRun [fuzz]: enabled at tier 3 (<= --max-tier 3)"),
        "{text}"
    );
}

#[test]
fn formal_kani_in_range_runs_cargo_kani() {
    let fixture = fixture_with("kani", &["static-lint", "formal-kani", "laws"]);
    adopt(&fixture, Origin::None);
    stub_bend(&fixture);
    let (out, text) = gate_with(
        &fixture,
        path_without_bend(&fixture.bin),
        None,
        &["--max-tier", "4"],
    );
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("==> [formal-kani] cargo kani --workspace"),
        "{text}"
    );
    assert!(
        cargo_log(&fixture)
            .lines()
            .any(|line| line == "kani --workspace"),
        "{}",
        cargo_log(&fixture)
    );
    assert!(!text.contains("NotRun"), "{text}");
}

#[test]
fn a_missing_cargo_kani_fails_in_range_and_not_at_tier_1() {
    let fixture = fixture_with("no-kani", &["static-lint", "formal-kani", "laws"]);
    adopt(&fixture, Origin::None);
    stub_bend(&fixture);
    fs::remove_file(fixture.bin.join("cargo-kani")).expect("remove stub");
    let (out, text) = gate_with(
        &fixture,
        path_with_only_stubs(&fixture.bin),
        None,
        &["--max-tier", "4"],
    );
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("formal-kani: `cargo-kani` not on PATH"),
        "{text}"
    );
    assert!(
        !cargo_ran(&fixture, "kani"),
        "cargo kani ran without cargo-kani"
    );

    // A fresh init, every layer as `init` wrote it, at the default
    // `--max-tier 1`: Kani is missing and the gate still passes.
    let fresh = init_fixture("no-kani-fresh", "library-core");
    adopt(&fresh, Origin::None);
    stub_bend(&fresh);
    fs::remove_file(fresh.bin.join("cargo-kani")).expect("remove stub");
    let (out, text) = gate(&fresh, path_with_only_stubs(&fresh.bin));
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(ABOVE_TIER_1), "{text}");
    assert!(!text.contains("not on PATH"), "{text}");
}

/// Real Bend 2.0.36 on `PATH`: a refusing repo fails, and a fresh init of
/// every class with `laws` turned on passes.
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
    // The main.bend run gave every checked law a verdict.
    let cases = cases_json(&refused);
    let laws = cases["laws"].as_array().expect("laws");
    assert!(
        laws.iter()
            .filter(|law| law["status"] == "checked")
            .all(|law| matches!(law["verdict"].as_str(), Some("Allow" | "Ack" | "Refuse"))),
        "{cases}"
    );
    assert_eq!(law(&cases, "clippy_toolchain_pinned")["verdict"], "Refuse");

    for class in INIT_CLASSES {
        expect_fresh_init_with_laws_passes(class, path_with_bend, false);
    }
}
