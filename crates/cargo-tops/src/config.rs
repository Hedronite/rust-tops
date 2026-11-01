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
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct CoverageGate {
    pub line_min_pct: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct CrapGate {
    /// Absent on a `workspace-root`, whose packages keep their own floors.
    pub threshold_agent: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct MutationGate {
    pub unexplained_survivors_max: i64,
}
