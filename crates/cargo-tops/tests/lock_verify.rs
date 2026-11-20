//! `cargo tops lock --verify` and `--write`, against shims on `PATH`.
//!
//! Each test makes its own scratch directory under the system temp dir and
//! removes only that. The `PATH` of the command under test is the shim
//! directory alone, so a real tool on the host never answers for a missing
//! one. The `cargo` shim hands `cargo <sub> …` to a `cargo-<sub>` shim in the
//! same directory, as the real cargo does.
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

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

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("cargo-tops-lock-{name}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(root.join("bin")).expect("mkdir bin");
        fs::create_dir_all(root.join("repo")).expect("mkdir repo");
        let scratch = Scratch { root };
        scratch.script(
            "cargo",
            "#!/bin/sh\nsub=\"$1\"; shift\nexec \"$(dirname \"$0\")/cargo-$sub\" \"$@\"\n",
        );
        scratch
    }

    fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    fn repo(&self) -> PathBuf {
        self.root.join("repo")
    }

    fn script(&self, name: &str, body: &str) {
        let path = self.bin().join(name);
        fs::write(&path, body).expect("write shim");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod shim");
    }

    /// A tool that prints `line` and, for a nextest-shaped shim, more lines.
    fn tool(&self, name: &str, line: &str) {
        self.script(name, &format!("#!/bin/sh\nprintf '%s\\n' '{line}'\n"));
    }

    fn lock(&self, text: &str) {
        fs::write(self.repo().join("rust-tops.lock"), text).expect("write lock");
    }

    fn read_lock(&self) -> String {
        fs::read_to_string(self.repo().join("rust-tops.lock")).expect("read lock")
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
            .arg("lock")
            .args(args)
            .arg("--path")
            .arg(self.repo())
            .env("PATH", self.bin())
            .output()
            .expect("spawn cargo-tops lock")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A shim for every tool in the lock's `[tools]` that prints a line in the
/// tool's own shape. `fuzz` and `mutants` keep the pin as the version, with a
/// patch part added when the pin has two parts, as the real tool prints.
fn shim_all(scratch: &Scratch, pins: &BTreeMap<String, String>) {
    let full = |pin: &str| {
        if pin.matches('.').count() == 1 {
            format!("{pin}.0")
        } else {
            pin.to_string()
        }
    };
    let lines: [(&str, &str, &str); 8] = [
        ("rustc", "rustc", "rustc {} (58cf8f9c3 2026-10-13)"),
        (
            "cargo-nextest",
            "cargo-nextest",
            "cargo-nextest {} (00af4550e 2026-09-16)",
        ),
        ("cargo-llvm-cov", "cargo-llvm-cov", "cargo-llvm-cov {}"),
        ("cargo-mutants", "cargo-mutants", "cargo-mutants {}"),
        ("cargo-crap", "cargo-crap", "cargo-crap {}"),
        ("cargo-deny", "cargo-deny", "cargo-deny {}"),
        ("cargo-fuzz", "cargo-fuzz", "cargo-fuzz {}"),
        ("nushell", "nu", "{}"),
    ];
    for (key, shim, shape) in lines {
        if let Some(pin) = pins.get(key) {
            scratch.tool(shim, &shape.replace("{}", &full(pin)));
        }
    }
    if let Some(pin) = pins.get("kani-verifier") {
        scratch.tool("cargo-kani", &format!("cargo-kani {}", full(pin)));
    }
}

fn pins_of(lock: &str) -> BTreeMap<String, String> {
    let table: toml::Table = lock.parse().expect("lock is toml");
    table
        .get("tools")
        .and_then(toml::Value::as_table)
        .expect("[tools]")
        .iter()
        .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
        .collect()
}

fn init(scratch: &Scratch, class: &str) {
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(["init", "--class", class, "--name", "fixture", "--path"])
        .arg(scratch.repo())
        .output()
        .expect("spawn cargo-tops init");
    assert_eq!(out.status.code(), Some(0), "{class}: {}", text(&out));
}

#[test]
fn a_nextest_that_prints_another_version_exits_3_and_names_tool_pin_and_found() {
    let scratch = Scratch::new("nextest");
    scratch.lock("[tools]\ncargo-nextest = \"0.9.145\"\n");
    scratch.tool(
        "cargo-nextest",
        "cargo-nextest 0.9.144 (00af4550e 2026-09-16)",
    );
    let out = scratch.run(&["--verify"]);
    let said = text(&out);
    assert_eq!(out.status.code(), Some(3), "{said}");
    assert!(
        said.contains("mismatch cargo-nextest: pin 0.9.145, found 0.9.144"),
        "{said}"
    );
}

#[test]
fn a_partial_pin_matches_whole_parts_only() {
    let scratch = Scratch::new("partial");
    scratch.lock("[tools]\ncargo-fuzz = \"0.12\"\n");
    scratch.tool("cargo-fuzz", "cargo-fuzz 0.120.0");
    let out = scratch.run(&["--verify"]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    assert!(text(&out).contains("found 0.120.0"), "{}", text(&out));
    scratch.tool("cargo-fuzz", "cargo-fuzz 0.12.3");
    let out = scratch.run(&["--verify"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
}

#[test]
fn a_pinned_tool_that_is_not_on_path_is_missing_and_exits_3() {
    let scratch = Scratch::new("missing");
    scratch.lock("[tools]\ncargo-crap = \"0.9.0\"\n");
    let out = scratch.run(&["--verify"]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    assert!(
        text(&out).contains("missing  cargo-crap: pin 0.9.0, not found"),
        "{}",
        text(&out)
    );
}

#[test]
fn library_and_name_rows_are_skipped_and_the_run_passes() {
    let scratch = Scratch::new("skipped");
    scratch.lock(
        "[tools]\nproptest = \"1\"\ncriterion = \"0.8\"\ncallgrind-runner = \"gungraun-runner\"\n",
    );
    let out = scratch.run(&["--verify"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(text(&out).contains("skipped  proptest"), "{}", text(&out));
}

#[test]
fn a_missing_or_unparsable_lock_exits_2() {
    let scratch = Scratch::new("unreadable");
    let out = scratch.run(&["--verify"]);
    assert_eq!(out.status.code(), Some(2), "no lock: {}", text(&out));
    scratch.lock("[tools\nrustc = ");
    let out = scratch.run(&["--verify"]);
    assert_eq!(out.status.code(), Some(2), "bad toml: {}", text(&out));
}

#[test]
fn write_replaces_values_and_keeps_comments_order_and_other_tables() {
    let scratch = Scratch::new("write");
    let before = "# head comment\n[tools]\nrustc = \"1.99.1\"  # the toolchain\n# nextest note\ncargo-nextest = \"0.9.145\"\ncargo-crap = \"0.9.0\"\nproptest = \"1\"\n\n[notes]\nschema = \"1.3.0\"\n";
    scratch.lock(before);
    scratch.tool("rustc", "rustc 1.99.2 (aaaaaaaaa 2026-11-01)");
    scratch.tool(
        "cargo-nextest",
        "cargo-nextest 0.9.150 (bbbbbbbbb 2026-11-02)",
    );
    // cargo-crap is not on PATH: its value is left alone.
    let out = scratch.run(&["--write"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(
        scratch.read_lock(),
        "# head comment\n[tools]\nrustc = \"1.99.2\"  # the toolchain\n# nextest note\ncargo-nextest = \"0.9.150\"\ncargo-crap = \"0.9.0\"\nproptest = \"1\"\n\n[notes]\nschema = \"1.3.0\"\n"
    );
}

#[test]
fn verify_never_writes_the_lock() {
    let scratch = Scratch::new("readonly");
    let before = "[tools]\ncargo-crap = \"0.9.0\"\n";
    scratch.lock(before);
    scratch.tool("cargo-crap", "cargo-crap 0.9.1");
    let out = scratch.run(&["--verify"]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    assert_eq!(scratch.read_lock(), before);
}

#[test]
fn max_tier_asks_only_for_the_tools_of_layers_in_range() {
    let scratch = Scratch::new("tier");
    init(&scratch, "library-core");
    let pins = pins_of(&scratch.read_lock());
    shim_all(&scratch, &pins);
    fs::remove_file(scratch.bin().join("cargo-mutants")).expect("drop mutants");
    fs::remove_file(scratch.bin().join("cargo-fuzz")).expect("drop fuzz");
    // The mutation layer is tier 1 and fuzz is tier 3.
    let tier0 = scratch.run(&["--verify", "--max-tier", "0"]);
    assert_eq!(tier0.status.code(), Some(0), "{}", text(&tier0));
    let tier1 = scratch.run(&["--verify", "--max-tier", "1"]);
    assert_eq!(tier1.status.code(), Some(3), "{}", text(&tier1));
    assert!(
        text(&tier1).contains("missing  cargo-mutants"),
        "{}",
        text(&tier1)
    );
    assert!(!text(&tier1).contains("cargo-fuzz"), "{}", text(&tier1));
}

#[test]
fn the_lock_of_every_init_class_passes_verify_against_shims_at_the_pins() {
    for class in INIT_CLASSES {
        let scratch = Scratch::new(&format!("init-{class}"));
        init(&scratch, class);
        let pins = pins_of(&scratch.read_lock());
        shim_all(&scratch, &pins);
        let out = scratch.run(&["--verify"]);
        assert_eq!(out.status.code(), Some(0), "{class}: {}", text(&out));
    }
}
