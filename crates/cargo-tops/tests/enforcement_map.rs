//! `schema/enforcement.json` classes every leaf key of the instance schema as
//! `judged` (a source file reads it and a different value can change what
//! `check` or `gate` decides) or `declarative` (carried, never read). These
//! tests fail when a leaf has no class, an entry names no leaf, or a judged
//! entry's reader file does not hold the key's name (PROTOCOL.md §23).
use std::fs;
use std::path::Path;

use serde_json::{json, Value};

const SCHEMA: &str = include_str!("../../../schema/rust-tops.schema.json");
const MAP: &str = include_str!("../../../schema/enforcement.json");

/// The leaf keys of a schema as JSON pointers, `*` for each item of an array.
/// An object with `properties` is a branch; so is an array whose items have.
fn leaves(node: &Value, path: &str, out: &mut Vec<String>) {
    let branch = node
        .get("properties")
        .or_else(|| node.get("items").and_then(|items| items.get("properties")));
    let step = if node.get("properties").is_some() {
        ""
    } else {
        "/*"
    };
    match branch.and_then(Value::as_object) {
        Some(children) => {
            for (name, child) in children {
                leaves(child, &format!("{path}{step}/{name}"), out);
            }
        }
        None => out.push(path.to_string()),
    }
}

fn schema_leaves(schema: &Value) -> Vec<String> {
    let mut out = Vec::new();
    leaves(schema, "", &mut out);
    out
}

/// Every way `map` disagrees with `schema`. `read` gives a reader file's
/// text, or `None` when it cannot be read.
fn problems(schema: &Value, map: &Value, read: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let entries = map.get("keys").and_then(Value::as_object);
    let Some(entries) = entries else {
        return vec!["the map has no `keys` object".to_string()];
    };
    let leaves = schema_leaves(schema);
    let mut found: Vec<String> = leaves
        .iter()
        .filter(|leaf| !entries.contains_key(leaf.as_str()))
        .map(|leaf| format!("{leaf} is a schema key with no class in the map"))
        .collect();
    for (key, entry) in entries {
        if !leaves.contains(key) {
            found.push(format!("{key} is in the map but names no schema key"));
        }
        found.extend(entry_problems(key, entry, &read));
    }
    found
}

/// What is wrong with one map entry, on its own.
fn entry_problems(key: &str, entry: &Value, read: &impl Fn(&str) -> Option<String>) -> Vec<String> {
    let class = entry.get("class").and_then(Value::as_str);
    let reader = entry.get("reader").and_then(Value::as_str);
    match (class, reader) {
        (Some("declarative"), None) => Vec::new(),
        (Some("declarative"), Some(_)) => {
            vec![format!("{key} is declarative and must name no reader")]
        }
        (Some("judged"), Some(file)) => judged_problems(key, entry, file, read),
        (Some("judged"), None) => vec![format!("{key} is judged and names no reader")],
        _ => vec![format!("{key} has no class of `judged` or `declarative`")],
    }
}

fn judged_problems(
    key: &str,
    entry: &Value,
    file: &str,
    read: &impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut found = Vec::new();
    let via = entry.get("via").and_then(Value::as_str);
    if !matches!(via, Some("check" | "gate")) {
        found.push(format!("{key} needs `via` of `check` or `gate`"));
    }
    let name = key.rsplit('/').next().unwrap_or(key);
    match read(file) {
        None => found.push(format!("{key}: the reader {file} cannot be read")),
        Some(text) if !text.contains(name) => {
            found.push(format!("{key}: the reader {file} never names `{name}`"));
        }
        Some(_) => {}
    }
    found
}

fn repo_file(path: &str) -> Option<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    fs::read_to_string(root.join(path)).ok()
}

fn schema() -> Value {
    serde_json::from_str(SCHEMA).expect("the schema is JSON")
}

fn map() -> Value {
    serde_json::from_str(MAP).expect("the map is JSON")
}

#[test]
fn the_shipped_map_matches_the_shipped_schema() {
    let found = problems(&schema(), &map(), repo_file);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn the_map_covers_every_leaf_the_schema_has() {
    // 79 leaves at 73f86c6, plus `base_ref` and the two `test_filter` keys.
    assert_eq!(schema_leaves(&schema()).len(), 82);
}

#[test]
fn kill_rate_min_pct_is_declarative() {
    let map = map();
    let entry = map
        .pointer("/keys/~1gates~1mutation~1kill_rate_min_pct")
        .expect("the key is classed");
    assert_eq!(entry.get("class"), Some(&json!("declarative")));
}

fn tiny_schema() -> Value {
    json!({"properties": {
        "a": {"type": "string"},
        "b": {"properties": {"c": {"type": "integer"}}},
        "d": {"type": "array", "items": {"properties": {"e": {"type": "string"}}}},
        "f": {"type": "array", "items": {"type": "string"}}
    }})
}

fn tiny_map() -> Value {
    json!({"keys": {
        "/a": {"class": "judged", "reader": "r.rs", "via": "gate"},
        "/b/c": {"class": "declarative"},
        "/d/*/e": {"class": "declarative"},
        "/f": {"class": "declarative"}
    }})
}

fn entries(map: &mut Value) -> &mut serde_json::Map<String, Value> {
    map.get_mut("keys")
        .and_then(Value::as_object_mut)
        .expect("the map has keys")
}

fn put(map: &mut Value, key: &str, entry: Value) {
    entries(map).insert(key.to_string(), entry);
}

fn reads(file: &str) -> Option<String> {
    (file == "r.rs").then(|| "fn a() {}".to_string())
}

#[test]
fn a_consistent_map_has_no_problem() {
    assert_eq!(
        problems(&tiny_schema(), &tiny_map(), reads),
        Vec::<String>::new()
    );
    let mut found = Vec::new();
    leaves(&tiny_schema(), "", &mut found);
    assert_eq!(found, ["/a", "/b/c", "/d/*/e", "/f"]);
}

#[test]
fn an_unclassified_key_fails() {
    let mut map = tiny_map();
    entries(&mut map).remove("/b/c");
    let found = problems(&tiny_schema(), &map, reads);
    assert_eq!(found, ["/b/c is a schema key with no class in the map"]);
}

#[test]
fn a_stale_entry_fails() {
    let mut map = tiny_map();
    put(&mut map, "/gone", json!({"class": "declarative"}));
    let found = problems(&tiny_schema(), &map, reads);
    assert_eq!(found, ["/gone is in the map but names no schema key"]);
}

#[test]
fn a_reader_that_lacks_the_key_fails() {
    let found = problems(&tiny_schema(), &tiny_map(), |_| {
        Some("fn other() {}".into())
    });
    assert_eq!(found, ["/a: the reader r.rs never names `a`"]);
    let unreadable = problems(&tiny_schema(), &tiny_map(), |_| None);
    assert_eq!(unreadable, ["/a: the reader r.rs cannot be read"]);
}

#[test]
fn a_malformed_entry_fails() {
    let mut map = tiny_map();
    put(
        &mut map,
        "/b/c",
        json!({"class": "declarative", "reader": "r.rs"}),
    );
    put(&mut map, "/f", json!({"class": "judged"}));
    put(&mut map, "/a", json!({"class": "judged", "reader": "r.rs"}));
    put(&mut map, "/d/*/e", json!({"class": "maybe"}));
    let found = problems(&tiny_schema(), &map, reads);
    assert_eq!(
        found,
        [
            "/a needs `via` of `check` or `gate`",
            "/b/c is declarative and must name no reader",
            "/d/*/e has no class of `judged` or `declarative`",
            "/f is judged and names no reader",
        ]
    );
    let none = problems(&tiny_schema(), &json!({}), reads);
    assert_eq!(none, ["the map has no `keys` object"]);
}
