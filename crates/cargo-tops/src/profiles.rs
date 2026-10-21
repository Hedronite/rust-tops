pub const AGENTS_MD: &str = include_str!("../templates/AGENTS.md");
pub const CLIPPY_TOML: &str = include_str!("../templates/drop-in/clippy.toml");
pub const NEXTEST_TOML: &str = include_str!("../templates/drop-in/nextest.toml");
pub const MUTANTS_TOML: &str = include_str!("../templates/drop-in/mutants.toml");
pub const CARGO_CRAP_TOML: &str = include_str!("../templates/drop-in/cargo-crap.toml");
pub const DENY_TOML: &str = include_str!("../templates/drop-in/deny.toml");
pub const WORKFLOW_YML: &str = include_str!("../templates/drop-in/rust-tops.yml");
pub const LAWS_BEND: &str = include_str!("../templates/LAWS.bend");
pub const BEND_LOCK: &str = include_str!("../templates/bend.lock");
pub const FETCH_BEND_SH: &str = include_str!("../templates/fetch-bend.sh");

const PROFILE_LIBRARY_CORE: &str = include_str!("../templates/rust-tops.example.yaml");

pub fn profile_yaml(class: &str, crate_name: &str) -> String {
    let mut yaml = PROFILE_LIBRARY_CORE.to_string();
    yaml = yaml.replace("example-codec", crate_name);
    yaml = yaml.replace("class: parser-codec", &format!("class: {class}"));
    match class {
        "library-core" => {}
        "no-std-embedded" => {
            yaml = yaml.replace("line_min_pct: 95", "line_min_pct: 90");
            yaml = yaml.replace("region_min_pct: 90", "region_min_pct: 85");
            yaml = yaml.replace(
                "unsafe_policy: allow-documented",
                "unsafe_policy: required-kernel",
            );
        }
        "unsafe-kernel" => {
            yaml = yaml.replace(
                "unsafe_policy: allow-documented",
                "unsafe_policy: required-kernel",
            );
        }
        "binary-cli" => {
            yaml = yaml.replace("line_min_pct: 95", "line_min_pct: 80");
            yaml = yaml.replace("region_min_pct: 90", "region_min_pct: 75");
            yaml = yaml.replace("kill_rate_min_pct: 95", "kill_rate_min_pct: 85");
        }
        "library-util" => {
            yaml = yaml.replace("line_min_pct: 95", "line_min_pct: 90");
            yaml = yaml.replace("region_min_pct: 90", "region_min_pct: 85");
        }
        _ => {}
    }
    yaml
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{BEND_LOCK, FETCH_BEND_SH, LAWS_BEND};

    /// The template must be a byte copy of the workspace file it vendors.
    fn expect_vendored(template: &str, workspace_rel: &str, template_name: &str) {
        // `cargo package` does not ship the workspace tree. Skip there; the
        // workspace checkout still compares the template to its source.
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(workspace_rel);
        if !source.is_file() {
            eprintln!(
                "template drift check: skipped, {workspace_rel} is outside the packaged crate ({})",
                source.display()
            );
            return;
        }
        let body = std::fs::read_to_string(&source)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", source.display()));
        assert_eq!(
            template, body,
            "bend-gate: template drift ({workspace_rel} != crates/cargo-tops/templates/{template_name})"
        );
    }

    #[test]
    fn laws_bend_matches_pack() {
        expect_vendored(LAWS_BEND, "laws/LAWS.bend", "LAWS.bend");
    }

    #[test]
    fn bend_lock_matches_pack() {
        expect_vendored(BEND_LOCK, "laws/bend.lock", "bend.lock");
    }

    #[test]
    fn fetch_bend_matches_script() {
        expect_vendored(FETCH_BEND_SH, "scripts/fetch-bend.sh", "fetch-bend.sh");
    }
}
