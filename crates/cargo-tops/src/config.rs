use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TopsConfig {
    pub protocol: Protocol,
    #[serde(rename = "crate")]
    pub crate_: Crate,
    #[allow(dead_code)]
    pub goals: serde_yaml::Value,
    pub layers: Vec<Layer>,
    pub gates: Gates,
    #[allow(dead_code)]
    pub density: serde_yaml::Value,
    #[allow(dead_code)]
    pub performance: Option<serde_yaml::Value>,
    #[allow(dead_code)]
    pub agent: Option<serde_yaml::Value>,
    #[allow(dead_code)]
    pub exemptions: Option<serde_yaml::Value>,
}

#[derive(Debug, Deserialize)]
pub struct Protocol {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct Crate {
    pub name: String,
    pub class: String,
}

#[derive(Debug, Deserialize)]
pub struct Layer {
    pub id: String,
    pub enabled: bool,
    pub tier: Option<u8>,
    #[allow(dead_code)]
    pub runner: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Gates {
    #[allow(dead_code)]
    pub coverage: CoverageGate,
    pub crap: CrapGate,
    pub mutation: MutationGate,
}

#[derive(Debug, Deserialize)]
pub struct CoverageGate {
    #[allow(dead_code)]
    pub line_min_pct: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct CrapGate {
    pub threshold_agent: f64,
}

#[derive(Debug, Deserialize)]
pub struct MutationGate {
    pub unexplained_survivors_max: i64,
}
