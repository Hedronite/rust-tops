use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct TopsConfig {
    pub protocol: Protocol,
    #[serde(rename = "crate")]
    pub crate_: Crate,
    #[cfg_attr(not(test), allow(dead_code))]
    pub goals: serde_json::Value,
    pub layers: Vec<Layer>,
    pub gates: Gates,
    #[cfg_attr(not(test), allow(dead_code))]
    pub density: serde_json::Value,
    #[cfg_attr(not(test), allow(dead_code))]
    pub performance: Option<serde_json::Value>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub agent: Option<serde_json::Value>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub exemptions: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct Protocol {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct Crate {
    pub name: String,
    pub class: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct Layer {
    pub id: String,
    pub enabled: bool,
    pub tier: Option<u8>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub runner: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct Gates {
    #[allow(dead_code)]
    pub coverage: CoverageGate,
    pub crap: CrapGate,
    pub mutation: MutationGate,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct CoverageGate {
    #[cfg_attr(not(test), allow(dead_code))]
    pub line_min_pct: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct CrapGate {
    /// Absent on a `workspace-root`, whose packages keep their own floors.
    pub threshold_agent: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct MutationGate {
    pub unexplained_survivors_max: i64,
}
