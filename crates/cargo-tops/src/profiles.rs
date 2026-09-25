pub const AGENTS_MD: &str = include_str!("../templates/AGENTS.md");
pub const CLIPPY_TOML: &str = include_str!("../templates/drop-in/clippy.toml");
pub const NEXTEST_TOML: &str = include_str!("../templates/drop-in/nextest.toml");
pub const MUTANTS_TOML: &str = include_str!("../templates/drop-in/mutants.toml");
pub const CARGO_CRAP_TOML: &str = include_str!("../templates/drop-in/cargo-crap.toml");
pub const DENY_TOML: &str = include_str!("../templates/drop-in/deny.toml");
pub const WORKFLOW_YML: &str = include_str!("../templates/drop-in/rust-tops.yml");

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
