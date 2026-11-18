//! The port skill: its SKILL.md shape, `inventory` on the fixtures, and each
//! `verify` plant. The script tests need Nushell; with `RUST_TOPS_REQUIRE_NU`
//! set a missing `nu` fails the test.
#![expect(clippy::panic, reason = "this crate already violates the strict table")]
#![expect(
    clippy::unwrap_used,
    reason = "this crate already violates the strict table"
)]
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn skill_dir() -> PathBuf {
    manifest().join("templates/skills/rust-tops-port")
}

fn nu_bin() -> Option<PathBuf> {
    let found = Command::new("sh")
        .args(["-c", "command -v nu"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    if found.is_some() {
        return found;
    }
    let local = PathBuf::from("/tmp/nu/nu");
    local.is_file().then_some(local)
}

fn require_nu() -> Option<PathBuf> {
    let nu = nu_bin();
    if nu.is_none() {
        assert!(
            std::env::var_os("RUST_TOPS_REQUIRE_NU").is_none(),
            "nu is not installed; RUST_TOPS_REQUIRE_NU is set"
        );
        eprintln!("nu is not installed; port skill smoke skipped");
    }
    nu
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}

/// A scratch copy of the port fixtures with its own target directory.
struct Rig {
    root: PathBuf,
    nu: PathBuf,
}

impl Rig {
    fn new(nu: PathBuf, label: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("cargo-tops-port-{label}-{}", std::process::id()));
        fs::remove_dir_all(&root).unwrap_or_default();
        copy_tree(&manifest().join("tests/fixtures/port"), &root.join("port"));
        Self { root, nu }
    }

    fn krate(&self, lang: &str) -> PathBuf {
        self.root.join("port").join(lang).join("rust")
    }

    fn run(&self, cwd: &Path, args: &[&str]) -> (Value, Vec<u8>) {
        let out = Command::new(&self.nu)
            .arg(skill_dir().join("rust-tops-port.nu"))
            .args(args)
            .current_dir(cwd)
            .env("CARGO_TARGET_DIR", self.root.join("target"))
            .output()
            .expect("nu port");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout);
        let json =
            serde_json::from_str(text.trim()).unwrap_or_else(|err| panic!("json ({err}): {text}"));
        (json, out.stdout)
    }

    fn verify(&self, lang: &str, map: &str) -> Value {
        self.run(&self.krate(lang), &["verify", "--map", map]).0
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap_or_default();
    }
}

fn ok(v: &Value) -> bool {
    v.pointer("/ok").and_then(Value::as_bool).unwrap()
}

fn messages(v: &Value) -> Vec<String> {
    v.pointer("/data/findings")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|f| f.pointer("/message").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn ids(v: &Value) -> Vec<String> {
    v.pointer("/data/tests")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|t| t.pointer("/id").and_then(Value::as_str).unwrap().to_owned())
        .collect()
}

#[test]
fn skill_md_has_the_required_shape() {
    let skill = fs::read_to_string(skill_dir().join("SKILL.md")).unwrap();
    assert!(skill.len() <= 4000, "{} bytes", skill.len());
    assert!(skill.starts_with("---\nname: rust-tops-port\ndescription: "));
    assert!(skill.contains("Distilled from BeautifulRust v1.5.0 (40db61c6)"));
    assert!(skill.contains("Nushell"));
    assert!(skill.contains("nu: command not found"));
    let catalog =
        fs::read_to_string(manifest().join("templates/skills/rust-tops-style/rules.toml")).unwrap();
    let mut named = 0;
    for (at, _) in skill.match_indices("PORT-") {
        let tail = &skill[at..];
        let id: String = tail
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        assert!(
            catalog.contains(&format!("id = \"{id}\"")),
            "{id} not in the catalog"
        );
        named += 1;
    }
    assert!(named >= 10, "{named} PORT ids named");
    let script = fs::read_to_string(skill_dir().join("rust-tops-port.nu")).unwrap();
    assert!(script.starts_with("#!/usr/bin/env nu\n"));
    for banned in ["^grep", "^sed", "^find", "^awk", "^rm"] {
        assert!(!script.contains(banned), "{banned} in the script");
    }
}

#[test]
fn inventory_lists_exactly_the_fixture_tests() {
    let Some(nu) = require_nu() else { return };
    let rig = Rig::new(nu, "inventory");
    let (py, _) = rig.run(
        &rig.root,
        &[
            "inventory",
            "--from",
            "port/python/original",
            "--lang",
            "python",
        ],
    );
    assert!(ok(&py), "{py}");
    assert_eq!(
        ids(&py),
        [
            "test_wc.py::test_empty",
            "test_wc.py::test_counts_words",
            "test_wc.py::test_first_of_a_tie_wins"
        ]
    );
}

#[test]
fn inventory_lists_the_c_fixture_tests() {
    let Some(nu) = require_nu() else { return };
    let rig = Rig::new(nu, "c");
    let (c, _) = rig.run(
        &rig.root,
        &["inventory", "--from", "port/c/original", "--lang", "c"],
    );
    assert_eq!(
        ids(&c),
        [
            "tests/test_lines.c::test_no_lines",
            "tests/test_lines.c::test_trailing_newline",
            "tests/test_lines.c::lines.unterminated_last_line"
        ]
    );
    let (bad, _) = rig.run(
        &rig.root,
        &["inventory", "--from", "port/c/original", "--lang", "cobol"],
    );
    assert!(!ok(&bad));
    let (go, _) = rig.run(
        &rig.root,
        &["inventory", "--from", "port/c/original", "--lang", "go"],
    );
    assert!(ok(&go));
    assert!(ids(&go).is_empty(), "{go}");
}

#[test]
fn inventory_output_is_byte_identical_and_write_refuses_to_overwrite() {
    let Some(nu) = require_nu() else { return };
    let rig = Rig::new(nu, "write");
    let args = [
        "inventory",
        "--from",
        "port/python/original",
        "--lang",
        "python",
    ];
    let (_, first) = rig.run(&rig.root, &args);
    let (_, second) = rig.run(&rig.root, &args);
    assert_eq!(first, second);
    let text = String::from_utf8(first).unwrap();
    assert!(!text.contains(rig.root.to_str().unwrap()));
    let write = [
        "inventory",
        "--from",
        "port/python/original",
        "--lang",
        "python",
        "--write",
        "map.toml",
    ];
    let (wrote, _) = rig.run(&rig.root, &write);
    assert!(ok(&wrote), "{wrote}");
    let map = fs::read_to_string(rig.root.join("map.toml")).unwrap();
    assert_eq!(map.lines().filter(|l| *l == "[[test]]").count(), 3);
    let (again, _) = rig.run(&rig.root, &write);
    assert!(!ok(&again), "{again}");
    assert_eq!(fs::read_to_string(rig.root.join("map.toml")).unwrap(), map);
}

#[test]
fn another_nushell_minor_is_refused_with_both_versions() {
    let Some(nu) = require_nu() else { return };
    let rig = Rig::new(nu.clone(), "minor");
    let version = Command::new(&nu).arg("--version").output().unwrap();
    let found = String::from_utf8(version.stdout).unwrap().trim().to_owned();
    let minor: u32 = found.split('.').nth(1).unwrap().parse().unwrap();
    let wrong = format!("0.{}.0", if minor == 0 { 1 } else { minor - 1 });
    fs::write(
        rig.root.join("rust-tops.lock"),
        format!("[tools]\nnushell = \"{wrong}\"\n"),
    )
    .unwrap();
    let want = format!("nu {wrong} required, found {found}");
    let (inv, _) = rig.run(
        &rig.root,
        &[
            "inventory",
            "--from",
            "port/python/original",
            "--lang",
            "python",
        ],
    );
    assert!(!ok(&inv), "{inv}");
    assert_eq!(
        inv.pointer("/error").and_then(Value::as_str),
        Some(want.as_str())
    );
    let ver = rig.verify("python", "port-map.toml");
    assert!(!ok(&ver), "{ver}");
    assert_eq!(
        ver.pointer("/error").and_then(Value::as_str),
        Some(want.as_str())
    );
    assert!(ver.pointer("/data").is_none(), "{ver}");
}

#[test]
fn verify_passes_the_complete_fixtures() {
    let Some(nu) = require_nu() else { return };
    let rig = Rig::new(nu, "pass");
    for lang in ["python", "c"] {
        let first = rig.verify(lang, "port-map.toml");
        assert!(ok(&first), "{lang}: {first}");
        assert_eq!(
            first.pointer("/data/mapped").and_then(Value::as_u64),
            Some(3)
        );
    }
}

#[test]
fn verify_output_is_byte_identical_and_holds_no_path() {
    let Some(nu) = require_nu() else { return };
    let rig = Rig::new(nu, "same");
    let args = ["verify", "--map", "port-map.toml"];
    for lang in ["python", "c"] {
        let (_, first) = rig.run(&rig.krate(lang), &args);
        let (_, second) = rig.run(&rig.krate(lang), &args);
        assert_eq!(first, second, "{lang}");
        let text = String::from_utf8(first).unwrap();
        assert!(!text.contains(rig.root.to_str().unwrap()), "{lang}: {text}");
    }
    // A failing run is byte-identical too: findings are sorted.
    let krate = rig.krate("python");
    let good = fs::read_to_string(krate.join("port-map.toml")).unwrap();
    fs::write(
        krate.join("bad.toml"),
        good.replace("tests::test_", "tests::t_"),
    )
    .unwrap();
    let bad = ["verify", "--map", "bad.toml"];
    let (_, first) = rig.run(&krate, &bad);
    let (_, second) = rig.run(&krate, &bad);
    assert_eq!(first, second);
}

#[test]
fn verify_rejects_an_unmapped_original() {
    let Some(nu) = require_nu() else { return };
    let rig = Rig::new(nu, "unmapped");
    let krate = rig.krate("python");
    let good = fs::read_to_string(krate.join("port-map.toml")).unwrap();

    // An original test left unmapped: empty name, and a new original the map never heard of.
    fs::write(
        krate.join("empty.toml"),
        good.replace("rust = \"tests::test_empty\"", "rust = \"\""),
    )
    .unwrap();
    let empty = rig.verify("python", "empty.toml");
    assert!(!ok(&empty), "{empty}");
    assert!(messages(&empty)
        .iter()
        .any(|m| m.contains("unmapped: test_wc.py::test_empty")));
    let extra = rig.root.join("port/python/original/test_extra.py");
    fs::write(&extra, "def test_added_later():\n    pass\n").unwrap();
    let new = rig.verify("python", "port-map.toml");
    assert!(!ok(&new), "{new}");
    assert!(messages(&new)
        .iter()
        .any(|m| m.contains("no map entry: test_extra.py::test_added_later")));
    fs::remove_file(extra).unwrap();
}

#[test]
fn verify_rejects_a_mapped_name_that_does_not_exist() {
    let Some(nu) = require_nu() else { return };
    let rig = Rig::new(nu, "ghost");
    let krate = rig.krate("python");
    let good = fs::read_to_string(krate.join("port-map.toml")).unwrap();
    fs::write(
        krate.join("ghost.toml"),
        good.replace("tests::test_counts_words", "tests::test_ghost"),
    )
    .unwrap();
    let ghost = rig.verify("python", "ghost.toml");
    assert!(!ok(&ghost), "{ghost}");
    assert!(messages(&ghost)
        .iter()
        .any(|m| m.contains("tests::test_ghost")));
}

#[test]
fn verify_rejects_a_failing_rust_test_with_catalog_text() {
    let Some(nu) = require_nu() else { return };
    let rig = Rig::new(nu, "red");
    let krate = rig.krate("python");
    let lib = krate.join("src/lib.rs");
    let source = fs::read_to_string(&lib).unwrap();
    fs::write(&lib, source.replace("Some(\"ab\")", "Some(\"zz\")")).unwrap();
    let red = rig.verify("python", "port-map.toml");
    assert!(!ok(&red), "{red}");
    assert!(messages(&red)
        .iter()
        .any(|m| m.contains("cargo test failed")));

    // Findings come from the catalog.
    let first = red.pointer("/data/findings/0").unwrap();
    assert_eq!(
        first.pointer("/rule").and_then(Value::as_str),
        Some("PORT-4")
    );
    assert_eq!(
        first.pointer("/kind").and_then(Value::as_str),
        Some("probe")
    );
}
