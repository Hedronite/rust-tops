#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
pub(crate) const AGENTS_MD: &str = include_str!("../templates/AGENTS.md");
pub(crate) const CLIPPY_TOML: &str = include_str!("../templates/drop-in/clippy.toml");

/// `clippy.toml` for `class`. Binaries set `avoid-breaking-exported-api` to
/// false because that is the file clippy reads. Libraries keep clippy's default.
pub(crate) fn clippy_toml(class: &str) -> Result<String, crate::profile_data::ProfileError> {
    let mut text = CLIPPY_TOML.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    if !crate::profile_data::avoid_breaking_exported_api(class)? {
        text.push_str("avoid-breaking-exported-api = false\n");
    }
    Ok(text)
}
pub(crate) const NEXTEST_TOML: &str = include_str!("../templates/drop-in/nextest.toml");
pub(crate) const MUTANTS_TOML: &str = include_str!("../templates/drop-in/mutants.toml");
pub(crate) const CARGO_CRAP_TOML: &str = include_str!("../templates/drop-in/cargo-crap.toml");
pub(crate) const DENY_TOML: &str = include_str!("../templates/drop-in/deny.toml");
pub(crate) const WORKFLOW_YML: &str = include_str!("../templates/drop-in/rust-tops.yml");
const JOB_FUZZ: &str = include_str!("../templates/drop-in/jobs/fuzz.yml");
const JOB_MIRI: &str = include_str!("../templates/drop-in/jobs/miri.yml");
const JOB_KANI: &str = include_str!("../templates/drop-in/jobs/kani.yml");
const JOB_CALLGRIND: &str = include_str!("../templates/drop-in/jobs/callgrind.yml");
pub(crate) const LAWS_BEND: &str = include_str!("../templates/LAWS.bend");
pub(crate) const BEND_LOCK: &str = include_str!("../templates/bend.lock");
pub(crate) const FETCH_BEND_SH: &str = include_str!("../templates/fetch-bend.sh");

/// Byte copy of `schema/rust-tops.schema.json`; `cargo tops check` validates
/// against it (`cargo package` ships `templates/`, not the workspace `schema/`).
pub(crate) const SCHEMA_JSON: &str = include_str!("../templates/rust-tops.schema.json");

/// Class jobs `init` appends to the drop-in floor, keyed by the job keyword
/// `facts::class_keywords` asks the workflow to show.
const CLASS_JOBS: [(&str, &str); 4] = [
    ("fuzz", JOB_FUZZ),
    ("miri", JOB_MIRI),
    ("kani", JOB_KANI),
    ("callgrind", JOB_CALLGRIND),
];

/// The drop-in workflow plus one job per keyword this class requires (§4).
pub(crate) fn workflow_yml(class: &str) -> String {
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
pub(crate) fn rust_toolchain_toml(msrv: &str) -> String {
    format!(
        "# Written by `cargo tops init` from rust-tops.yaml crate.msrv. Local builds\n\
         # and every CI job that is not nightly-only use this channel\n\
         # (law clippy_toolchain_pinned).\n\
         [toolchain]\n\
         channel = \"{msrv}\"\n\
         components = [\"clippy\", \"rustfmt\"]\n"
    )
}

/// `rust-tops.yaml` for `class`: the class profile merged onto the template
/// as data. A class with no profile is an error, never library-core.
pub(crate) fn profile_yaml(
    class: &str,
    crate_name: &str,
) -> Result<String, crate::profile_data::ProfileError> {
    crate::profile_data::merge_profile(class, crate_name)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{BEND_LOCK, FETCH_BEND_SH, LAWS_BEND, SCHEMA_JSON};

    #[test]
    fn binary_clippy_toml_sets_avoid_breaking_exported_api_false() {
        let bin = super::clippy_toml("binary-cli").unwrap();
        assert!(
            bin.lines()
                .any(|line| line == "avoid-breaking-exported-api = false"),
            "{bin}"
        );
        let lib = super::clippy_toml("library-core").unwrap();
        assert!(
            lib.lines()
                .all(|line| !line.starts_with("avoid-breaking-exported-api")),
            "{lib}"
        );
        assert_eq!(lib, super::CLIPPY_TOML);
    }

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

    const INIT_CLASSES: [&str; 8] = [
        "library-core",
        "parser-codec",
        "no-std-embedded",
        "unsafe-kernel",
        "binary-cli",
        "library-util",
        "ffi-bridge",
        "workspace-root",
    ];

    #[test]
    fn every_init_class_workflow_has_a_job_per_required_keyword() {
        for class in INIT_CLASSES {
            let doc = crate::facts::parse_yaml(&super::workflow_yml(class))
                .unwrap_or_else(|err| panic!("{class} workflow: {err}"));
            #[expect(
                clippy::indexing_slicing,
                reason = "already in this crate when the strict table was turned on"
            )]
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
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let jobs = doc["jobs"].as_object().expect("jobs").clone();
        for (id, job) in jobs {
            #[expect(
                clippy::indexing_slicing,
                reason = "already in this crate when the strict table was turned on"
            )]
            let steps_val = job["steps"].clone();
            let steps = steps_val.as_array().expect("steps");
            for (i, step) in steps.iter().enumerate() {
                #[expect(
                    clippy::indexing_slicing,
                    reason = "already in this crate when the strict table was turned on"
                )]
                let toolchain = step["with"]["toolchain"].clone();
                if toolchain == "${{ steps.toolchain.outputs.channel }}" {
                    #[expect(
                        clippy::indexing_slicing,
                        reason = "already in this crate when the strict table was turned on"
                    )]
                    let prev_id = steps[i - 1]["id"].clone();
                    assert_eq!(prev_id, "toolchain", "{id}");
                    #[expect(
                        clippy::indexing_slicing,
                        reason = "already in this crate when the strict table was turned on"
                    )]
                    let prev_run = steps[i - 1]["run"].clone();
                    assert_eq!(prev_run, read, "{id}");
                    pinned_jobs += 1;
                }
            }
        }
        // fmt-clippy-deny, nextest, msrv, feature-matrix, coverage-crap,
        // tops-gate, kani, callgrind-hot
        assert_eq!(pinned_jobs, 8);
    }

    #[test]
    fn rust_toolchain_toml_pins_the_msrv() {
        let text = super::rust_toolchain_toml("1.88");
        let table: toml::Table = text.parse().expect("toml");
        #[expect(
            clippy::indexing_slicing,
            reason = "already in this crate when the strict table was turned on"
        )]
        let tops_idx_217_20 = table["toolchain"]["channel"].clone();
        assert_eq!(tops_idx_217_20.as_str(), Some("1.88"));
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
    fn schema_matches_the_workspace_schema() {
        expect_vendored(
            SCHEMA_JSON,
            "schema/rust-tops.schema.json",
            "rust-tops.schema.json",
        );
    }

    #[test]
    fn fetch_bend_matches_script() {
        expect_vendored(FETCH_BEND_SH, "scripts/fetch-bend.sh", "fetch-bend.sh");
    }
}
