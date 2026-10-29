//! Class profiles merged onto the init template as data.
//!
//! Every file in `profiles/` is embedded with `include_str!`. [`merge_profile`]
//! parses the base template and the class file and writes the profile's
//! numbers into the template tree. It does not search-and-replace template
//! text. An unknown class is an error, not a library-core document.
//!
//! `profiles.rs` is the caller. This allow holds until that call lands.
#![allow(dead_code, reason = "profiles.rs calls merge_profile")]

use serde_json::{Map, Number, Value};

const TEMPLATE: &str = include_str!("../templates/rust-tops.example.yaml");

/// Every `profiles/*.yaml`, keyed by the class id in the file.
const PROFILES: &[(&str, &str)] = &[
    (
        "binary-cli",
        include_str!("../../../profiles/binary-cli.yaml"),
    ),
    (
        "ffi-bridge",
        include_str!("../../../profiles/ffi-bridge.yaml"),
    ),
    (
        "library-core",
        include_str!("../../../profiles/library-core.yaml"),
    ),
    (
        "library-util",
        include_str!("../../../profiles/library-util.yaml"),
    ),
    (
        "no-std-embedded",
        include_str!("../../../profiles/no-std-embedded.yaml"),
    ),
    (
        "parser-codec",
        include_str!("../../../profiles/parser-codec.yaml"),
    ),
    (
        "unsafe-kernel",
        include_str!("../../../profiles/unsafe-kernel.yaml"),
    ),
    (
        "workspace-root",
        include_str!("../../../profiles/workspace-root.yaml"),
    ),
];

const COVERAGE_FLOORS: &[&str] = &[
    "line_min_pct",
    "region_min_pct",
    "per_file_line_min_pct",
    "branch_min_pct",
];
const CRAP_FLOORS: &[&str] = &["threshold_agent", "threshold_human", "threshold_production"];
const MUTATION_FLOORS: &[&str] = &["kill_rate_min_pct"];

/// Why [`merge_profile`] produced no document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileError {
    /// No embedded profile for this class. Library-core is not a fallback.
    UnknownClass { class: String },
    /// The profile or the template could not be merged.
    Invalid { class: String, detail: String },
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownClass { class } => write!(f, "no profile for class {class}"),
            Self::Invalid { class, detail } => write!(f, "profile {class} is invalid: {detail}"),
        }
    }
}

impl std::error::Error for ProfileError {}

/// `rust-tops.yaml` for `class`, with `crate.name` set to `crate_name`.
///
/// Coverage, CRAP, and mutation numbers come from the profile file and land
/// on `gates`. `inherits: per-package` drops the template's package floors,
/// so a workspace root does not keep library-core's agent CRAP or line floor.
/// Call this from `init` and `check`. Do not fall back when it returns
/// [`ProfileError::UnknownClass`].
#[must_use = "an unknown class is an error and must not be written as library-core"]
pub fn merge_profile(class: &str, crate_name: &str) -> Result<String, ProfileError> {
    let source = profile_source(class).ok_or_else(|| ProfileError::UnknownClass {
        class: class.to_string(),
    })?;
    let mut doc = parse_yaml(TEMPLATE).map_err(|detail| ProfileError::Invalid {
        class: class.to_string(),
        detail,
    })?;
    let profile = parse_yaml(source).map_err(|detail| ProfileError::Invalid {
        class: class.to_string(),
        detail,
    })?;
    apply_profile(&mut doc, &profile, class, crate_name)?;
    Ok(emit_yaml(&doc))
}

fn profile_source(class: &str) -> Option<&'static str> {
    PROFILES
        .iter()
        .find(|(name, _)| *name == class)
        .map(|(_, body)| *body)
}

fn apply_profile(
    doc: &mut Value,
    profile: &Value,
    class: &str,
    crate_name: &str,
) -> Result<(), ProfileError> {
    let declared = profile.get("class").and_then(Value::as_str).unwrap_or("");
    if declared != class {
        return Err(invalid(class, format!("file says class {declared}")));
    }
    set_crate(doc, class, crate_name, profile)?;
    for (key, pointer) in [
        ("coverage", "/gates/coverage"),
        ("crap", "/gates/crap"),
        ("mutation", "/gates/mutation"),
    ] {
        if let Some(src) = profile.get(key) {
            overlay(doc, pointer, src, class)?;
        }
    }
    if inherits_per_package(profile) {
        drop_unset_floors(doc, profile);
    }
    reject_tighter_agent(doc, class)
}

fn set_crate(
    doc: &mut Value,
    class: &str,
    crate_name: &str,
    profile: &Value,
) -> Result<(), ProfileError> {
    let Some(slot) = doc.pointer_mut("/crate").and_then(Value::as_object_mut) else {
        return Err(invalid(class, "template has no crate"));
    };
    slot.insert("name".to_string(), Value::String(crate_name.to_string()));
    slot.insert("class".to_string(), Value::String(class.to_string()));
    if let Some(policy) = profile.get("unsafe_policy").and_then(Value::as_str) {
        slot.insert(
            "unsafe_policy".to_string(),
            Value::String(policy.to_string()),
        );
    }
    Ok(())
}

fn overlay(doc: &mut Value, pointer: &str, src: &Value, class: &str) -> Result<(), ProfileError> {
    let Some(src_map) = src.as_object() else {
        return Err(invalid(
            class,
            format!("{pointer} profile value is not a map"),
        ));
    };
    let Some(slot) = doc.pointer_mut(pointer).and_then(Value::as_object_mut) else {
        return Err(invalid(class, format!("template has no {pointer}")));
    };
    for (key, value) in src_map {
        slot.insert(key.clone(), value.clone());
    }
    Ok(())
}

fn inherits_per_package(profile: &Value) -> bool {
    profile.get("inherits").and_then(Value::as_str) == Some("per-package")
}

/// A workspace root sets no coverage, CRAP, or mutation floors of its own.
/// Keys the profile did not supply are removed so the template's library-core
/// numbers do not remain.
fn drop_unset_floors(doc: &mut Value, profile: &Value) {
    if profile.get("coverage").is_none() {
        remove_keys(doc, "/gates/coverage", COVERAGE_FLOORS);
    }
    if profile.get("crap").is_none() {
        remove_keys(doc, "/gates/crap", CRAP_FLOORS);
    }
    if profile.get("mutation").is_none() {
        remove_keys(doc, "/gates/mutation", MUTATION_FLOORS);
    }
}

fn remove_keys(doc: &mut Value, pointer: &str, keys: &[&str]) {
    let Some(slot) = doc.pointer_mut(pointer).and_then(Value::as_object_mut) else {
        return;
    };
    for key in keys {
        slot.remove(*key);
    }
}

fn reject_tighter_agent(doc: &Value, class: &str) -> Result<(), ProfileError> {
    let Some(crap) = doc.pointer("/gates/crap") else {
        return Ok(());
    };
    let (Some(agent), Some(human)) = (
        crap.get("threshold_agent").and_then(Value::as_f64),
        crap.get("threshold_human").and_then(Value::as_f64),
    ) else {
        return Ok(());
    };
    if agent < human {
        return Err(invalid(
            class,
            format!("agent CRAP {agent} is tighter than human {human}"),
        ));
    }
    Ok(())
}

fn invalid(class: &str, detail: impl Into<String>) -> ProfileError {
    ProfileError::Invalid {
        class: class.to_string(),
        detail: detail.into(),
    }
}

fn parse_yaml(raw: &str) -> Result<Value, String> {
    serde_saphyr::from_str_with_options(
        raw,
        serde_saphyr::options! {
            strict_booleans: true,
        },
    )
    .map_err(|err| err.to_string())
}

fn emit_yaml(value: &Value) -> String {
    let mut out = String::new();
    match value {
        Value::Object(map) => write_map(&mut out, map, 0),
        other => {
            write_scalar(&mut out, other);
            out.push('\n');
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn write_map(out: &mut String, map: &Map<String, Value>, indent: usize) {
    for (key, value) in map {
        write_entry(out, key, value, indent);
    }
}

fn write_entry(out: &mut String, key: &str, value: &Value, indent: usize) {
    pad(out, indent);
    out.push_str(&plain_key(key));
    match value {
        Value::Object(map) if !map.is_empty() => {
            out.push_str(":\n");
            write_map(out, map, indent + 2);
        }
        Value::Array(items) if !items.is_empty() => {
            out.push_str(":\n");
            write_array(out, items, indent + 2);
        }
        Value::Object(_) => out.push_str(": {}\n"),
        Value::Array(_) => out.push_str(": []\n"),
        other => {
            out.push_str(": ");
            write_scalar(out, other);
            out.push('\n');
        }
    }
}

fn write_array(out: &mut String, items: &[Value], indent: usize) {
    for item in items {
        pad(out, indent);
        out.push('-');
        match item {
            Value::Object(map) if !map.is_empty() => {
                out.push('\n');
                write_map(out, map, indent + 2);
            }
            Value::Array(nested) if !nested.is_empty() => {
                out.push('\n');
                write_array(out, nested, indent + 2);
            }
            other => {
                out.push(' ');
                write_scalar(out, other);
                out.push('\n');
            }
        }
    }
}

fn write_scalar(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&emit_number(number)),
        Value::String(text) => write_string(out, text),
        Value::Array(_) | Value::Object(_) => out.push_str("null"),
    }
}

fn emit_number(number: &Number) -> String {
    if let Some(value) = number.as_i64() {
        return value.to_string();
    }
    if let Some(value) = number.as_u64() {
        return value.to_string();
    }
    if let Some(value) = number.as_f64() {
        if value.is_finite() && value.fract() == 0.0 && value.abs() < i64::MAX as f64 {
            return (value as i64).to_string();
        }
        return value.to_string();
    }
    number.to_string()
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' | '"' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn plain_key(key: &str) -> String {
    let plain = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if plain {
        key.to_string()
    } else {
        let mut quoted = String::new();
        write_string(&mut quoted, key);
        quoted
    }
}

fn pad(out: &mut String, indent: usize) {
    out.push_str(&" ".repeat(indent));
}
