//! `cargo tops gate` with the `laws` layer, against a `cargo tops init` repo.
//!
//! `bend` is a POSIX sh stub, as in `scripts/test-bend-gate.sh`: it refuses
//! when the case file applies a verdict to `ClippyUnpinned{}` and otherwise
//! prints one `Allow{}` per law. `cargo` and `cargo-deny` are stubs that
//! pass, so only `static-lint` and `laws` run. The `#[ignore]` test at the
//! end swaps the stub for the real pinned Bend; CI runs it in
//! `bend-gate-matrix`.
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BEND_STUB: &str = r#"#!/bin/sh
if [ -n "${BEND_STUB_LOG:-}" ]; then
  printf '%s\n' "$*" >> "$BEND_STUB_LOG"
fi
if [ "$1" = version ]; then
  echo "bend 2.0.36"
  exit 0
fi
n=0
refuse=
while IFS= read -r line || [ -n "$line" ]; do
  case $line in
    "law case_"*) n=$((n + 1)) ;;
    *"Laws.ClippyUnpinned{}"*) refuse=clippy_toolchain_pinned ;;
  esac
done < "$1"
if [ -n "$refuse" ]; then
  printf 'SOME PROOFS FAIL\nLocation: case_%s\n' "$refuse"
  exit 1
fi
out=
i=0
while [ "$i" -lt "$n" ]; do
  out="$out${out:+, }LAWS.Allow{}"
  i=$((i + 1))
done
printf '(%s)\n' "$out"
"#;

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

/// An initialized library-core repo with every layer but `static-lint` and
/// `laws` turned off, and passing `cargo` / `cargo-deny` stubs.
fn fixture(name: &str) -> Fixture {
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
        .args([
            "init",
            "--class",
            "library-core",
            "--name",
            "fixture",
            "--path",
        ])
        .arg(&fixture.repo)
        .output()
        .expect("spawn cargo-tops init");
    assert!(
        init.status.success(),
        "init: {}",
        String::from_utf8_lossy(&init.stderr)
    );
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
    stub(&fixture.bin, "cargo", "#!/bin/sh\nexit 0\n");
    stub(&fixture.bin, "cargo-deny", "#!/bin/sh\nexit 0\n");
    fixture
}

fn pin_toolchain(fixture: &Fixture) {
    fs::write(
        fixture.repo.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.99.1\"\n",
    )
    .expect("write rust-toolchain.toml");
}

/// The stub dir, then the inherited `PATH` minus any directory holding a
/// real `bend`.
fn path_without_bend(bin: &Path) -> OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let dirs = std::iter::once(bin.to_path_buf())
        .chain(std::env::split_paths(&inherited).filter(|dir| !dir.join("bend").exists()));
    std::env::join_paths(dirs).expect("join PATH")
}

fn gate(fixture: &Fixture, path: OsString) -> (Output, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(["gate", "--path"])
        .arg(&fixture.repo)
        .env("PATH", path)
        .env("HOME", &fixture.home)
        .env("BEND_STUB_LOG", fixture.root.join("bend.log"))
        .env_remove("RUST_TOPS_ROOT")
        .output()
        .expect("spawn cargo-tops gate");
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

#[test]
fn init_writes_the_laws_layer_and_the_bend_kit() {
    let fixture = fixture("init");
    let yaml = fs::read_to_string(fixture.repo.join("rust-tops.yaml")).expect("yaml");
    assert!(yaml.contains("version: \"1.1.0\""));
    assert!(yaml.contains("{ id: laws,              enabled: true,  tier: 0, runner: bend,"));
    for rel in ["LAWS.bend", "laws/bend.lock", "scripts/fetch-bend.sh"] {
        assert!(fixture.repo.join(rel).is_file(), "init did not write {rel}");
    }
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
fn a_refusing_case_fails_the_gate() {
    let fixture = fixture("refuse");
    stub(&fixture.bin, "bend", BEND_STUB);
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
    assert_eq!(law(&cases, "fmt_is_tier0")["verdict"], "unreported");
}

#[test]
fn a_non_refusing_case_passes_and_lists_not_evaluated_laws() {
    let fixture = fixture("allow");
    pin_toolchain(&fixture);
    stub(&fixture.bin, "bend", BEND_STUB);
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
    stub(&fixture.bin, "bend", BEND_STUB);
    let (out, text) = gate(&fixture, path_without_bend(&fixture.bin));
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        !fixture.root.join("bend.log").exists(),
        "bend was called with laws off"
    );
    assert!(!fixture.repo.join("target/rust-tops").exists());
}

/// Real Bend 2.0.36 on `PATH` judges the same two fixtures.
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
    let real_path = |bin: &Path| {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let dirs = std::iter::once(bin.to_path_buf()).chain(std::env::split_paths(&inherited));
        std::env::join_paths(dirs).expect("join PATH")
    };

    let refused = fixture("real-refuse");
    let (out, text) = gate(&refused, real_path(&refused.bin));
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("laws: bend refused clippy_toolchain_pinned"),
        "{text}"
    );
    assert!(text.contains("SOME PROOFS FAIL"), "{text}");

    let allowed = fixture("real-allow");
    pin_toolchain(&allowed);
    let (out, text) = gate(&allowed, real_path(&allowed.bin));
    assert_eq!(out.status.code(), Some(0), "{text}");
    let cases = cases_json(&allowed);
    let checked: Vec<&serde_json::Value> = cases["laws"]
        .as_array()
        .expect("laws")
        .iter()
        .filter(|entry| entry["status"] == "checked")
        .collect();
    assert!(!checked.is_empty());
    for entry in checked {
        assert!(
            matches!(entry["verdict"].as_str(), Some("Allow" | "Ack")),
            "{entry}"
        );
    }
    assert_eq!(
        law(&cases, "evidence_not_self_served")["status"],
        "not-evaluated"
    );
}
