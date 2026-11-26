#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
use serde::{Deserialize, Serialize};

use crate::steps;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Enforce {
    Hard,
    Ratchet,
    Advisory,
}

impl Enforce {
    /// Canonical YAML/JSON name of this mode.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Hard => "hard",
            Self::Ratchet => "ratchet",
            Self::Advisory => "advisory",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct Advisory {
    pub expires: String,
    pub reason: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct TopsConfig {
    pub protocol: Protocol,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enforce: Option<Enforce>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advisory: Option<Advisory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<String>,
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

impl TopsConfig {
    /// Validates `advisory` when `enforce: advisory` is active (§2.3).
    pub(crate) fn check_advisory(&self) -> Result<(), String> {
        if self.enforce != Some(Enforce::Advisory) {
            return Ok(());
        }
        let Some(adv) = &self.advisory else {
            return Err("enforce: advisory requires an advisory block".to_string());
        };
        if !steps::valid_expiry(&adv.expires) {
            return Err(format!(
                "advisory.expires {:?} is not a real YYYY-MM-DD calendar day",
                adv.expires
            ));
        }
        if adv.reason.split_whitespace().count() < 3 {
            return Err("advisory.reason must have at least three words".to_string());
        }
        Ok(())
    }

    /// Warning / failure line when `enforce: advisory` has reached `expires` UTC.
    pub(crate) fn expired_advisory(&self, today: &str) -> Option<String> {
        if self.enforce != Some(Enforce::Advisory) {
            return None;
        }
        let adv = self.advisory.as_ref()?;
        (adv.expires.as_str() <= today).then(|| {
            format!(
                "advisory: the advisory expired on {} (reason: {}); \
                 renew it or move to enforce: ratchet (PROTOCOL.md §2.3)",
                adv.expires, adv.reason
            )
        })
    }

    /// Whether a live (valid and unexpired) `enforce: advisory` waives exit codes.
    pub(crate) fn live_advisory(&self, today: &str) -> bool {
        self.enforce == Some(Enforce::Advisory)
            && self.advisory.as_ref().is_some_and(|adv| {
                steps::valid_expiry(&adv.expires) && adv.expires.as_str() > today
            })
    }

    /// Warnings emitted by `check` for non-fatal config anomalies (expired
    /// advisory, unused `advisory`, or ignored `gates.crap.baseline`).
    pub(crate) fn check_warnings(&self, today: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(problem) = self.expired_advisory(today) {
            out.push(problem);
        }
        if self.enforce != Some(Enforce::Advisory) && self.advisory.is_some() {
            out.push("advisory is ignored when enforce is not advisory (PROTOCOL.md §2.3)".into());
        }
        if self.enforce.is_some() && self.gates.crap.baseline.is_some() {
            out.push(
                "gates.crap.baseline is ignored when enforce is set; use top-level baseline (PROTOCOL.md §2.1, §2.3)".into(),
            );
        }
        out
    }
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

impl CrapGate {
    /// Active CRAP score cap (`threshold_agent` takes precedence over `threshold_production`).
    pub(crate) fn cap(&self) -> Option<f64> {
        self.threshold_agent.or(self.threshold_production)
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct MutationGate {
    pub unexplained_survivors_max: i64,
}

#[cfg(test)]
mod tests {
    use super::{Advisory, Enforce, Gates, TopsConfig};
    use serde_json::json;

    fn gates(extra: &serde_json::Value) -> Gates {
        let mut doc =
            json!({"coverage": {}, "crap": {}, "mutation": {"unexplained_survivors_max": 0}});
        if let (Some(base), Some(more)) = (doc.as_object_mut(), extra.as_object()) {
            base.extend(more.clone());
        }
        serde_json::from_value(doc).expect("gates")
    }

    fn base_cfg() -> TopsConfig {
        TopsConfig {
            protocol: super::Protocol {
                id: "rust-tops".into(),
                version: "1.5.0".into(),
            },
            enforce: None,
            advisory: None,
            baseline: None,
            crate_: super::Crate {
                name: "demo".into(),
                class: "library-core".into(),
            },
            goals: json!({}),
            layers: Vec::new(),
            gates: gates(&json!({})),
            density: json!({}),
            performance: None,
            agent: None,
            exemptions: None,
        }
    }

    #[test]
    fn a_test_filter_defaults_to_the_layer_id() {
        let none = gates(&json!({}));
        assert_eq!(none.test_filter("property"), "property");
        assert_eq!(none.test_filter("acceptance"), "acceptance");
        assert_eq!(none.test_filter("unit"), "unit");
        let set = gates(
            &json!({"property": {"test_filter": "prop_"}, "acceptance": {"test_filter": "acc_"}}),
        );
        assert_eq!(set.test_filter("property"), "prop_");
        assert_eq!(set.test_filter("acceptance"), "acc_");
        let empty = gates(&json!({"property": {"test_filter": ""}}));
        assert_eq!(empty.test_filter("property"), "property");
        let absent = gates(&json!({"property": {"framework": "proptest"}}));
        assert_eq!(absent.test_filter("property"), "property");
    }

    #[test]
    fn advisory_validation_checks_presence_calendar_date_and_word_count() {
        let mut cfg = base_cfg();
        assert!(cfg.check_advisory().is_ok());

        cfg.enforce = Some(Enforce::Advisory);
        assert!(cfg
            .check_advisory()
            .is_err_and(|e| e.contains("requires an advisory block")));

        cfg.advisory = Some(Advisory {
            expires: "2026-02-30".into(),
            reason: "adopting on legacy crate".into(),
        });
        assert!(cfg
            .check_advisory()
            .is_err_and(|e| e.contains("calendar day")));

        cfg.advisory = Some(Advisory {
            expires: "2027-01-01".into(),
            reason: "too short".into(),
        });
        assert!(cfg
            .check_advisory()
            .is_err_and(|e| e.contains("three words")));

        cfg.advisory = Some(Advisory {
            expires: "2027-01-01".into(),
            reason: "adopting on legacy".into(),
        });
        assert!(cfg.check_advisory().is_ok());
        assert!(cfg.live_advisory("2026-12-31"));
        assert!(!cfg.live_advisory("2027-01-01"));
        assert!(cfg.expired_advisory("2026-12-31").is_none());
        assert!(cfg.expired_advisory("2027-01-01").is_some());
    }

    #[test]
    fn enforce_as_str_and_check_warnings_report_ignored_fields() {
        assert_eq!(Enforce::Hard.as_str(), "hard");
        assert_eq!(Enforce::Ratchet.as_str(), "ratchet");
        assert_eq!(Enforce::Advisory.as_str(), "advisory");

        let mut cfg = base_cfg();
        cfg.enforce = Some(Enforce::Ratchet);
        cfg.advisory = Some(Advisory {
            expires: "2026-01-01".into(),
            reason: "legacy onboarding window".into(),
        });
        cfg.gates.crap.baseline = Some("crap-baseline.json".into());
        let warnings = cfg.check_warnings("2026-11-25");
        assert_eq!(warnings.len(), 2, "{warnings:?}");

        let mut adv_cfg = base_cfg();
        adv_cfg.enforce = Some(Enforce::Advisory);
        adv_cfg.advisory = Some(Advisory {
            expires: "2099-01-01".into(),
            reason: "legacy onboarding window".into(),
        });
        assert!(adv_cfg.check_warnings("2026-11-25").is_empty());

        let mut legacy_cfg = base_cfg();
        legacy_cfg.gates.crap.baseline = Some("crap-baseline.json".into());
        assert!(legacy_cfg.check_warnings("2026-11-25").is_empty());
    }
}
