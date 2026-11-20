#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct TopsConfig {
    pub protocol: Protocol,
    #[serde(rename = "crate")]
    pub crate_: Crate,
    pub goals: serde_json::Value,
    pub layers: Vec<Layer>,
    pub gates: Gates,
    pub density: serde_json::Value,
    pub performance: Option<serde_json::Value>,
    pub agent: Option<serde_json::Value>,
    pub exemptions: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct Protocol {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct Crate {
    pub name: String,
    pub class: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct Layer {
    pub id: String,
    pub enabled: bool,
    pub tier: Option<u8>,
    pub runner: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct Gates {
    pub coverage: CoverageGate,
    pub crap: CrapGate,
    pub mutation: MutationGate,
    /// The ref the mutation diff starts from, when `--base` is not given.
    /// Absent means the remote's default branch (`origin/HEAD`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    /// The `property` gate. Only `test_filter` is read here; the rest of the
    /// object stays as written.
    #[serde(default, skip_serializing)]
    pub property: Option<serde_json::Value>,
    /// The `acceptance` gate, as [`Gates::property`].
    #[serde(default, skip_serializing)]
    pub acceptance: Option<serde_json::Value>,
}

impl Gates {
    /// The `test_filter` of `layer` (`property` or `acceptance`): the name
    /// prefix a test must start a `::` segment with to count (PROTOCOL.md
    /// §6, §9). It defaults to the layer's id.
    pub(crate) fn test_filter(&self, layer: &str) -> String {
        let gate = match layer {
            "property" => &self.property,
            "acceptance" => &self.acceptance,
            _ => &None,
        };
        gate.as_ref()
            .and_then(|gate| gate.get("test_filter"))
            .and_then(serde_json::Value::as_str)
            .filter(|filter| !filter.is_empty())
            .unwrap_or(layer)
            .to_string()
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct CoverageGate {
    pub line_min_pct: Option<f64>,
    pub region_min_pct: Option<f64>,
    pub per_file_line_min_pct: Option<f64>,
    pub exclude_globs: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct CrapGate {
    /// Absent on a `workspace-root`, whose packages keep their own floors.
    pub threshold_agent: Option<f64>,
    pub threshold_production: Option<f64>,
    pub fail_on: Option<String>,
    /// The CRAP baseline, relative to the instance directory. Absent means
    /// `crap-baseline.json` in the Cargo target directory (§16).
    pub baseline: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct MutationGate {
    pub unexplained_survivors_max: i64,
}

#[cfg(test)]
mod tests {
    use super::Gates;
    use serde_json::json;

    fn gates(extra: &serde_json::Value) -> Gates {
        let mut doc =
            json!({"coverage": {}, "crap": {}, "mutation": {"unexplained_survivors_max": 0}});
        if let (Some(base), Some(more)) = (doc.as_object_mut(), extra.as_object()) {
            base.extend(more.clone());
        }
        serde_json::from_value(doc).expect("gates")
    }

    #[test]
    fn a_test_filter_defaults_to_the_layer_id() {
        let none = gates(&json!({}));
        assert_eq!(none.test_filter("property"), "property");
        assert_eq!(none.test_filter("acceptance"), "acceptance");
        let other = gates(&json!({"property": {"framework": "proptest"}}));
        assert_eq!(other.test_filter("property"), "property");
    }

    #[test]
    fn each_layer_reads_its_own_test_filter() {
        let set = gates(&json!({
            "property": {"test_filter": "prop_"},
            "acceptance": {"test_filter": "journey_"},
        }));
        assert_eq!(set.test_filter("property"), "prop_");
        assert_eq!(set.test_filter("acceptance"), "journey_");
        assert_eq!(set.test_filter("unit"), "unit");
    }

    #[test]
    fn an_empty_test_filter_is_the_default() {
        let empty = gates(&json!({"property": {"test_filter": ""}}));
        assert_eq!(empty.test_filter("property"), "property");
    }
}
