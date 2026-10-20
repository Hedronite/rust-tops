//! Parsed `TopsConfig` for every bundled profile, frozen to the serde_yaml
//! decode from main before the serde-saphyr migration.
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

fn expect_config(name: &str, cfg: &TopsConfig) {
    let value = config_json(cfg);
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
        expect_config(class, &cfg);
    }
}

#[test]
fn workspace_instance_parses_to_pre_migration_config() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let instance = root.join("rust-tops.yaml");
    if !instance.is_file() {
        eprintln!(
            "workspace_instance_parses_to_pre_migration_config: skipped, {} is outside the packaged crate",
            instance.display()
        );
        return;
    }
    let cfg = load_config(&root).unwrap_or_else(|err| panic!("parse workspace instance: {err:#}"));
    check(&cfg).unwrap_or_else(|err| panic!("check workspace instance: {err:#}"));
    assert_eq!(cfg.crate_.name, "cargo-tops");
    assert_eq!(cfg.crate_.class, "binary-cli");
    expect_config("workspace", &cfg);
}
