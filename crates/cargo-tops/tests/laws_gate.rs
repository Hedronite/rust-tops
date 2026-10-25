//! `cargo tops gate` with the `laws` layer, against a `cargo tops init` repo.
//!
//! `bend` is a POSIX sh stub, as in `scripts/test-bend-gate.sh`. It refuses
//! exactly the case constructors the fixture's own `LAWS.bend` maps to
//! `Refuse`, stops at the first one like `bend` does, and otherwise prints
//! one `Allow{}` per law. Cargo and its plugins are stubs that pass and
//! leave the artifacts a real run records. The `#[ignore]` test at the end
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
/// `cargo llvm-cov` and `cargo crap` do.
const CARGO_STUB: &str = r#"#!/bin/sh
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
    let fixture = init_fixture(name, "library-core");
    let yaml_path = fixture.repo.join("rust-tops.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read rust-tops.yaml");
    let only_lint_and_laws: Vec<String> = yaml
        .lines()
        .map(|line| {
            let keep = line.contains("id: static-lint,") || line.contains("id: laws,");
            if line.contains("- { id:") && !keep {
                line.replace("enabled: true", "enabled: false")
            } else {
                line.to_string()
            }
        })
        .collect();
    fs::write(&yaml_path, only_lint_and_laws.join("\n") + "\n").expect("write rust-tops.yaml");
    fixture
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

/// A fresh init repo for `class` passes `gate` with every layer it enables,
/// and the self-fail laws chose their passing facts.
fn expect_fresh_init_passes(class: &str, path: impl Fn(&Path) -> OsString, stubbed: bool) {
    let fixture = init_fixture(&format!("fresh-{class}-{stubbed}"), class);
    if stubbed {
        stub_bend(&fixture);
    }
    let (out, text) = gate(&fixture, path(&fixture.bin));
    assert_eq!(out.status.code(), Some(0), "{class}: {text}");
    let cases = cases_json(&fixture);
    assert_eq!(cases["passed"], true, "{class}");
    for (name, case) in [
        ("clippy_toolchain_pinned", "ClippyPinned"),
        ("ci_matrix_matches_class", "ClassJobPresent"),
        ("drop_in_is_a_floor_not_a_ceiling", "DropInFloor"),
        ("record_only_artifacts_are_nonempty", "RecordNonempty"),
        ("crap_wires_coverage", "CrapWired"),
        ("baseline_green_before_mutants", "BaselineGreen"),
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
