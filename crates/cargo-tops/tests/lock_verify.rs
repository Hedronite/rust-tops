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
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let root = std::env::temp_dir().join(format!(
            "cargo-tops-lock-{name}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        // A fresh name per test and per process: create_dir_all on the new
        // path, and Drop removes only that path.
        fs::create_dir_all(root.join("bin")).expect("mkdir bin");
        fs::create_dir_all(root.join("repo")).expect("mkdir repo");
        let scratch = Scratch { root };
        scratch.script(
            "cargo",
            "#!/bin/sh\nsub=\"$1\"; shift\n# Cargo says exit 101 for a subcommand it does not have.\n[ -x \"${0%/*}/cargo-$sub\" ] || exit 101\nexec \"${0%/*}/cargo-$sub\" \"$@\"\n",
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
        scratch.tool(
            "cargo-kani",
            &format!("Kani Rust Verifier {} (cargo plugin)", full(pin)),
        );
    }
    if pins.contains_key("clippy") {
        scratch.tool("cargo-clippy", "clippy 0.1.99 (58cf8f9c35 2026-10-13)");
    }
    if pins.contains_key("rustfmt") {
        scratch.tool("cargo-fmt", "rustfmt 1.10.0-stable (58cf8f9c35 2026-10-13)");
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
        text(&out).contains("missing  cargo-crap: pin 0.9.0, `cargo crap --version` exited 101"),
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
fn the_lock_every_init_class_writes_passes_verify_against_shims_at_the_pins() {
    // `init_golden` holds every class to these bytes, so this is the lock each
    // of the eight classes gets.
    let golden =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/init/shared/rust-tops.lock");
    let lock_text = fs::read_to_string(golden).expect("the golden lock");
    let scratch = Scratch::new("golden-lock");
    scratch.lock(&lock_text);
    shim_all(&scratch, &pins_of(&lock_text));
    let out = scratch.run(&["--verify"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    // One line per tool the lock names, none of them a mismatch.
    let said = text(&out);
    assert!(
        !said.contains("mismatch") && !said.contains("missing"),
        "{said}"
    );
    for tool in [
        "rustc",
        "cargo-nextest",
        "cargo-llvm-cov",
        "cargo-mutants",
        "cargo-crap",
        "cargo-deny",
        "cargo-fuzz",
        "kani-verifier",
        "nushell",
    ] {
        assert!(
            said.contains(&format!("ok       {tool} ")),
            "{tool}: {said}"
        );
    }
}
#[test]
fn clippy_and_rustfmt_must_run_even_when_rustc_matches_the_pin() {
    let scratch = Scratch::new("toolchain");
    scratch.lock("[tools]\nrustc = \"1.99.1\"\nclippy = \"1.99.1\"\nrustfmt = \"1.99.1\"\n");
    scratch.tool("rustc", "rustc 1.99.1 (58cf8f9c3 2026-10-13)");
    scratch.tool("cargo-fmt", "rustfmt 1.10.0-stable (58cf8f9c35 2026-10-13)");
    let out = scratch.run(&["--verify"]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    assert!(text(&out).contains("missing  clippy"), "{}", text(&out));
    scratch.tool("cargo-clippy", "clippy 0.1.99 (58cf8f9c35 2026-10-13)");
    let out = scratch.run(&["--verify"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
}

/// Runs `lock <args>` on the lock text and returns the output.
fn verify_lock(name: &str, lock: &str, shims: &[(&str, &str)]) -> (Scratch, Output) {
    let scratch = Scratch::new(name);
    scratch.lock(lock);
    for (shim, line) in shims {
        scratch.tool(shim, line);
    }
    let out = scratch.run(&["--verify"]);
    (scratch, out)
}

#[test]
fn a_pin_that_is_not_a_string_is_a_lock_error_naming_the_key() {
    for (lock, key) in [
        ("[tools]\ncargo-crap = 0.9\n", "cargo-crap"),
        ("[tools]\ncargo-mutants = 27\n", "cargo-mutants"),
        (
            "[tools]\ncargo-crap = { version = \"0.9.0\" }\n",
            "cargo-crap",
        ),
        ("[tools]\nrustc = true\n", "rustc"),
    ] {
        let (_scratch, out) = verify_lock("not-a-string", lock, &[]);
        assert_eq!(out.status.code(), Some(2), "{lock}: {}", text(&out));
        assert!(text(&out).contains(key), "{lock}: {}", text(&out));
    }
}

#[test]
fn a_tools_key_that_is_not_a_table_is_a_lock_error() {
    for lock in [
        "[[tools]]\nrustc = \"1.99.1\"\n",
        "tools = \"pinned below\"\n",
    ] {
        let (_scratch, out) = verify_lock("tools-not-a-table", lock, &[]);
        assert_eq!(out.status.code(), Some(2), "{lock}: {}", text(&out));
        assert!(text(&out).contains("tools"), "{lock}: {}", text(&out));
    }
}

#[test]
fn a_lock_with_no_tools_table_is_a_lock_error_and_an_empty_one_says_so() {
    let (_scratch, out) = verify_lock("no-tools", "[notes]\nschema = \"1.3.0\"\n", &[]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("[tools]"), "{}", text(&out));
    let (_scratch, out) = verify_lock("empty-tools", "[tools]\n", &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(
        text(&out).contains("nothing pinned in [tools]"),
        "{}",
        text(&out)
    );
}

#[test]
fn a_pin_that_is_not_dotted_digits_is_a_lock_error() {
    for pin in ["=0.9.0", "v0.9.0", "^0.9", "0.9.*", "", "0..9", "0.9."] {
        let lock = format!("[tools]\ncargo-crap = \"{pin}\"\n");
        let (_scratch, out) = verify_lock("bad-pin", &lock, &[]);
        assert_eq!(out.status.code(), Some(2), "{pin:?}: {}", text(&out));
        assert!(text(&out).contains("cargo-crap"), "{pin:?}: {}", text(&out));
    }
}

#[test]
fn a_pre_release_or_a_build_never_matches_a_release_pin() {
    for (shim, line, lock) in [
        (
            "cargo-crap",
            "cargo-crap 0.9.0-rc.1",
            "[tools]\ncargo-crap = \"0.9.0\"\n",
        ),
        (
            "cargo-crap",
            "cargo-crap 0.9.0-beta.1",
            "[tools]\ncargo-crap = \"0.9\"\n",
        ),
        (
            "rustc",
            "rustc 1.99.1-nightly (58cf8f9c3 2026-10-13)",
            "[tools]\nrustc = \"1.99.1\"\n",
        ),
        (
            "rustc",
            "rustc 1.99.1-beta.3 (58cf8f9c3 2026-10-13)",
            "[tools]\nrustc = \"1.99\"\n",
        ),
        (
            "nu",
            "0.108.0-nightly.3",
            "[tools]\nnushell = \"0.108.0\"\n",
        ),
    ] {
        let (_scratch, out) = verify_lock("pre-release", lock, &[(shim, line)]);
        assert_eq!(out.status.code(), Some(3), "{line}: {}", text(&out));
        assert!(text(&out).contains("mismatch"), "{line}: {}", text(&out));
    }
}

#[test]
fn empty_output_reads_as_no_version_line() {
    let (_scratch, out) = verify_lock(
        "empty-output",
        "[tools]\ncargo-crap = \"0.9.0\"\n",
        &[("cargo-crap", "")],
    );
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    assert!(
        text(&out).contains("mismatch cargo-crap: pin 0.9.0, found no version line"),
        "{}",
        text(&out)
    );
}

#[test]
fn an_unknown_key_warns_and_only_the_three_named_rows_are_skipped() {
    let lock = "[tools]\nproptest = \"1\"\ncriterion = \"0.8\"\ncallgrind-runner = \"gungraun-runner\"\ncargo-nexttest = \"0.9.145\"\n";
    let (_scratch, out) = verify_lock("unknown-key", lock, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let said = text(&out);
    assert!(
        said.contains("unknown  cargo-nexttest: not a tool rust-tops knows, not checked"),
        "{said}"
    );
    for name in ["proptest", "criterion", "callgrind-runner"] {
        assert!(said.contains(&format!("skipped  {name}:")), "{said}");
    }
    assert!(!said.contains("skipped  cargo-nexttest"), "{said}");
}

#[test]
fn a_probe_that_fails_is_named_with_its_argv_and_how_it_failed() {
    let scratch = Scratch::new("probe-failed");
    scratch.lock("[tools]\ncargo-crap = \"0.9.0\"\n");
    scratch.script(
        "cargo-crap",
        "#!/bin/sh\necho 'cargo-crap 0.9.0'\nexit 101\n",
    );
    let out = scratch.run(&["--verify"]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    assert!(
        text(&out).contains("missing  cargo-crap: pin 0.9.0, `cargo crap --version` exited 101"),
        "{}",
        text(&out)
    );
}

#[test]
fn check_refuses_a_lock_that_cannot_be_read_as_pins() {
    let scratch = Scratch::new("check-lock");
    init(&scratch, "library-core");
    let run = |scratch: &Scratch| {
        Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
            .args(["check", "--path"])
            .arg(scratch.repo())
            .env("PATH", scratch.bin())
            .output()
            .expect("spawn cargo-tops check")
    };
    let ok = run(&scratch);
    assert_eq!(ok.status.code(), Some(0), "{}", text(&ok));
    scratch.lock("[tools]\ncargo-crap = 0.9\n");
    let out = run(&scratch);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("cargo-crap"), "{}", text(&out));
}

/// A lock written in a form `--write` will not rewrite, and the key it names.
const UNWRITABLE: [(&str, &str); 6] = [
    ("[tools]\n\"cargo-crap\" = \"0.9.0\"\n", "cargo-crap"),
    ("tools.cargo-crap = \"0.9.0\"\n", "cargo-crap"),
    ("tools = { cargo-crap = \"0.9.0\" }\n", "cargo-crap"),
    ("[tools]\ncargo-crap = '0.9.0'\n", "cargo-crap"),
    ("[tools]\ncargo-crap = \"\"\"0.9.0\"\"\"\n", "cargo-crap"),
    ("[ tools ]\ncargo-crap = \"0.9.0\"\n", "cargo-crap"),
];

#[test]
fn write_refuses_any_form_it_cannot_rewrite_and_changes_nothing() {
    for (lock, key) in UNWRITABLE {
        let scratch = Scratch::new("write-refuses");
        scratch.lock(lock);
        scratch.tool("cargo-crap", "cargo-crap 0.9.5");
        let out = scratch.run(&["--write"]);
        assert_eq!(out.status.code(), Some(2), "{lock}: {}", text(&out));
        assert!(text(&out).contains(key), "{lock}: {}", text(&out));
        assert!(!text(&out).contains("wrote"), "{lock}: {}", text(&out));
        assert_eq!(scratch.read_lock(), lock, "{lock}");
    }
}

#[test]
fn write_leaves_a_partial_pin_that_already_matches_and_prints_wrote_only_for_changes() {
    let scratch = Scratch::new("write-partial");
    let lock = "[tools]\ncargo-fuzz = \"0.12\"\ncargo-crap = \"0.9.0\"\n";
    scratch.lock(lock);
    scratch.tool("cargo-fuzz", "cargo-fuzz 0.12.3");
    scratch.tool("cargo-crap", "cargo-crap 0.9.5");
    let out = scratch.run(&["--write"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(
        scratch.read_lock(),
        "[tools]\ncargo-fuzz = \"0.12\"\ncargo-crap = \"0.9.5\"\n"
    );
    let said = text(&out);
    assert!(said.contains("kept     cargo-fuzz 0.12"), "{said}");
    assert!(
        said.contains("wrote    cargo-crap: 0.9.0 -> 0.9.5"),
        "{said}"
    );
    assert!(!said.contains("wrote    cargo-fuzz"), "{said}");
}

#[test]
fn write_never_puts_a_pre_release_into_the_lock() {
    let scratch = Scratch::new("write-pre-release");
    let lock = "[tools]\ncargo-crap = \"0.9.0\"\n";
    scratch.lock(lock);
    scratch.tool("cargo-crap", "cargo-crap 0.9.5-rc.1");
    let out = scratch.run(&["--write"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(scratch.read_lock(), lock);
}

#[test]
fn a_lock_with_no_tools_table_is_an_error_in_lock_and_in_check() {
    for header in ["[tool]", "[Tools]", "[notes]"] {
        let scratch = Scratch::new("no-tools-header");
        init(&scratch, "library-core");
        scratch.lock(&format!("{header}\ncargo-crap = \"0.9.0\"\n"));
        let check = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
            .args(["check", "--path"])
            .arg(scratch.repo())
            .env("PATH", scratch.bin())
            .output()
            .expect("spawn check");
        assert_eq!(check.status.code(), Some(2), "{header}: {}", text(&check));
        assert!(
            text(&check).contains("no [tools] table"),
            "{header}: {}",
            text(&check)
        );
        let lock = scratch.run(&["--verify"]);
        assert_eq!(lock.status.code(), Some(2), "{header}: {}", text(&lock));
    }
}

#[test]
fn check_reads_the_lock_the_way_the_gate_does() {
    let check = |scratch: &Scratch| {
        Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
            .args(["check", "--path"])
            .arg(scratch.repo())
            .env("PATH", scratch.bin())
            .output()
            .expect("spawn check")
    };
    let scratch = Scratch::new("check-reads");
    init(&scratch, "library-core");
    let lock = scratch.repo().join("rust-tops.lock");
    // No lock file pins nothing.
    fs::remove_file(&lock).expect("rm lock");
    assert_eq!(check(&scratch).status.code(), Some(0));
    // A lock that cannot be read is an error, as in the gate.
    fs::write(&lock, [0xff, 0xfe, 0x00]).expect("write bytes");
    let bytes = check(&scratch);
    assert_eq!(bytes.status.code(), Some(2), "{}", text(&bytes));
    fs::remove_file(&lock).expect("rm lock");
    fs::create_dir(&lock).expect("mkdir lock");
    let dir = check(&scratch);
    assert_eq!(dir.status.code(), Some(2), "{}", text(&dir));
}

#[test]
fn an_unknown_key_is_named_at_every_tier_and_by_write() {
    let lock = "[tools]\ncargo-nexttest = \"0.9.145\"\n";
    let scratch = Scratch::new("unknown-tier");
    init(&scratch, "library-core");
    scratch.lock(lock);
    let out = scratch.run(&["--verify", "--max-tier", "1"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(
        text(&out).contains("unknown  cargo-nexttest: not a tool rust-tops knows, not checked"),
        "{}",
        text(&out)
    );
    let out = scratch.run(&["--write"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(
        text(&out).contains("unknown  cargo-nexttest"),
        "{}",
        text(&out)
    );
    assert_eq!(scratch.read_lock(), lock);
}
