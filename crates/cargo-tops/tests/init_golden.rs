//! `cargo tops init` file bytes and stdout, recorded from main `968c390`.
//! Re-recorded for protocol 1.1.0 / package 0.2.0: the `laws` layer and
//! version in `rust-tops.yaml`, the Bend kit (`laws/bend.lock`,
//! `scripts/fetch-bend.sh`), `rust-toolchain.toml`, and the per-class
//! workflow (drop-in floor plus the jobs §4 requires).
//! Re-recorded for the drop-in pack: `laws` off (D3) and the drop-in
//! `tops-gate` job.
//! Re-recorded for protocol 1.2.0 / package 0.4.0: `RUST-TOPS.md`, the
//! adoption record filled per class, the `protocol.version` and the
//! cargo-tops install pin.
//! Re-recorded for protocol 1.4.0 / package 0.5.0: `RUST-TOPS.md` spec tag
//! and `cargo-tops` install pin updated to `v0.5.0` / `0.5.0`.
//! Re-recorded for protocol 1.5.0 (G3): `rust-tops.yaml` sets `enforce: ratchet`
//! and `protocol.version: "1.5.0"`, `rust-tops.lock` `notes.schema = "1.5.0"`,
//! and `AGENTS.md` documents `enforce` and `cargo tops baseline`.
//!
//! Re-recorded for P0 G2: `init` reads the crate it runs in (name, edition,
//! `rust-version`, the lint table), writes `rust-tops.lock`, writes no Bend
//! file without `--laws`, and writes no toolchain file over a pin. The golden
//! crate is a 2021-edition package with `rust-version = "1.95"` that forbids
//! unsafe code.
//!
//! `RUST_TOPS_BLESS` rewrites these files for a local re-record.
//! CI does not set it.
#![expect(clippy::panic, reason = "this crate already violates the strict table")]
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CLASSES: &[&str] = &[
    "library-core",
    "parser-codec",
    "no-std-embedded",
    "unsafe-kernel",
    "binary-cli",
    "library-util",
    "ffi-bridge",
    "workspace-root",
];

/// Files whose bytes depend on the class.
const PER_CLASS: &[&str] = &[
    "rust-tops.yaml",
    "rust-toolchain.toml",
    "clippy.toml",
    ".github/workflows/rust-tops.yml",
    "RUST-TOPS.md",
];

const SHARED: &[&str] = &[
    "AGENTS.md",
    ".config/nextest.toml",
    ".cargo/mutants.toml",
    ".cargo-crap.toml",
    "deny.toml",
    "rust-tops.lock",
];

/// The Bend kit, written only by `init --laws`.
const KIT: &[&str] = &["LAWS.bend", "laws/bend.lock", "scripts/fetch-bend.sh"];

const CRATE_2021: &str =
    "[package]\nname = \"k\"\nversion = \"0.0.0\"\nedition = \"2021\"\nrust-version = \"1.95\"\n";
const GOLDEN_MANIFEST: &str = "[package]\nname = \"golden-crate\"\nversion = \"0.0.0\"\nedition = \"2021\"\nrust-version = \"1.95\"\n\n[lints.rust]\nunsafe_code = \"forbid\"\n";

fn golden_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/init")
}

fn recording() -> bool {
    std::env::var_os("RUST_TOPS_BLESS").is_some()
}

/// A package at `dir` with `manifest` as its `Cargo.toml`.
fn write_crate(dir: &Path, manifest: &str) {
    fs::create_dir_all(dir.join("src")).expect("mkdir src");
    fs::write(dir.join("Cargo.toml"), manifest).expect("write Cargo.toml");
    fs::write(dir.join("src/lib.rs"), "// fixture\n").expect("write lib.rs");
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cargo-tops-init-{tag}-{}", std::process::id()));
    #[expect(
        clippy::let_underscore_must_use,
        reason = "already in this crate when the strict table was turned on"
    )]
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// `cargo-tops <args>`: status, stdout, stderr.
fn tops(args: &[&str], dir: &Path) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(args)
        .arg("--path")
        .arg(dir)
        .output()
        .unwrap_or_else(|err| panic!("spawn cargo-tops {args:?}: {err}"));
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn run_init(class: &str) -> (PathBuf, String) {
    let dir = scratch(class);
    write_crate(&dir, GOLDEN_MANIFEST);
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(["init", "--class", class, "--name", "golden-crate", "--path"])
        .arg(&dir)
        .output()
        .unwrap_or_else(|err| panic!("spawn cargo-tops init: {err}"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "init {class} exited {}: {stderr}",
        output.status
    );
    assert!(stderr.is_empty(), "init {class} stderr: {stderr}");
    let stdout = String::from_utf8(output.stdout).expect("init stdout is utf-8");
    let rendered = dir.display().to_string();
    let stdout = stdout.replace(&rendered, "{path}");
    assert!(
        stdout.contains("{path}/rust-tops.yaml"),
        "stdout did not contain the init path for {class}:\n{stdout}"
    );
    assert!(
        !stdout.contains(&rendered),
        "stdout still contains the absolute init path for {class}"
    );
    (dir, stdout)
}

fn expect_bytes(golden: &Path, actual: &[u8]) {
    if recording() {
        if let Some(parent) = golden.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|err| panic!("mkdir {}: {err}", parent.display()));
        }
        fs::write(golden, actual).unwrap_or_else(|err| panic!("write {}: {err}", golden.display()));
        return;
    }
    let expected =
        fs::read(golden).unwrap_or_else(|err| panic!("read {}: {err}", golden.display()));
    if actual == expected.as_slice() {
        return;
    }
    let at = actual
        .iter()
        .zip(expected.iter())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| actual.len().min(expected.len()));
    panic!(
        "{} differs from init output (golden {} bytes, actual {} bytes, first byte {at})",
        golden.display(),
        expected.len(),
        actual.len()
    );
}

fn expect_text(golden: &Path, actual: &str) {
    expect_bytes(golden, actual.as_bytes());
}

#[test]
fn init_output_matches_main_bytes() {
    for class in CLASSES {
        let (dir, stdout) = run_init(class);
        expect_text(&golden_root().join(class).join("stdout.txt"), &stdout);
        for rel in PER_CLASS {
            let bytes =
                fs::read(dir.join(rel)).unwrap_or_else(|err| panic!("read {class} {rel}: {err}"));
            expect_bytes(&golden_root().join(class).join(rel), &bytes);
        }
        for rel in SHARED {
            let bytes =
                fs::read(dir.join(rel)).unwrap_or_else(|err| panic!("read {class} {rel}: {err}"));
            expect_bytes(&golden_root().join("shared").join(rel), &bytes);
        }
        #[expect(
            clippy::let_underscore_must_use,
            reason = "already in this crate when the strict table was turned on"
        )]
        let _ = fs::remove_dir_all(&dir);
    }
}

/// The value at JSON pointer `pointer`, or null.
fn at(doc: &serde_json::Value, pointer: &str) -> serde_json::Value {
    doc.pointer(pointer)
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

/// Whether the `laws` layer is on.
fn laws_enabled(doc: &serde_json::Value) -> bool {
    let layers = doc.pointer("/layers").and_then(serde_json::Value::as_array);
    layers.into_iter().flatten().any(|layer| {
        layer.get("id") == Some(&"laws".into()) && layer.get("enabled") == Some(&true.into())
    })
}

fn yaml_of(dir: &Path) -> serde_json::Value {
    let raw = fs::read_to_string(dir.join("rust-tops.yaml")).expect("rust-tops.yaml");
    serde_saphyr::from_str(&raw).expect("rust-tops.yaml parses")
}

#[test]
fn init_takes_name_edition_and_msrv_from_the_crate_and_then_check_passes_for_every_class() {
    for class in CLASSES {
        let dir = scratch(&format!("reads-{class}"));
        write_crate(
            &dir,
            "[package]\nname = \"reader\"\nversion = \"0.0.0\"\nedition = \"2021\"\nrust-version = \"1.95\"\n",
        );
        let (ok, out, err) = tops(&["init", "--class", class], &dir);
        assert!(ok, "init {class}: {out}{err}");
        let doc = yaml_of(&dir);
        assert_eq!(at(&doc, "/crate/name"), "reader", "{class}");
        assert_eq!(at(&doc, "/crate/edition"), "2021", "{class}");
        assert_eq!(at(&doc, "/crate/msrv"), "1.95", "{class}");
        assert_eq!(
            at(&doc, "/crate/hot_paths"),
            serde_json::json!([]),
            "{class}"
        );
        assert!(!out.contains("crate.msrv"), "{class}: {out}");
        assert!(!out.contains("crate.edition"), "{class}: {out}");
        let toolchain = fs::read_to_string(dir.join("rust-toolchain.toml")).expect("toolchain");
        assert!(
            toolchain.contains("channel = \"1.95\""),
            "{class}: {toolchain}"
        );
        for kit in KIT {
            assert!(!dir.join(kit).exists(), "{class}: plain init wrote {kit}");
            assert!(!out.contains(kit), "{class}: stdout names {kit}");
        }
        assert!(!laws_enabled(&doc), "{class}");
        assert!(dir.join("rust-tops.lock").is_file(), "{class}");
        let (ok, out, err) = tops(&["check"], &dir);
        assert!(ok, "check {class}: {out}{err}");
        fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn the_lint_table_sets_unsafe_policy_and_a_class_policy_or_forbid_stands_in_with_a_note() {
    let plain = "[package]\nname = \"p\"\nversion = \"0.0.0\"\nedition = \"2024\"\nrust-version = \"1.95\"\n";
    let cases = [
        (
            "unsafe-kernel",
            format!("{plain}[lints.rust]\nunsafe_code = \"forbid\"\n"),
            "forbid",
            false,
        ),
        ("unsafe-kernel", plain.to_string(), "required-kernel", true),
        ("library-core", plain.to_string(), "forbid", true),
        (
            "library-core",
            format!("{plain}[lints.rust]\nunsafe_code = \"deny\"\n"),
            "forbid",
            true,
        ),
    ];
    for (class, manifest, policy, noted) in cases {
        let dir = scratch("unsafe");
        write_crate(&dir, &manifest);
        let (ok, out, err) = tops(&["init", "--class", class], &dir);
        assert!(ok, "{out}{err}");
        assert_eq!(
            at(&yaml_of(&dir), "/crate/unsafe_policy"),
            policy,
            "{class}"
        );
        assert_eq!(out.contains("note: unsafe_policy"), noted, "{class}: {out}");
        fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn an_existing_toolchain_pin_in_the_crate_or_above_it_stays_byte_identical() {
    let manifest = "[package]\nname = \"m\"\nversion = \"0.0.0\"\nedition = \"2021\"\nrust-version = \"1.95\"\n";
    let pin = b"[toolchain]\nchannel = \"1.96.0\"\n# mine\n";
    let dir = scratch("pin-own");
    write_crate(&dir, manifest);
    fs::write(dir.join("rust-toolchain.toml"), pin).expect("pin");
    let (ok, out, err) = tops(&["init"], &dir);
    assert!(ok, "{out}{err}");
    assert_eq!(
        fs::read(dir.join("rust-toolchain.toml")).expect("read"),
        pin
    );
    assert!(
        out.contains("note: rust-toolchain.toml not written"),
        "{out}"
    );
    fs::remove_dir_all(&dir).ok();

    for legacy in ["rust-toolchain.toml", "rust-toolchain"] {
        let root = scratch(&format!("pin-root-{legacy}"));
        fs::create_dir_all(&root).expect("mkdir");
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/m\"]\nresolver = \"2\"\n",
        )
        .expect("ws");
        fs::write(root.join(legacy), pin).expect("pin");
        let member = root.join("crates/m");
        write_crate(&member, manifest);
        let (ok, out, err) = tops(&["init"], &member);
        assert!(ok, "{out}{err}");
        assert!(
            !member.join("rust-toolchain.toml").exists(),
            "{legacy}: written below a pin"
        );
        assert_eq!(fs::read(root.join(legacy)).expect("read"), pin, "{legacy}");
        fs::remove_dir_all(&root).ok();
    }
}

#[test]
fn a_toolchain_pin_above_the_workspace_root_is_not_a_pin_for_this_crate() {
    let outer = scratch("pin-above");
    fs::create_dir_all(&outer).expect("mkdir");
    fs::write(
        outer.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.96.0\"\n",
    )
    .expect("pin");
    let root = outer.join("ws");
    fs::create_dir_all(&root).expect("mkdir");
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"k\"]\nresolver = \"2\"\n",
    )
    .expect("ws");
    let krate = root.join("k");
    write_crate(&krate, CRATE_2021);
    let (ok, out, err) = tops(&["init"], &krate);
    assert!(ok, "{out}{err}");
    assert!(krate.join("rust-toolchain.toml").is_file());
    fs::remove_dir_all(&outer).ok();
}

#[test]
fn a_toolchain_pin_two_levels_above_a_crate_with_no_workspace_is_honoured() {
    let outer = scratch("pin-two-up");
    fs::create_dir_all(&outer).expect("mkdir");
    let pin = b"[toolchain]\nchannel = \"1.96.0\"\n";
    fs::write(outer.join("rust-toolchain.toml"), pin).expect("pin");
    // The git root fences the walk, so a pin outside the scratch tree
    // cannot reach this fixture.
    fs::create_dir_all(outer.join(".git")).expect("git");
    let krate = outer.join("a/k");
    write_crate(&krate, CRATE_2021);
    let (ok, out, err) = tops(&["init"], &krate);
    assert!(ok, "{out}{err}");
    assert!(!krate.join("rust-toolchain.toml").exists());
    assert!(!outer.join("a/rust-toolchain.toml").exists());
    assert_eq!(
        fs::read(outer.join("rust-toolchain.toml")).expect("read"),
        pin
    );
    assert!(
        out.contains("note: rust-toolchain.toml not written"),
        "{out}"
    );
    fs::remove_dir_all(&outer).ok();
}

#[test]
fn a_bare_directory_takes_defaults_and_names_each_one() {
    let dir = scratch("bare");
    fs::create_dir_all(&dir).expect("mkdir");
    let (ok, out, err) = tops(&["init", "--class", "binary-cli"], &dir);
    assert!(ok, "{out}{err}");
    let base = dir
        .file_name()
        .expect("name")
        .to_string_lossy()
        .into_owned();
    let notes: Vec<&str> = out.lines().filter(|l| l.starts_with("note:")).collect();
    let [name, edition, msrv, policy] = notes.as_slice() else {
        panic!("want four notes: {out}");
    };
    assert!(name.contains(&format!("crate.name {base}")), "{out}");
    assert!(edition.contains("crate.edition 2024"), "{out}");
    assert!(msrv.contains("crate.msrv 1.99.1"), "{out}");
    assert!(policy.contains("unsafe_policy forbid"), "{out}");
    let doc = yaml_of(&dir);
    assert_eq!(at(&doc, "/crate/name"), base.as_str());
    assert_eq!(at(&doc, "/crate/msrv"), "1.99.1");
    assert_eq!(at(&doc, "/crate/hot_paths"), serde_json::json!([]));
    for stale in ["varint", "1.88"] {
        assert!(
            !fs::read_to_string(dir.join("rust-tops.yaml"))
                .expect("yaml")
                .contains(stale),
            "{stale}"
        );
    }
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn laws_writes_the_kit_and_turns_the_layer_on_and_plain_init_does_neither() {
    let dir = scratch("laws");
    write_crate(&dir, GOLDEN_MANIFEST);
    let (ok, out, err) = tops(&["init", "--laws"], &dir);
    assert!(ok, "{out}{err}");
    for kit in KIT {
        assert!(dir.join(kit).is_file(), "init --laws did not write {kit}");
        assert!(out.contains(kit), "stdout lacks {kit}: {out}");
        expect_bytes(
            &golden_root().join("shared").join(kit),
            &fs::read(dir.join(kit)).expect("kit"),
        );
    }
    assert!(laws_enabled(&yaml_of(&dir)));
    let (ok, out, err) = tops(&["check"], &dir);
    assert!(ok, "{out}{err}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_manifest_cargo_cannot_read_is_an_error_and_writes_nothing() {
    let dir = scratch("broken");
    fs::create_dir_all(&dir).expect("mkdir");
    fs::write(dir.join("Cargo.toml"), "not toml [").expect("write");
    let (ok, out, err) = tops(&["init"], &dir);
    assert!(!ok, "{out}");
    assert!(err.contains("cargo metadata failed"), "{err}");
    assert!(!dir.join("rust-tops.yaml").exists());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn force_never_rewrites_an_existing_toolchain_pin() {
    let manifest = "[package]\nname = \"f\"\nversion = \"0.0.0\"\nedition = \"2021\"\nrust-version = \"1.95\"\n";
    let pin = b"[toolchain]\nchannel = \"1.96.0\"\n# mine\n";
    for file in ["rust-toolchain.toml", "rust-toolchain"] {
        let dir = scratch(&format!("force-{file}"));
        write_crate(&dir, manifest);
        fs::write(dir.join(file), pin).expect("pin");
        for flags in [&["init"][..], &["init", "--force"]] {
            let (ok, out, err) = tops(flags, &dir);
            assert!(ok, "{file} {flags:?}: {out}{err}");
            assert_eq!(
                fs::read(dir.join(file)).expect("read"),
                pin,
                "{file} {flags:?}"
            );
            let other = if file == "rust-toolchain" {
                "rust-toolchain.toml"
            } else {
                "rust-toolchain"
            };
            assert!(!dir.join(other).exists(), "{file} {flags:?}: wrote {other}");
        }
        // `--force` still rewrites a file init owns.
        fs::write(dir.join("clippy.toml"), "stale\n").expect("write");
        let (ok, out, err) = tops(&["init", "--force"], &dir);
        assert!(ok, "{out}{err}");
        assert_ne!(
            fs::read_to_string(dir.join("clippy.toml")).expect("read"),
            "stale\n"
        );
        fs::remove_dir_all(&dir).ok();
    }
}
