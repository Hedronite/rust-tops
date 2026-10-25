pub const AGENTS_MD: &str = include_str!("../templates/AGENTS.md");
pub const CLIPPY_TOML: &str = include_str!("../templates/drop-in/clippy.toml");
pub const NEXTEST_TOML: &str = include_str!("../templates/drop-in/nextest.toml");
pub const MUTANTS_TOML: &str = include_str!("../templates/drop-in/mutants.toml");
pub const CARGO_CRAP_TOML: &str = include_str!("../templates/drop-in/cargo-crap.toml");
pub const DENY_TOML: &str = include_str!("../templates/drop-in/deny.toml");
pub const WORKFLOW_YML: &str = include_str!("../templates/drop-in/rust-tops.yml");
const JOB_FUZZ: &str = include_str!("../templates/drop-in/jobs/fuzz.yml");
const JOB_MIRI: &str = include_str!("../templates/drop-in/jobs/miri.yml");
const JOB_KANI: &str = include_str!("../templates/drop-in/jobs/kani.yml");
const JOB_CALLGRIND: &str = include_str!("../templates/drop-in/jobs/callgrind.yml");
pub const LAWS_BEND: &str = include_str!("../templates/LAWS.bend");
pub const BEND_LOCK: &str = include_str!("../templates/bend.lock");
pub const FETCH_BEND_SH: &str = include_str!("../templates/fetch-bend.sh");

const PROFILE_LIBRARY_CORE: &str = include_str!("../templates/rust-tops.example.yaml");

/// Class jobs `init` appends to the drop-in floor, keyed by the job keyword
/// `facts::class_keywords` asks the workflow to show.
const CLASS_JOBS: [(&str, &str); 4] = [
    ("fuzz", JOB_FUZZ),
    ("miri", JOB_MIRI),
    ("kani", JOB_KANI),
    ("callgrind", JOB_CALLGRIND),
];

/// The drop-in workflow plus one job per keyword this class requires (§4).
pub fn workflow_yml(class: &str) -> String {
    let keywords = crate::facts::class_keywords(class).unwrap_or(&[]);
    let mut yaml = WORKFLOW_YML.to_string();
    for (keyword, job) in CLASS_JOBS {
        if keywords.contains(&keyword) {
            yaml.push_str(job);
        }
    }
    yaml
}

/// `rust-toolchain.toml` pinned to the instance's `crate.msrv`.
pub fn rust_toolchain_toml(msrv: &str) -> String {
    format!(
        "# Written by `cargo tops init` from rust-tops.yaml crate.msrv. Local builds\n\
         # and every CI job that is not nightly-only use this channel\n\
         # (law clippy_toolchain_pinned).\n\
         [toolchain]\n\
         channel = \"{msrv}\"\n\
         components = [\"clippy\", \"rustfmt\"]\n"
    )
}

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

    const INIT_CLASSES: [&str; 5] = [
        "library-core",
        "parser-codec",
        "no-std-embedded",
        "unsafe-kernel",
        "binary-cli",
    ];

    #[test]
    fn every_init_class_workflow_has_a_job_per_required_keyword() {
        for class in INIT_CLASSES {
            let doc = crate::facts::parse_yaml(&super::workflow_yml(class))
                .unwrap_or_else(|err| panic!("{class} workflow: {err}"));
            let jobs = doc["jobs"].as_object().expect("jobs");
            let keywords = crate::facts::class_keywords(class).expect("init class has a §4 row");
            for keyword in keywords {
                assert!(
                    jobs.keys().any(|id| id.contains(keyword)),
                    "{class} workflow lacks a {keyword} job: {:?}",
                    jobs.keys().collect::<Vec<_>>()
                );
            }
            for (keyword, _) in super::CLASS_JOBS {
                let has_job = jobs.keys().any(|id| id.contains(keyword));
                assert_eq!(
                    has_job,
                    keywords.contains(&keyword),
                    "{class}: {keyword} job"
                );
            }
        }
    }

    #[test]
    fn no_workflow_job_installs_stable() {
        for class in INIT_CLASSES {
            let yaml = super::workflow_yml(class);
            assert!(!yaml.contains("rust-toolchain@stable"), "{class}");
            assert!(!yaml.contains("toolchain: stable"), "{class}");
            assert!(!yaml.contains("{MSRV}"), "{class}");
        }
    }

    /// Every job that installs the channel from `rust-toolchain.toml` reads
    /// it in the step just before, with this exact shell line.
    #[test]
    fn toolchain_steps_read_rust_toolchain_toml() {
        let read = r#"echo "channel=$(sed -n 's/^channel *= *"\([^"]*\)".*/\1/p' rust-toolchain.toml)" >> "$GITHUB_OUTPUT""#;
        let doc = crate::facts::parse_yaml(&super::workflow_yml("unsafe-kernel")).expect("yaml");
        let mut pinned_jobs = 0;
        for (id, job) in doc["jobs"].as_object().expect("jobs") {
            let steps = job["steps"].as_array().expect("steps");
            for (i, step) in steps.iter().enumerate() {
                if step["with"]["toolchain"] == "${{ steps.toolchain.outputs.channel }}" {
                    assert_eq!(steps[i - 1]["id"], "toolchain", "{id}");
                    assert_eq!(steps[i - 1]["run"], read, "{id}");
                    pinned_jobs += 1;
                }
            }
        }
        // fmt-clippy-deny, nextest, msrv, feature-matrix, coverage-crap, kani, callgrind-hot
        assert_eq!(pinned_jobs, 7);
    }

    #[test]
    fn rust_toolchain_toml_pins_the_msrv() {
        let text = super::rust_toolchain_toml("1.88");
        let table: toml::Table = text.parse().expect("toml");
        assert_eq!(table["toolchain"]["channel"].as_str(), Some("1.88"));
    }

    #[test]
    fn agents_md_matches_the_workspace_and_the_golden() {
        expect_vendored(super::AGENTS_MD, "AGENTS.md", "AGENTS.md");
        let golden =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/init/shared/AGENTS.md");
        let body = std::fs::read_to_string(&golden).expect("golden AGENTS.md");
        assert_eq!(super::AGENTS_MD, body);
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
