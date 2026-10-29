//! Canonical JSON of `TopsConfig` for every bundled profile.
//!
//! Recorded from main `968c390` by decoding with serde_yaml 0.9.34 into this
//! struct (free-form fields as `serde_json::Value`) and sorting object keys.
//! `crate.msrv` is not a field, so it is absent. The templates match that
//! commit; `rust-tops.yaml` there differs from this branch only in `msrv`.
//!
//! `RUST_TOPS_RECORD_GOLDEN` rewrites these files for a local re-record.
//! CI does not set it.
use std::fs;
use std::path::PathBuf;

use super::config::TopsConfig;
use super::{check, load_config};

const CLASSES: &[&str] = &[
    "library-core",
    "parser-codec",
    "no-std-embedded",
    "unsafe-kernel",
    "binary-cli",
];

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/config")
}

fn canonical(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(canonical).collect())
        }
        serde_json::Value::Object(map) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for key in keys {
                let child = canonical(map[&key].clone());
                out.insert(key, child);
            }
            serde_json::Value::Object(out)
        }
        other => other,
    }
}

fn config_json(cfg: &TopsConfig) -> serde_json::Value {
    canonical(serde_json::to_value(cfg).expect("config serializes to json"))
}

fn load_yaml(label: &str, yaml: &str) -> TopsConfig {
    let dir = std::env::temp_dir().join(format!("cargo-tops-parse-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("mkdir {label}: {err}"));
    fs::write(dir.join("rust-tops.yaml"), yaml)
        .unwrap_or_else(|err| panic!("write {label}: {err}"));
    let cfg = load_config(&dir).unwrap_or_else(|err| panic!("parse {label}: {err:#}"));
    check(&cfg).unwrap_or_else(|err| panic!("check {label}: {err:#}"));
    let _ = fs::remove_dir_all(&dir);
    cfg
}

/// Protocol 1.1.0 changed the bundled template in two places: it moved
/// `protocol.version` and added an enabled `laws` layer. Check both, then undo
/// them so the rest still compares to the `968c390` serde_yaml oracle.
fn without_protocol_1_1(mut value: serde_json::Value) -> serde_json::Value {
    assert_eq!(value["protocol"]["version"], "1.1.0");
    value["protocol"]["version"] = serde_json::Value::from("1.0.0");
    let layers = value["layers"].as_array_mut().expect("layers array");
    let laws: Vec<serde_json::Value> = layers
        .iter()
        .filter(|layer| layer["id"] == "laws")
        .cloned()
        .collect();
    assert_eq!(
        laws,
        [serde_json::json!({"enabled": true, "id": "laws", "runner": "bend", "tier": 0})],
        "template must carry one enabled tier-0 laws layer"
    );
    layers.retain(|layer| layer["id"] != "laws");
    value
}

/// The unmapped-layers pack turned `acceptance` off in the bundled template:
/// it has no command, and an enabled layer with none fails the gate. Check
/// that, then undo it so the rest still compares to the `968c390` oracle.
fn with_acceptance_restored(mut value: serde_json::Value) -> serde_json::Value {
    let layers = value["layers"].as_array_mut().expect("layers array");
    let acceptance = layers
        .iter_mut()
        .find(|layer| layer["id"] == "acceptance")
        .expect("template has an acceptance layer");
    assert_eq!(
        acceptance["enabled"], false,
        "template must write acceptance off"
    );
    acceptance["enabled"] = serde_json::Value::from(true);
    value
}

fn expect_config(name: &str, cfg: &TopsConfig) {
    expect_value(name, config_json(cfg));
}

fn expect_value(name: &str, value: serde_json::Value) {
    let path = golden_dir().join(format!("{name}.json"));
    if std::env::var_os("RUST_TOPS_RECORD_GOLDEN").is_some() {
        fs::create_dir_all(golden_dir()).unwrap_or_else(|err| panic!("mkdir golden: {err}"));
        let pretty = serde_json::to_string_pretty(&value).expect("pretty json") + "\n";
        fs::write(&path, pretty).unwrap_or_else(|err| panic!("write {}: {err}", path.display()));
        return;
    }
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("missing golden {}: {err}", path.display()));
    let expected_value: serde_json::Value =
        serde_json::from_str(&expected).unwrap_or_else(|err| panic!("golden {name} json: {err}"));
    assert_eq!(
        value, expected_value,
        "bundled profile {name} no longer parses to the pre-migration config"
    );
}

#[test]
fn bundled_profile_templates_parse_to_pre_migration_config() {
    for class in CLASSES {
        let yaml = super::profiles::profile_yaml(class, "golden-crate");
        let cfg = load_yaml(class, &yaml);
        assert_eq!(cfg.crate_.name, "golden-crate");
        assert_eq!(cfg.crate_.class, *class);
        assert_eq!(cfg.protocol.id, "rust-tops");
        expect_value(
            class,
            with_acceptance_restored(without_protocol_1_1(config_json(&cfg))),
        );
    }
}

#[test]
fn workspace_instance_parses_to_pre_migration_config() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let instance = root.join("rust-tops.yaml");
    assert!(
        instance.is_file(),
        "workspace golden requires {}",
        instance.display()
    );
    let cfg = load_config(&root).unwrap_or_else(|err| panic!("parse workspace instance: {err:#}"));
    check(&cfg).unwrap_or_else(|err| panic!("check workspace instance: {err:#}"));
    assert_eq!(cfg.crate_.name, "cargo-tops");
    assert_eq!(cfg.crate_.class, "binary-cli");
    expect_config("workspace", &cfg);
}

const YAML11_BOOL_WORDS: &[&str] = &["yes", "no", "on", "off", "y", "n"];

fn strict_bool_yaml() -> String {
    include_str!("../tests/golden/strict-bool/instance.yaml").to_string()
}

/// Pins `strict_booleans`. This file is the serde-saphyr decode, not the
/// `968c390` serde_yaml oracle above.
#[test]
fn strict_booleans_keep_yaml11_words_as_strings() {
    let cfg = load_yaml("strict-bool", &strict_bool_yaml());
    let goals = config_json(&cfg)
        .get("goals")
        .cloned()
        .expect("goals present");
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/strict-bool/goals.json");
    let expected: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display())),
    )
    .expect("strict-bool goals golden");
    assert_eq!(goals, expected);
    assert!(cfg.layers[0].enabled);
}

#[test]
fn strict_booleans_reject_yaml11_words_on_typed_bool() {
    let base = strict_bool_yaml();
    for word in YAML11_BOOL_WORDS {
        let yaml = base.replace("enabled: true", &format!("enabled: {word}"));
        let dir =
            std::env::temp_dir().join(format!("cargo-tops-bool-{word}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("mkdir {word}: {err}"));
        fs::write(dir.join("rust-tops.yaml"), yaml)
            .unwrap_or_else(|err| panic!("write {word}: {err}"));
        let err = match load_config(&dir) {
            Err(err) => err,
            Ok(_) => panic!("typed bool accepted {word}"),
        };
        let message = format!("{err:#}");
        assert!(
            message.contains("invalid boolean"),
            "enabled: {word} did not reject as a strict bool: {message}"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
