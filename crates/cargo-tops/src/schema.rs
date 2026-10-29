//! `cargo tops check` against `schema/rust-tops.schema.json`.
//!
//! The validator knows the JSON Schema keywords that schema uses and no
//! others. A keyword it does not know is a test failure
//! (`the_schema_uses_only_keywords_the_validator_knows`), never a silent pass.
//! `format` is an annotation, as JSON Schema 2020-12 treats it by default.

use serde_json::Value;

/// Keywords [`validate`] enforces.
#[cfg(test)]
const CHECKED: &[&str] = &[
    "type",
    "const",
    "enum",
    "required",
    "properties",
    "additionalProperties",
    "items",
    "minItems",
    "minimum",
    "maximum",
    "minLength",
    "pattern",
];

/// Keywords that carry no assertion.
#[cfg(test)]
const ANNOTATIONS: &[&str] = &["$schema", "$id", "title", "description", "format"];

/// Every violation of `schema` in `value`, each prefixed with its JSON pointer.
pub fn validate(schema: &Value, value: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    check(schema, value, "", &mut errors);
    errors
}

fn check(schema: &Value, value: &Value, at: &str, errors: &mut Vec<String>) {
    let Some(rules) = schema.as_object() else {
        return;
    };
    let mut fail =
        |msg: String| errors.push(format!("{}: {msg}", if at.is_empty() { "/" } else { at }));
    if let Some(types) = rules.get("type") {
        let names: Vec<&str> = match types {
            Value::Array(list) => list.iter().filter_map(Value::as_str).collect(),
            other => other.as_str().into_iter().collect(),
        };
        if !names.iter().any(|name| has_type(value, name)) {
            fail(format!(
                "{} is not of type {}",
                show(value),
                names.join(" or ")
            ));
            return;
        }
    }
    if let Some(want) = rules.get("const") {
        if !same(want, value) {
            fail(format!("{} is not {}", show(value), show(want)));
        }
    }
    if let Some(Value::Array(options)) = rules.get("enum") {
        if !options.iter().any(|option| same(option, value)) {
            let list: Vec<String> = options.iter().map(show).collect();
            fail(format!("{} is not one of {}", show(value), list.join(", ")));
        }
    }
    if let Some(number) = value.as_f64() {
        if let Some(min) = rules.get("minimum").and_then(Value::as_f64) {
            if number < min {
                fail(format!("{number} is below the minimum {min}"));
            }
        }
        if let Some(max) = rules.get("maximum").and_then(Value::as_f64) {
            if number > max {
                fail(format!("{number} is above the maximum {max}"));
            }
        }
    }
    if let Some(text) = value.as_str() {
        if let Some(min) = rules.get("minLength").and_then(Value::as_u64) {
            if (text.chars().count() as u64) < min {
                fail(format!("{} is shorter than {min} characters", show(value)));
            }
        }
        if let Some(pattern) = rules.get("pattern").and_then(Value::as_str) {
            match regex::Regex::new(pattern) {
                Ok(re) if re.is_match(text) => {}
                Ok(_) => fail(format!("{} does not match {pattern}", show(value))),
                Err(err) => fail(format!("schema pattern {pattern} is invalid: {err}")),
            }
        }
    }
    if let Some(items) = value.as_array() {
        if let Some(min) = rules.get("minItems").and_then(Value::as_u64) {
            if (items.len() as u64) < min {
                fail(format!("has {} item(s), fewer than {min}", items.len()));
            }
        }
        if let Some(item_schema) = rules.get("items") {
            for (index, item) in items.iter().enumerate() {
                check(item_schema, item, &format!("{at}/{index}"), errors);
            }
        }
    }
    if let Some(map) = value.as_object() {
        object(rules, map, at, errors);
    }
}

fn object(
    rules: &serde_json::Map<String, Value>,
    map: &serde_json::Map<String, Value>,
    at: &str,
    errors: &mut Vec<String>,
) {
    let here = if at.is_empty() { "/" } else { at };
    if let Some(Value::Array(required)) = rules.get("required") {
        for key in required.iter().filter_map(Value::as_str) {
            if !map.contains_key(key) {
                errors.push(format!("{here}: missing required key {key}"));
            }
        }
    }
    let properties = rules.get("properties").and_then(Value::as_object);
    for (key, child) in map {
        let path = format!("{at}/{key}");
        match properties.and_then(|props| props.get(key)) {
            Some(child_schema) => check(child_schema, child, &path, errors),
            None => match rules.get("additionalProperties") {
                Some(Value::Bool(false)) => {
                    errors.push(format!("{here}: key {key} is not allowed"));
                }
                Some(extra @ Value::Object(_)) => check(extra, child, &path, errors),
                _ => {}
            },
        }
    }
}

fn has_type(value: &Value, name: &str) -> bool {
    match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        "integer" => value.as_f64().is_some_and(|n| n.fract() == 0.0),
        _ => false,
    }
}

/// JSON Schema equality: `30` and `30.0` are the same number.
fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

fn show(value: &Value) -> String {
    let text = value.to_string();
    if text.len() > 60 {
        format!("{}…", &text[..text.floor_char_boundary(57)])
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::{validate, ANNOTATIONS, CHECKED};
    use serde_json::{json, Value};

    fn schema() -> Value {
        serde_json::from_str(crate::profiles::SCHEMA_JSON).expect("schema is JSON")
    }

    fn keywords(schema: &Value, out: &mut Vec<String>) {
        let Some(rules) = schema.as_object() else {
            return;
        };
        for (key, child) in rules {
            out.push(key.clone());
            match key.as_str() {
                "properties" => {
                    for sub in child.as_object().into_iter().flat_map(|m| m.values()) {
                        keywords(sub, out);
                    }
                }
                "items" | "additionalProperties" => keywords(child, out),
                _ => {}
            }
        }
    }

    #[test]
    fn the_schema_uses_only_keywords_the_validator_knows() {
        let mut seen = Vec::new();
        keywords(&schema(), &mut seen);
        let unknown: Vec<&String> = seen
            .iter()
            .filter(|k| !CHECKED.contains(&k.as_str()) && !ANNOTATIONS.contains(&k.as_str()))
            .collect();
        assert!(unknown.is_empty(), "unchecked schema keywords: {unknown:?}");
    }

    #[test]
    fn every_keyword_rejects_what_it_should() {
        let cases = [
            (json!({"type": "integer"}), json!(1.5)),
            (json!({"type": ["number", "null"]}), json!("x")),
            (json!({"const": 30}), json!(31)),
            (json!({"enum": ["a", "b"]}), json!("c")),
            (json!({"required": ["k"]}), json!({})),
            (json!({"additionalProperties": false}), json!({"k": 1})),
            (
                json!({"properties": {"k": {"type": "string"}}}),
                json!({"k": 1}),
            ),
            (json!({"items": {"type": "string"}}), json!([1])),
            (json!({"minItems": 1}), json!([])),
            (json!({"minimum": 1}), json!(0)),
            (json!({"maximum": 100}), json!(101)),
            (json!({"minLength": 3}), json!("ab")),
            (json!({"pattern": "^[0-9]+$"}), json!("1a")),
        ];
        for (rule, bad) in cases {
            assert_eq!(validate(&rule, &bad).len(), 1, "{rule} accepted {bad}");
        }
        assert!(validate(&json!({"const": 30}), &json!(30.0)).is_empty());
        assert!(validate(&json!({"type": "integer"}), &json!(4.0)).is_empty());
        assert!(validate(&json!({"format": "date"}), &json!("not a date")).is_empty());
    }

    #[test]
    fn a_violation_names_its_pointer() {
        let rule = json!({"properties": {"gates": {"properties": {"crap": {"properties": {
            "threshold_agent": {"type": "number", "minimum": 1}
        }}}}}});
        let doc = json!({"gates": {"crap": {"threshold_agent": 0}}});
        assert_eq!(
            validate(&rule, &doc),
            ["/gates/crap/threshold_agent: 0 is below the minimum 1"]
        );
    }
}
