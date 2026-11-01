//! `cargo tops init` writes each class profile's numbers as data, and
//! `cargo tops check` refuses a class with no profile and a schema-invalid
//! instance. Every test drives the built `cargo-tops` binary.
#![expect(clippy::panic, reason = "this crate already violates the strict table")]
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("cargo-tops-profiles-{name}-{}", std::process::id()));
    #[expect(
        clippy::let_underscore_must_use,
        reason = "already in this crate when the strict table was turned on"
    )]
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("mkdir {}: {err}", dir.display()));
    dir
}

fn cargo_tops(args: &[&str], path: &Path) -> (Output, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-tops"))
        .args(args)
        .arg("--path")
        .arg(path)
        .output()
        .expect("spawn cargo-tops");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out, text)
}

fn init(class: &str, repo: &Path) -> Value {
    let (out, text) = cargo_tops(&["init", "--class", class, "--name", "fixture"], repo);
    assert!(out.status.success(), "init {class}: {text}");
    read_yaml(repo)
}

fn read_yaml(repo: &Path) -> Value {
    let raw = fs::read_to_string(repo.join("rust-tops.yaml")).expect("read rust-tops.yaml");
    serde_saphyr::from_str(&raw).expect("rust-tops.yaml parses")
}

/// Writes `doc` back as JSON, which is YAML.
fn write_yaml(repo: &Path, doc: &Value) {
    let json = serde_json::to_string_pretty(doc).expect("json");
    #[expect(
        clippy::string_add,
        reason = "already in this crate when the strict table was turned on"
    )]
    fs::write(repo.join("rust-tops.yaml"), json + "\n").expect("write rust-tops.yaml");
}

fn check(repo: &Path) -> (Output, String) {
    cargo_tops(&["check"], repo)
}

#[test]
#[expect(clippy::too_many_lines, reason = "over the kit line threshold")]
fn init_writes_each_profile_s_numbers() {
    let root = scratch("numbers");
    // class, line, region, human CRAP, agent CRAP (RUST_TOPS.md §4).
    for (class, line, region, human, agent) in [
        ("binary-cli", 80, 75, 6, 8),
        ("library-util", 90, 85, 4, 6),
        ("ffi-bridge", 90, 85, 6, 8),
        ("library-core", 95, 90, 4, 6),
    ] {
        let repo = root.join(class);
        let doc = init(class, &repo);
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_79_20 = doc["crate"]["class"].clone();
        assert_eq!(tops_idx_79_20, class);
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let gates = &doc["gates"];
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_90_13 = gates["coverage"]["line_min_pct"].clone();
        assert_eq!(tops_idx_90_13, json!(line), "{class} line");
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_99_13 = gates["coverage"]["region_min_pct"].clone();
        assert_eq!(tops_idx_99_13, json!(region), "{class} region");
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_108_13 = gates["crap"]["threshold_human"].clone();
        assert_eq!(tops_idx_108_13, json!(human), "{class} human");
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_117_13 = gates["crap"]["threshold_agent"].clone();
        assert_eq!(tops_idx_117_13, json!(agent), "{class} agent");
        let (out, text) = check(&repo);
        assert!(out.status.success(), "check {class}: {text}");
    }
    #[expect(
        clippy::let_underscore_must_use,
        reason = "already in this crate when the strict table was turned on"
    )]
    let _ = fs::remove_dir_all(&root);
}

#[expect(clippy::too_many_lines, reason = "over the kit line threshold")]
#[test]
fn workspace_root_inits_without_library_core_floors() {
    let root = scratch("workspace-root");
    let doc = init("workspace-root", &root);
    #[expect(
        clippy::indexing_slicing,
        reason = "already in this crate when the strict table was turned on"
    )]
    let gates = &doc["gates"];
    #[expect(
        clippy::indexing_slicing,
        reason = "already in this crate when the strict table was turned on"
    )]
    let tops_idx_144_16 = doc["crate"]["class"].clone();
    assert_eq!(tops_idx_144_16, "workspace-root");
    for (gate, key) in [
        ("crap", "threshold_agent"),
        ("crap", "threshold_human"),
        ("coverage", "line_min_pct"),
        ("coverage", "region_min_pct"),
        ("mutation", "kill_rate_min_pct"),
    ] {
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_157_13 = gates[gate][key].clone();
        assert_eq!(
            tops_idx_157_13,
            Value::Null,
            "workspace-root kept {gate}.{key}"
        );
    }
    let raw = fs::read_to_string(root.join("rust-tops.yaml")).expect("yaml");
    assert!(!raw.contains("threshold_agent: 6"), "{raw}");
    assert!(!raw.contains("line_min_pct: 95"), "{raw}");
    #[expect(
        clippy::indexing_slicing,
        reason = "already in this crate when the strict table was turned on"
    )]
    let tops_idx_169_16 = doc["exemptions"].clone();
    assert_eq!(tops_idx_169_16, json!([]));
    let (out, text) = check(&root);
    assert!(out.status.success(), "check workspace-root: {text}");
    #[expect(
        clippy::let_underscore_must_use,
        reason = "already in this crate when the strict table was turned on"
    )]
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_unknown_class_fails_init_and_writes_nothing() {
    let root = scratch("unknown-init");
    let repo = root.join("repo");
    let (out, text) = cargo_tops(&["init", "--class", "not-a-class"], &repo);
    assert!(!out.status.success(), "init accepted not-a-class: {text}");
    assert!(
        text.contains("not-a-class"),
        "error does not name the class: {text}"
    );
    assert!(
        !repo.exists(),
        "init wrote {} for an unknown class",
        repo.display()
    );

    // An existing directory is left as it was.
    fs::create_dir_all(&repo).expect("mkdir");
    let (out, text) = cargo_tops(&["init", "--class", "not-a-class"], &repo);
    assert!(!out.status.success(), "{text}");
    assert_eq!(
        fs::read_dir(&repo).expect("ls").count(),
        0,
        "init wrote files"
    );
    #[expect(
        clippy::let_underscore_must_use,
        reason = "already in this crate when the strict table was turned on"
    )]
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn check_fails_a_class_with_no_profile() {
    let root = scratch("unknown-check");
    let mut doc = init("library-core", &root);
    // `binary-service` is in the schema's class enum but has no profile.
    for class in ["not-a-class", "binary-service"] {
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_slot_220 = &mut doc["crate"]["class"];
        *tops_slot_220 = json!(class);
        write_yaml(&root, &doc);
        let (out, text) = check(&root);
        assert!(!out.status.success(), "check accepted {class}: {text}");
        assert!(
            text.contains(&format!("no profile for class {class}")),
            "{text}"
        );
    }
    #[expect(
        clippy::let_underscore_must_use,
        reason = "already in this crate when the strict table was turned on"
    )]
    let _ = fs::remove_dir_all(&root);
}

/// Breaks one schema rule in a valid instance.
type Breaker = fn(&mut Value);
#[expect(clippy::too_many_lines, reason = "over the kit line threshold")]
#[test]
fn check_fails_a_schema_invalid_instance() {
    let root = scratch("schema");
    let fresh = init("library-core", &root);
    let (out, text) = check(&root);
    assert!(out.status.success(), "fresh init fails check: {text}");
    let cases: [(&str, Breaker); 4] = [
        ("/crate/edition", |doc| {
            #[expect(
                clippy::indexing_slicing,
                reason = "already in this crate when the strict table was turned on"
            )]
            let tops_slot_251 = &mut doc["crate"]["edition"];
            *tops_slot_251 = json!("2015")
        }),
        ("key surprise is not allowed", |doc| {
            doc["surprise"] = json!(1)
        }),
        ("/gates/coverage/line_min_pct", |doc| {
            #[expect(
                clippy::indexing_slicing,
                reason = "already in this crate when the strict table was turned on"
            )]
            let tops_slot_261 = &mut doc["gates"]["coverage"]["line_min_pct"];
            *tops_slot_261 = json!(101)
        }),
        ("missing required key exemptions", |doc| {
            doc.as_object_mut().expect("map").remove("exemptions");
        }),
    ];
    for (want, break_it) in cases {
        let mut doc = fresh.clone();
        break_it(&mut doc);
        write_yaml(&root, &doc);
        let (out, text) = check(&root);
        assert!(!out.status.success(), "check accepted {want}: {text}");
        assert!(
            text.contains("does not match schema/rust-tops.schema.json"),
            "{text}"
        );
        assert!(text.contains(want), "{want}: {text}");
        // gate runs check first, so it refuses the instance too.
        let (out, text) = cargo_tops(&["gate", "--dry-run"], &root);
        assert_eq!(out.status.code(), Some(2), "gate on {want}: {text}");
    }
    #[expect(
        clippy::let_underscore_must_use,
        reason = "already in this crate when the strict table was turned on"
    )]
    let _ = fs::remove_dir_all(&root);
}
