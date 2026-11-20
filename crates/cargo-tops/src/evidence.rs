//! Evidence that a credited layer's tests ran: the credited run's nextest
//! JUnit report.
//!
//! `property` and `acceptance` may take another layer's run (see
//! [`crate::credit`]). That run passes for them only if its JUnit report
//! holds a test whose name matches the layer's `test_filter` (see [`matches`])
//! and that test passed. This module reads that one file, from where nextest
//! writes it: the workspace root `cargo metadata` names, nextest's
//! `[store] dir` (default `target/nextest`), the profile, then the
//! `[profile.ci.junit]` path. A report older than the credited run is not
//! evidence. Nothing is built and no test runs, so reading it costs no
//! second test run.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The nextest profile whose report is read.
pub(crate) const PROFILE: &str = "ci";

/// Where nextest keeps its reports, under the workspace root, when
/// `.config/nextest.toml` has no `[store] dir`.
const DEFAULT_STORE: &str = "target/nextest";

/// How many matching tests passed and how many did not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    pub passed: usize,
    pub failed: usize,
}

/// Whether a nextest test name matches `filter` (HN-L2): one `::`-separated
/// segment of the name starts with it. `property_x` and `tests::property_x`
/// match `property`; `a_property_x` does not. A layer that runs itself
/// selects tests with the same rule.
#[must_use]
pub(crate) fn matches(name: &str, filter: &str) -> bool {
    name.split("::").any(|segment| segment.starts_with(filter))
}

/// Counts the tests in a JUnit report whose name matches `filter`.
///
/// A test with a `failure`, `error` or `skipped` child did not pass. An empty
/// `filter` matches every test.
#[must_use]
pub(crate) fn tally(xml: &str, filter: &str) -> Tally {
    let mut tally = Tally::default();
    for (name, passed) in testcases(xml) {
        if !matches(&name, filter) {
            continue;
        }
        if passed {
            tally.passed += 1;
        } else {
            tally.failed += 1;
        }
    }
    tally
}

/// Each `testcase` as its name and whether it passed.
fn testcases(xml: &str) -> Vec<(String, bool)> {
    let mut found = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<testcase ") {
        let from = rest.get(at..).unwrap_or_default();
        let Some((tag, after)) = from.split_once('>') else {
            break;
        };
        let (body, tail) = if tag.ends_with('/') {
            ("", after)
        } else {
            after.split_once("</testcase>").unwrap_or((after, ""))
        };
        if let Some(name) = attribute(tag, "name") {
            found.push((name, !has_verdict_child(body)));
        }
        rest = tail;
    }
    found
}

fn has_verdict_child(body: &str) -> bool {
    ["<failure", "<error", "<skipped"]
        .iter()
        .any(|child| body.contains(child))
}

/// The unescaped value of `key` in a start tag.
fn attribute(tag: &str, key: &str) -> Option<String> {
    let marker = format!(" {key}=\"");
    let value = tag.split_once(&marker)?.1;
    Some(unescape(value.split_once('"')?.0))
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The `path` of `[profile.ci.junit]` in the text of `.config/nextest.toml`.
#[must_use]
pub(crate) fn junit_name(nextest_toml: &str) -> Option<String> {
    let doc: toml::Table = toml::from_str(nextest_toml).ok()?;
    doc.get("profile")?
        .get(PROFILE)?
        .get("junit")?
        .get("path")?
        .as_str()
        .map(str::to_string)
}

/// The `dir` of `[store]` in the text of `.config/nextest.toml`, relative to
/// the workspace root. nextest keeps its reports there.
#[must_use]
pub(crate) fn store_dir(nextest_toml: &str) -> String {
    toml::from_str::<toml::Table>(nextest_toml)
        .ok()
        .and_then(|doc| doc.get("store")?.get("dir")?.as_str().map(str::to_string))
        .unwrap_or_else(|| DEFAULT_STORE.to_string())
}

/// The report the credited run wrote: `<root>/<store dir>/ci/<path>`, with
/// the store dir and the file name from `.config/nextest.toml` under `root`.
///
/// # Errors
/// The setting is missing, so the run writes no report to read.
pub(crate) fn junit_path(root: &Path) -> Result<PathBuf, String> {
    let text = fs::read_to_string(root.join(".config").join("nextest.toml")).unwrap_or_default();
    junit_name(&text)
        .map(|name| root.join(store_dir(&text)).join(PROFILE).join(name))
        .ok_or_else(|| {
            "no JUnit report is configured: set `path` under `[profile.ci.junit]` in \
             .config/nextest.toml so the credited run writes one"
                .to_string()
        })
}

/// Whether the credited run, which began at `since`, held a passed test
/// matching `filter` in a report it wrote.
///
/// Returns how many matching tests passed. The file is only read, never
/// removed.
///
/// # Errors
/// The detail for a `fail` (the judge adds the layer): it names the filter
/// and the report path read (relative to `root`), or the
/// `[profile.ci.junit]` setting.
pub(crate) fn credit(
    layer: &str,
    root: &Path,
    filter: &str,
    since: SystemTime,
) -> Result<usize, String> {
    let path = junit_path(root)?;
    let shown = path
        .strip_prefix(root)
        .unwrap_or(&path)
        .display()
        .to_string();
    let xml = fs::read_to_string(&path).map_err(|_| {
        format!(
            "no JUnit report at {shown}; the credited run must write one \
             (`[profile.ci.junit]` and `[store] dir` in .config/nextest.toml)"
        )
    })?;
    let fresh = fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .is_ok_and(|written| written >= since);
    if !fresh {
        return Err(format!(
            "the JUnit report {shown} is older than the credited run, so it is not evidence \
             of this run; make that run write it (`[profile.ci.junit]` and `[store] dir` in \
             .config/nextest.toml)"
        ));
    }
    let found = tally(&xml, filter);
    if found.passed > 0 {
        return Ok(found.passed);
    }
    let why = if found.failed > 0 {
        "every matching test failed or was skipped"
    } else {
        "no test matches"
    };
    Err(format!(
        "the credited run's report {shown} holds no passed test whose name has a `::` segment \
         starting with `{filter}` ({why}); add one, or set gates.{layer}.test_filter"
    ))
}

#[cfg(test)]
mod tests {
    use super::{credit, junit_name, junit_path, matches, store_dir, tally, Tally};
    use std::fs;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="4">
    <testsuite name="demo::p" tests="3">
        <testcase name="plain" classname="demo::p" time="0.017"/>
        <testcase name="property_bad" classname="demo::p" time="0.017">
            <failure message="boom &lt;x&gt;" type="test failure">boom</failure>
            <system-out>a &lt;testcase name="property_fake"/&gt;</system-out>
        </testcase>
        <testcase name="property_ok" classname="demo::p" time="0.017"/>
        <testcase name="tests::property_nested" classname="demo" time="0.1">
            <system-out>fine</system-out>
        </testcase>
        <testcase name="property_skip" classname="demo" time="0.0"><skipped/></testcase>
        <testcase name="property_err" classname="demo" time="0.0"><error message="m"/></testcase>
    </testsuite>
</testsuites>"#;

    #[test]
    fn only_a_passed_test_whose_name_matches_the_filter_counts() {
        assert_eq!(
            tally(REPORT, "property"),
            Tally {
                passed: 2,
                failed: 3
            }
        );
        assert_eq!(
            tally(REPORT, "plain"),
            Tally {
                passed: 1,
                failed: 0
            }
        );
        assert_eq!(tally(REPORT, "acceptance"), Tally::default());
        assert_eq!(tally(REPORT, "Property"), Tally::default());
        assert_eq!(tally("", "property"), Tally::default());
    }

    #[test]
    fn a_segment_must_start_with_the_filter() {
        assert!(matches("property_x", "property"));
        assert!(matches("tests::property_x", "property"));
        assert!(matches("a::property::b", "property"));
        assert!(!matches("a_property_x", "property"));
        assert!(!matches("tests::a_property", "property"));
        assert!(!matches("Property_x", "property"));
        assert!(matches("anything", ""));
    }

    #[test]
    fn a_meta_test_that_only_mentions_the_filter_is_not_evidence() {
        let xml = r#"<testcase name="a_property_filter_that_matches_nothing_fails"/>"#;
        assert_eq!(tally(xml, "property"), Tally::default());
    }

    #[test]
    fn escaped_text_inside_a_test_is_not_a_test() {
        assert_eq!(tally(REPORT, "property_fake"), Tally::default());
    }

    #[test]
    fn an_unclosed_tag_ends_the_scan_without_a_count() {
        assert_eq!(
            tally("<testcase name=\"property_x\"", "property"),
            Tally::default()
        );
        let open = r#"<testcase name="property_x"><failure/>"#;
        assert_eq!(tally(open, "property").failed, 1);
    }

    #[test]
    fn a_name_is_unescaped() {
        let xml = r#"<testcase name="property_a&amp;b&quot;c&apos;d&lt;e&gt;" classname="x"/>"#;
        assert_eq!(tally(xml, "property_a&b\"c'd<e>").passed, 1);
    }

    #[test]
    fn the_report_name_is_the_junit_path_of_the_ci_profile() {
        let toml = "[profile.ci]\nretries = 0\n[profile.ci.junit]\npath = \"j.xml\"\n";
        assert_eq!(junit_name(toml).as_deref(), Some("j.xml"));
        assert_eq!(junit_name("[profile.ci]\nretries = 0\n"), None);
        assert_eq!(
            junit_name("[profile.default.junit]\npath = \"j.xml\"\n"),
            None
        );
        assert_eq!(junit_name("not toml ["), None);
        assert_eq!(junit_name("[profile.ci.junit]\npath = 3\n"), None);
    }

    fn fixture(config: Option<&str>, report: Option<&str>) -> tempdir::Dir {
        let dir = tempdir::Dir::new();
        if let Some(config) = config {
            fs::create_dir_all(dir.path().join(".config")).unwrap();
            fs::write(dir.path().join(".config/nextest.toml"), config).unwrap();
        }
        if let Some(report) = report {
            let at = dir.path().join("target/nextest/ci");
            fs::create_dir_all(&at).unwrap();
            fs::write(at.join("junit.xml"), report).unwrap();
        }
        dir
    }

    const CONFIG: &str = "[profile.ci.junit]\npath = \"junit.xml\"\n";

    fn run(dir: &tempdir::Dir, filter: &str) -> Result<usize, String> {
        credit("property", dir.path(), filter, UNIX_EPOCH)
    }

    #[test]
    fn a_passed_matching_test_is_credit() {
        let dir = fixture(Some(CONFIG), Some(REPORT));
        assert_eq!(run(&dir, "property"), Ok(2));
        assert_eq!(run(&dir, "plain"), Ok(1));
    }

    #[test]
    fn no_match_names_the_filter_and_the_path_read() {
        let dir = fixture(Some(CONFIG), Some(REPORT));
        let why = run(&dir, "acceptance").unwrap_err();
        assert!(why.contains("`acceptance`"), "{why}");
        assert!(why.contains("target/nextest/ci/junit.xml"), "{why}");
        assert!(why.contains("no test matches"), "{why}");
        assert!(why.contains("gates.property.test_filter"), "{why}");
        assert!(!why.contains(dir.path().to_str().unwrap_or("?")), "{why}");
    }

    #[test]
    fn a_failed_match_is_not_evidence() {
        let xml = r#"<testcase name="property_x"><failure/></testcase>"#;
        let dir = fixture(Some(CONFIG), Some(xml));
        let why = run(&dir, "property").unwrap_err();
        assert!(why.contains("failed or was skipped"), "{why}");
    }

    #[test]
    fn a_missing_report_names_the_junit_setting() {
        let dir = fixture(Some(CONFIG), None);
        let why = run(&dir, "property").unwrap_err();
        assert!(
            why.contains("no JUnit report at target/nextest/ci/junit.xml"),
            "{why}"
        );
        assert!(why.contains("[profile.ci.junit]"), "{why}");
    }

    #[test]
    fn no_junit_setting_names_the_junit_setting() {
        let dir = fixture(None, Some(REPORT));
        let why = run(&dir, "property").unwrap_err();
        assert!(why.starts_with("no JUnit report is configured"), "{why}");
        assert!(why.contains("[profile.ci.junit]"), "{why}");
        let bare = fixture(Some("[profile.ci]\nretries = 0\n"), Some(REPORT));
        assert!(run(&bare, "property").is_err());
        assert!(junit_path(bare.path()).is_err());
    }

    #[test]
    fn the_file_name_comes_from_the_config() {
        let dir = fixture(Some("[profile.ci.junit]\npath = \"out/j.xml\"\n"), None);
        let path = junit_path(dir.path()).unwrap();
        assert_eq!(path, dir.path().join("target/nextest/ci/out/j.xml"));
    }

    #[test]
    fn a_moved_store_dir_moves_where_the_report_is_read() {
        let config = "[store]\ndir = \"target/nt\"\n[profile.ci.junit]\npath = \"junit.xml\"\n";
        assert_eq!(store_dir(config), "target/nt");
        assert_eq!(store_dir(CONFIG), "target/nextest");
        assert_eq!(store_dir("not toml ["), "target/nextest");
        assert_eq!(store_dir("[store]\ndir = 3\n"), "target/nextest");
        let dir = fixture(Some(config), None);
        assert_eq!(
            junit_path(dir.path()).unwrap(),
            dir.path().join("target/nt/ci/junit.xml")
        );
        // The old location holds a matching report; the moved store holds none.
        let old = fixture(Some(config), Some(REPORT));
        let why = run(&old, "property").unwrap_err();
        assert!(
            why.contains("no JUnit report at target/nt/ci/junit.xml"),
            "{why}"
        );
    }

    #[test]
    fn a_report_older_than_the_run_is_not_evidence_and_is_kept() {
        let dir = fixture(Some(CONFIG), Some(REPORT));
        let later = SystemTime::now() + Duration::from_secs(3600);
        let why = credit("property", dir.path(), "property", later).unwrap_err();
        assert!(why.contains("target/nextest/ci/junit.xml"), "{why}");
        assert!(why.contains("older than the credited run"), "{why}");
        assert!(dir.path().join("target/nextest/ci/junit.xml").is_file());
        assert_eq!(
            credit("property", dir.path(), "property", UNIX_EPOCH),
            Ok(2)
        );
    }

    /// A scratch directory removed on drop. It removes only what it made.
    mod tempdir {
        use std::path::{Path, PathBuf};

        pub(super) struct Dir(PathBuf);

        impl Dir {
            pub(super) fn new() -> Self {
                use std::sync::atomic::{AtomicUsize, Ordering};
                static N: AtomicUsize = AtomicUsize::new(0);
                let path = std::env::temp_dir().join(format!(
                    "rt-evidence-{}-{}",
                    std::process::id(),
                    N.fetch_add(1, Ordering::Relaxed)
                ));
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }

            pub(super) fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).ok();
            }
        }
    }
}
