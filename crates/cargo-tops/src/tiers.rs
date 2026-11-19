//! Fixed layer tiers (§3) and class floors (§4).
//!
//! Every layer's tier comes from its id, through [`TIERS`]. An instance may
//! repeat that tier or leave it out; any other value is a problem. [`check`]
//! also compares an instance's floors with its class profile and reports a
//! required layer that is switched off. It returns problem lines, one per
//! fault, each naming the file, the key, the class value and the fix.
//!
//! `check` in `main.rs` calls [`check`] once the protocol 1.3.0 wiring lands.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
#![allow(dead_code, reason = "main.rs calls check once the 1.3.0 wiring lands")]

use serde_json::Value;

use crate::exemptions;
use crate::profile_data::class_profile;

/// Each layer id and its tier: the §3 table. `security-advisory` is tier 0
/// so a default `--max-tier 1` gate runs the advisory check (VD-P2).
pub(crate) const TIERS: [(&str, u8); 21] = [
    ("static-lint", 0),
    ("unit", 0),
    ("doc", 0),
    ("coverage", 0),
    ("crap", 0),
    ("laws", 0),
    ("security-advisory", 0),
    ("integration", 1),
    ("acceptance", 1),
    ("mutation", 1),
    ("property", 1),
    ("snapshot", 2),
    ("compile-fail", 2),
    ("fuzz", 3),
    ("concurrency", 3),
    ("miri", 3),
    ("sanitizer", 3),
    ("formal-kani", 4),
    ("bench-callgrind", 4),
    ("bench-criterion", 5),
    ("density-audit", 5),
];

/// The tier §3 fixes for layer `id`; `None` for an id the table lacks.
pub(crate) fn tier_of(id: &str) -> Option<u8> {
    TIERS
        .iter()
        .find(|(layer, _)| *layer == id)
        .map(|(_, tier)| *tier)
}

/// Which way a floor is tight: a minimum may not drop, a maximum may not rise.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bound {
    Min,
    Max,
}

/// One floor: the instance key, the key read when that one is absent, the
/// class profile pointer, and the layer whose exemption may loosen it.
struct Floor {
    key: &'static str,
    fallback: Option<&'static str>,
    section: &'static str,
    profile: &'static str,
    layer: &'static str,
    bound: Bound,
}

const FLOORS: [Floor; 6] = [
    Floor {
        key: "line_min_pct",
        fallback: None,
        section: "coverage",
        profile: "/coverage/line_min_pct",
        layer: "coverage",
        bound: Bound::Min,
    },
    Floor {
        key: "region_min_pct",
        fallback: None,
        section: "coverage",
        profile: "/coverage/region_min_pct",
        layer: "coverage",
        bound: Bound::Min,
    },
    // The gate reads `threshold_production` when there is no agent key.
    Floor {
        key: "threshold_agent",
        fallback: Some("threshold_production"),
        section: "crap",
        profile: "/crap/threshold_agent",
        layer: "crap",
        bound: Bound::Max,
    },
    Floor {
        key: "threshold_human",
        fallback: None,
        section: "crap",
        profile: "/crap/threshold_human",
        layer: "crap",
        bound: Bound::Max,
    },
    Floor {
        key: "kill_rate_min_pct",
        fallback: None,
        section: "mutation",
        profile: "/mutation/kill_rate_min_pct",
        layer: "mutation",
        bound: Bound::Min,
    },
    Floor {
        key: "unexplained_survivors_max",
        fallback: None,
        section: "mutation",
        profile: "/mutation/unexplained_survivors_max",
        layer: "mutation",
        bound: Bound::Max,
    },
];

/// A class profile key, and the layer a `required...` value switches on.
const REQUIRED_BY_PROFILE: [(&str, &str); 5] = [
    ("property", "property"),
    ("fuzz", "fuzz"),
    ("miri", "miri"),
    ("kani", "formal-kani"),
    ("callgrind", "bench-callgrind"),
];

/// Every problem in the instance `doc` (read from `file`): a tier that is not
/// the table's, a floor looser than its class profile with no live exemption
/// on `today` (`YYYY-MM-DD`), and a required layer that is off.
pub(crate) fn check(file: &str, doc: &Value, today: &str) -> Vec<String> {
    let mut problems = tier_problems(file, doc);
    let class = doc.pointer("/crate/class").and_then(Value::as_str);
    let profile = class.and_then(class_profile);
    if let (Some(class), Some(profile)) = (class, profile.as_ref()) {
        problems.extend(floor_problems(file, doc, class, profile, today));
    }
    problems.extend(required_problems(file, doc, class, profile.as_ref()));
    problems
}

fn layers(doc: &Value) -> &[Value] {
    doc.get("layers")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn tier_problems(file: &str, doc: &Value) -> Vec<String> {
    layers(doc)
        .iter()
        .filter_map(|layer| tier_problem(file, layer))
        .collect()
}

fn tier_problem(file: &str, layer: &Value) -> Option<String> {
    let id = layer.get("id").and_then(Value::as_str)?;
    let Some(fixed) = tier_of(id) else {
        return Some(format!(
            "{file}: layers[id={id}] is not a layer id in the §3 table; \
             use one of its layer ids or remove the entry"
        ));
    };
    let tier = layer.get("tier").filter(|t| !t.is_null())?;
    (tier.as_u64() != Some(u64::from(fixed))).then(|| {
        format!(
            "{file}: layers[id={id}].tier is {tier}, the §3 table fixes {id} at tier {fixed}; \
             set tier: {fixed} or delete the key"
        )
    })
}

fn floor_problems(
    file: &str,
    doc: &Value,
    class: &str,
    profile: &Value,
    today: &str,
) -> Vec<String> {
    FLOORS
        .iter()
        .filter_map(|floor| floor_problem(file, doc, class, profile, today, floor))
        .collect()
}

fn floor_problem(
    file: &str,
    doc: &Value,
    class: &str,
    profile: &Value,
    today: &str,
    floor: &Floor,
) -> Option<String> {
    let wanted = profile.pointer(floor.profile).and_then(Value::as_f64)?;
    let section = doc.pointer(&format!("/gates/{}", floor.section));
    let found = [Some(floor.key), floor.fallback]
        .into_iter()
        .flatten()
        .find_map(|key| Some((key, section?.get(key)?.as_f64()?)));
    let loose = found.is_none_or(|(_, have)| match floor.bound {
        Bound::Min => have < wanted,
        Bound::Max => have > wanted,
    });
    if !loose || live_exemption(doc, floor.layer, today) {
        return None;
    }
    let key = found.map_or(floor.key, |(key, _)| key);
    let have = found.map_or_else(|| "not set".to_string(), |(_, have)| have.to_string());
    let verb = if floor.bound == Bound::Min {
        "raise"
    } else {
        "lower"
    };
    Some(format!(
        "{file}: gates.{}.{key} is {have}, class {class} sets {wanted}; {verb} it to {wanted} \
         or add a dated exemption for layer {} that has not expired (§19)",
        floor.section, floor.layer
    ))
}

fn live_exemption(doc: &Value, layer: &str, today: &str) -> bool {
    exemptions::for_layer(doc.get("exemptions"), layer)
        .iter()
        .any(|record| record.live(today) && covers_crate(&record.path))
}

/// Whether an exemption path covers the whole crate, the only reach that may
/// lift a floor (VD-R7). A narrower glob exempts files, not the floor.
fn covers_crate(path: &str) -> bool {
    matches!(path, "**" | "./**" | "**/*" | "." | "./")
}

fn required_problems(
    file: &str,
    doc: &Value,
    class: Option<&str>,
    profile: Option<&Value>,
) -> Vec<String> {
    let mut wanted = vec![("security-advisory", "every class".to_string())];
    for (key, layer) in REQUIRED_BY_PROFILE {
        let value = profile.and_then(|p| p.get(key)).and_then(Value::as_str);
        if value.is_some_and(|v| v.starts_with("required")) {
            wanted.push((layer, format!("class {}", class.unwrap_or_default())));
        }
    }
    wanted
        .into_iter()
        .filter_map(|(layer, who)| required_problem(file, doc, layer, &who))
        .collect()
}

fn required_problem(file: &str, doc: &Value, layer: &str, who: &str) -> Option<String> {
    let entry = layers(doc)
        .iter()
        .find(|l| l.get("id").and_then(Value::as_str) == Some(layer));
    let on = entry
        .and_then(|l| l.get("enabled"))
        .and_then(Value::as_bool);
    (on != Some(true)).then(|| {
        let state = if entry.is_some() {
            "is false"
        } else {
            "is missing"
        };
        format!(
            "{file}: layers[id={layer}].enabled {state}, {who} requires the {layer} layer (§4); \
             set enabled: true"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{check, tier_of, TIERS};
    use crate::facts::parse_yaml;
    use serde_json::{json, Value};

    const TODAY: &str = "2026-11-19";
    const CLASSES: [&str; 8] = [
        "binary-cli",
        "ffi-bridge",
        "library-core",
        "library-util",
        "no-std-embedded",
        "parser-codec",
        "unsafe-kernel",
        "workspace-root",
    ];

    fn golden(class: &str) -> Value {
        let path = format!(
            "{}/tests/golden/init/{class}/rust-tops.yaml",
            env!("CARGO_MANIFEST_DIR")
        );
        let raw = std::fs::read_to_string(&path).expect("golden reads");
        parse_yaml(&raw).expect("golden parses")
    }

    fn set(doc: &mut Value, pointer: &str, value: Value) {
        *doc.pointer_mut(pointer).expect("pointer exists") = value;
    }

    fn remove(doc: &mut Value, pointer: &str, key: &str) {
        doc.pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .expect("map exists")
            .remove(key);
    }

    fn layer_index(doc: &Value, id: &str) -> usize {
        doc.get("layers")
            .and_then(Value::as_array)
            .expect("layers")
            .iter()
            .position(|l| l.get("id").and_then(Value::as_str) == Some(id))
            .expect("layer present")
    }

    fn one(doc: &Value) -> String {
        let problems = check("rust-tops.yaml", doc, TODAY);
        assert_eq!(problems.len(), 1, "{problems:?}");
        problems.into_iter().next().expect("one problem")
    }

    #[test]
    fn the_table_is_section_three_with_advisories_at_tier_zero() {
        assert_eq!(TIERS.len(), 21);
        assert_eq!(tier_of("security-advisory"), Some(0));
        assert_eq!(tier_of("mutation"), Some(1));
        assert_eq!(tier_of("snapshot"), Some(2));
        assert_eq!(tier_of("miri"), Some(3));
        assert_eq!(tier_of("bench-callgrind"), Some(4));
        assert_eq!(tier_of("density-audit"), Some(5));
        assert_eq!(tier_of("nonsense"), None);
    }

    #[test]
    fn all_eight_init_goldens_pass() {
        for class in CLASSES {
            assert_eq!(
                check("rust-tops.yaml", &golden(class), TODAY),
                Vec::<String>::new(),
                "{class}"
            );
        }
    }

    #[test]
    fn the_kit_instance_and_the_example_pass() {
        let root = format!("{}/../..", env!("CARGO_MANIFEST_DIR"));
        for file in ["rust-tops.yaml", "config/rust-tops.example.yaml"] {
            let raw = std::fs::read_to_string(format!("{root}/{file}")).expect("file reads");
            let doc = parse_yaml(&raw).expect("file parses");
            assert_eq!(check(file, &doc, TODAY), Vec::<String>::new(), "{file}");
        }
    }

    #[test]
    fn mutation_at_tier_five_fails_with_the_fix() {
        let mut doc = golden("library-core");
        let at = layer_index(&doc, "mutation");
        set(&mut doc, &format!("/layers/{at}/tier"), json!(5));
        assert_eq!(
            one(&doc),
            "rust-tops.yaml: layers[id=mutation].tier is 5, the §3 table fixes mutation at tier 1; \
             set tier: 1 or delete the key"
        );
    }

    #[test]
    fn a_missing_tier_passes_and_an_unknown_layer_fails() {
        let mut doc = golden("library-core");
        let at = layer_index(&doc, "unit");
        remove(&mut doc, &format!("/layers/{at}"), "tier");
        assert_eq!(check("f.yaml", &doc, TODAY), Vec::<String>::new());
        set(&mut doc, &format!("/layers/{at}/id"), json!("mystery"));
        assert!(one(&doc).contains("layers[id=mystery] is not a layer id"));
    }

    #[test]
    fn a_loose_line_floor_fails_for_the_class() {
        let mut doc = golden("library-core");
        set(&mut doc, "/gates/coverage/line_min_pct", json!(50));
        assert_eq!(
            one(&doc),
            "rust-tops.yaml: gates.coverage.line_min_pct is 50, class library-core sets 95; \
             raise it to 95 or add a dated exemption for layer coverage that has not expired (§19)"
        );
    }

    #[test]
    fn crap_and_mutation_floors_fail_the_other_way_too() {
        let cases = [
            ("/gates/crap/threshold_agent", json!(9), "lower it to 6"),
            ("/gates/crap/threshold_human", json!(5), "lower it to 4"),
            (
                "/gates/mutation/kill_rate_min_pct",
                json!(80),
                "raise it to 95",
            ),
            (
                "/gates/mutation/unexplained_survivors_max",
                json!(2),
                "lower it to 0",
            ),
        ];
        for (pointer, value, fix) in cases {
            let mut doc = golden("library-core");
            set(&mut doc, pointer, value);
            let problem = one(&doc);
            assert!(
                problem.contains(&pointer[1..].replace('/', ".")),
                "{problem}"
            );
            assert!(problem.contains(fix), "{problem}");
        }
    }

    #[test]
    fn a_production_only_crap_floor_is_judged_as_the_agent_floor() {
        let mut doc = golden("library-core");
        remove(&mut doc, "/gates/crap", "threshold_agent");
        set(&mut doc, "/gates/crap/threshold_production", json!(12));
        assert!(one(&doc).contains("gates.crap.threshold_production is 12"));
    }

    #[test]
    fn a_floor_that_is_gone_is_looser_than_the_class() {
        let mut doc = golden("parser-codec");
        remove(&mut doc, "/gates/coverage", "region_min_pct");
        assert!(one(&doc).contains("gates.coverage.region_min_pct is not set"));
    }

    #[test]
    fn a_tighter_floor_passes() {
        let mut doc = golden("binary-cli");
        set(&mut doc, "/gates/coverage/line_min_pct", json!(99));
        set(&mut doc, "/gates/crap/threshold_agent", json!(5));
        assert_eq!(check("f.yaml", &doc, TODAY), Vec::<String>::new());
    }

    fn exemption(layer: &str, expires: &str) -> Value {
        exemption_at("**", layer, expires)
    }

    fn exemption_at(path: &str, layer: &str, expires: &str) -> Value {
        json!([{"path": path, "layer": layer, "expires": expires,
                "reason": "legacy module under rewrite", "owner": "team"}])
    }

    #[test]
    fn a_live_exemption_passes_and_an_expired_one_fails() {
        let mut doc = golden("library-core");
        set(&mut doc, "/gates/coverage/line_min_pct", json!(50));
        set(&mut doc, "/exemptions", exemption("coverage", "2026-12-31"));
        assert_eq!(check("f.yaml", &doc, TODAY), Vec::<String>::new());
        set(&mut doc, "/exemptions", exemption("coverage", "2026-11-19"));
        assert!(one(&doc).contains("gates.coverage.line_min_pct is 50"));
    }

    #[test]
    fn an_exemption_for_part_of_the_crate_does_not_lift_a_floor() {
        let mut doc = golden("library-core");
        set(&mut doc, "/gates/coverage/line_min_pct", json!(50));
        for path in ["src/**", "src/gen/*.rs", "tests/**"] {
            set(
                &mut doc,
                "/exemptions",
                exemption_at(path, "coverage", "2030-01-01"),
            );
            assert!(one(&doc).contains("gates.coverage.line_min_pct"), "{path}");
        }
        set(
            &mut doc,
            "/exemptions",
            exemption_at("./**", "coverage", "2030-01-01"),
        );
        assert_eq!(check("f.yaml", &doc, TODAY), Vec::<String>::new());
    }

    #[test]
    fn an_exemption_for_another_layer_does_not_loosen_coverage() {
        let mut doc = golden("library-core");
        set(&mut doc, "/gates/coverage/line_min_pct", json!(50));
        set(&mut doc, "/exemptions", exemption("crap", "2030-01-01"));
        assert!(one(&doc).contains("gates.coverage.line_min_pct"));
    }

    #[test]
    fn a_disabled_security_advisory_fails_in_every_class() {
        for class in CLASSES {
            let mut doc = golden(class);
            let at = layer_index(&doc, "security-advisory");
            set(&mut doc, &format!("/layers/{at}/enabled"), json!(false));
            assert_eq!(
                one(&doc),
                "rust-tops.yaml: layers[id=security-advisory].enabled is false, every class requires \
                 the security-advisory layer (§4); set enabled: true",
                "{class}"
            );
        }
    }

    #[test]
    fn a_layer_the_class_requires_may_not_be_switched_off() {
        let mut doc = golden("parser-codec");
        let at = layer_index(&doc, "fuzz");
        set(&mut doc, &format!("/layers/{at}/enabled"), json!(false));
        assert!(one(&doc).contains("class parser-codec requires the fuzz layer"));
        let mut doc = golden("binary-cli");
        let at = layer_index(&doc, "miri");
        set(&mut doc, &format!("/layers/{at}/enabled"), json!(false));
        assert_eq!(check("f.yaml", &doc, TODAY), Vec::<String>::new());
    }

    #[test]
    fn a_missing_security_advisory_layer_is_named_missing() {
        let mut doc = golden("library-util");
        let at = layer_index(&doc, "security-advisory");
        doc.get_mut("layers")
            .and_then(Value::as_array_mut)
            .expect("layers")
            .remove(at);
        assert!(one(&doc).contains("layers[id=security-advisory].enabled is missing"));
    }

    #[test]
    fn a_class_with_no_profile_still_gets_the_tier_and_advisory_checks() {
        let mut doc = golden("library-core");
        set(&mut doc, "/crate/class", json!("binary-service"));
        set(&mut doc, "/gates/coverage/line_min_pct", json!(1));
        assert_eq!(check("f.yaml", &doc, TODAY), Vec::<String>::new());
        let at = layer_index(&doc, "mutation");
        set(&mut doc, &format!("/layers/{at}/tier"), json!(5));
        assert!(one(&doc).contains("layers[id=mutation].tier is 5"));
    }
}
