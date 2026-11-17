//! What `init` reads from the crate it runs in.
//!
//! `cargo metadata --no-deps --manifest-path <dir>/Cargo.toml` gives the name,
//! edition and `rust-version`. The manifest's lint table gives the unsafe
//! policy. The directories from the crate up to the workspace root say whether
//! a toolchain pin already exists. `init` never walks up to a parent package:
//! a directory with no `Cargo.toml` is a bare directory and takes defaults,
//! each one named in a stdout note.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A toolchain pin cargo and rustup both read.
const TOOLCHAIN_FILES: [&str; 2] = ["rust-toolchain.toml", "rust-toolchain"];
/// The editions `rust-tops.schema.json` accepts.
const EDITIONS: [&str; 2] = ["2021", "2024"];

/// Values `init` writes into `rust-tops.yaml` besides the class profile's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Settings {
    /// `crate.edition`; `None` keeps the kit default.
    pub(crate) edition: Option<String>,
    /// `crate.msrv`; `None` keeps the template value.
    pub(crate) msrv: Option<String>,
    /// The crate's lint table sets `unsafe_code = "forbid"`.
    pub(crate) unsafe_forbid: bool,
    /// `--laws`: the laws layer is written `enabled: true`.
    pub(crate) laws: bool,
}

/// Everything `init` decided from the target directory.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) name: String,
    pub(crate) settings: Settings,
    /// Whether `rust-toolchain.toml` is written (no pin in the crate directory
    /// or above it up to the workspace root).
    pub(crate) write_toolchain: bool,
    /// One stdout line per defaulted field, in the order they were decided.
    pub(crate) notes: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct CrateFacts {
    bare: bool,
    name: Option<String>,
    edition: Option<String>,
    msrv: Option<String>,
    unsafe_forbid: bool,
    workspace_root: Option<PathBuf>,
}

/// What `init` was asked for: the target, the flags, and the fallbacks.
pub(crate) struct Request<'a> {
    pub(crate) dir: &'a Path,
    /// `--name`.
    pub(crate) name_flag: Option<&'a str>,
    /// The directory basename, for a crate with no package name.
    pub(crate) dir_name: &'a str,
    /// The `rustc` pin that stands in for an absent `rust-version`.
    pub(crate) lock_rustc: &'a str,
    /// `--laws`.
    pub(crate) laws: bool,
}

/// What `init` writes for the crate in `req.dir`.
pub(crate) fn plan(req: &Request<'_>) -> Result<Plan> {
    let facts = read(req.dir)?;
    let mut notes = Vec::new();
    let name = match (req.name_flag, facts.name) {
        (Some(flag), _) => flag.to_string(),
        (None, Some(found)) => found,
        (None, None) => {
            let why = if facts.bare {
                "there is no Cargo.toml here"
            } else {
                "Cargo.toml has no package"
            };
            notes.push(format!(
                "note: crate.name {} is the directory name; {why}",
                req.dir_name
            ));
            req.dir_name.to_string()
        }
    };
    let edition = checked_edition(facts.edition, &mut notes)?;
    let msrv = facts.msrv.unwrap_or_else(|| {
        notes.push(format!(
            "note: crate.msrv {} is the rustc in rust-tops.lock; no rust-version found",
            req.lock_rustc
        ));
        req.lock_rustc.to_string()
    });
    Ok(Plan {
        name,
        settings: Settings {
            edition,
            msrv: Some(msrv),
            unsafe_forbid: facts.unsafe_forbid,
            laws: req.laws,
        },
        write_toolchain: !has_toolchain_pin(req.dir, facts.workspace_root.as_deref()),
        notes,
    })
}

/// The edition the schema accepts, or a note that the kit default stands.
fn checked_edition(edition: Option<String>, notes: &mut Vec<String>) -> Result<Option<String>> {
    match edition {
        Some(found) if !EDITIONS.contains(&found.as_str()) => {
            bail!("edition {found} is not one rust-tops supports; use 2021 or 2024")
        }
        Some(found) => Ok(Some(found)),
        None => {
            notes.push(
                "note: crate.edition 2024 is the kit default; no package edition found".into(),
            );
            Ok(None)
        }
    }
}

/// The note `init` prints when `unsafe_policy` did not come from a
/// `forbid` lint. `policy` is the value written.
pub(crate) fn unsafe_note(settings: &Settings, policy: &str) -> Option<String> {
    (!settings.unsafe_forbid).then(|| {
        format!(
            "note: unsafe_policy {policy} is the class default; the crate's lint table does not set unsafe_code = \"forbid\""
        )
    })
}

/// The `[tools] rustc` of a `rust-tops.lock` text.
pub(crate) fn lock_rustc(raw: &str) -> Option<String> {
    let table: toml::Table = raw.parse().ok()?;
    table
        .get("tools")?
        .get("rustc")?
        .as_str()
        .map(str::to_string)
}

fn read(dir: &Path) -> Result<CrateFacts> {
    let manifest = dir.join("Cargo.toml");
    if !manifest.is_file() {
        return Ok(CrateFacts {
            bare: true,
            ..CrateFacts::default()
        });
    }
    let manifest = manifest
        .canonicalize()
        .with_context(|| format!("resolve {}", manifest.display()))?;
    let json = metadata(&manifest)?;
    let mut facts = from_metadata(&json, &manifest);
    facts.unsafe_forbid = forbids_unsafe(&manifest, facts.workspace_root.as_deref());
    Ok(facts)
}

fn metadata(manifest: &Path) -> Result<Value> {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .context("spawn cargo metadata")?;
    if !output.status.success() {
        bail!(
            "cargo metadata failed for {}: {}",
            manifest.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout).map_err(|err| anyhow!("cargo metadata output: {err}"))
}

fn text(package: &Value, key: &str) -> Option<String> {
    package.get(key)?.as_str().map(str::to_string)
}

/// The package whose manifest is `manifest`; for a virtual workspace root,
/// the highest edition and `rust-version` among its members.
fn from_metadata(json: &Value, manifest: &Path) -> CrateFacts {
    let packages = json
        .get("packages")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let own = packages
        .iter()
        .find(|package| package.get("manifest_path").and_then(Value::as_str) == manifest.to_str());
    let workspace_root = json
        .get("workspace_root")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let (name, edition, msrv) = match own {
        Some(package) => (
            text(package, "name"),
            text(package, "edition"),
            text(package, "rust_version"),
        ),
        None => (
            None,
            packages.iter().filter_map(|p| text(p, "edition")).max(),
            packages
                .iter()
                .filter_map(|p| text(p, "rust_version"))
                .max_by_key(|version| version_key(version)),
        ),
    };
    CrateFacts {
        bare: false,
        name,
        edition,
        msrv,
        unsafe_forbid: false,
        workspace_root,
    }
}

fn version_key(version: &str) -> Vec<u64> {
    version
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn read_toml(path: &Path) -> Option<toml::Table> {
    std::fs::read_to_string(path).ok()?.parse().ok()
}

/// Whether the lint table of the manifest (or, with `workspace = true`, of
/// the workspace root) sets `unsafe_code` to `forbid`.
fn forbids_unsafe(manifest: &Path, root: Option<&Path>) -> bool {
    let Some(own) = read_toml(manifest) else {
        return false;
    };
    let root_table = root.and_then(|dir| read_toml(&dir.join("Cargo.toml")));
    unsafe_level(&own, root_table.as_ref()).as_deref() == Some("forbid")
}

fn unsafe_level(own: &toml::Table, root: Option<&toml::Table>) -> Option<String> {
    let lints = own
        .get("lints")
        .or_else(|| own.get("workspace")?.get("lints"))?;
    let lints = if lints.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
        root?.get("workspace")?.get("lints")?
    } else {
        lints
    };
    let lint = lints.get("rust")?.get("unsafe_code")?;
    let level = lint.get("level").unwrap_or(lint);
    level.as_str().map(str::to_string)
}

/// Whether `dir`, or a directory above it up to the workspace root, holds a
/// toolchain pin. Without a workspace root above `dir`, only `dir` is read.
/// A dangling link counts: it is something at that name.
fn has_toolchain_pin(dir: &Path, root: Option<&Path>) -> bool {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let stop = root.filter(|root| dir.starts_with(root));
    for ancestor in dir.ancestors() {
        let pinned = TOOLCHAIN_FILES
            .iter()
            .any(|file| std::fs::symlink_metadata(ancestor.join(file)).is_ok());
        if pinned {
            return true;
        }
        if stop.is_none_or(|stop| stop == ancestor) {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn plan_for(dir: &Path, name_flag: Option<&str>, dir_name: &str) -> Result<Plan> {
        plan(&Request {
            dir,
            name_flag,
            dir_name,
            lock_rustc: "1.99.1",
            laws: false,
        })
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tops-meta-{tag}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("mkdir");
        dir.canonicalize().expect("canonical")
    }

    fn table(raw: &str) -> toml::Table {
        raw.parse().expect("toml")
    }

    #[test]
    fn a_package_gives_its_own_name_edition_and_rust_version() {
        let manifest = Path::new("/w/a/Cargo.toml");
        let json = json!({"workspace_root": "/w", "packages": [
            {"name": "b", "edition": "2024", "rust_version": "1.99", "manifest_path": "/w/b/Cargo.toml"},
            {"name": "a", "edition": "2021", "rust_version": "1.95", "manifest_path": "/w/a/Cargo.toml"},
        ]});
        let facts = from_metadata(&json, manifest);
        assert_eq!(facts.name.as_deref(), Some("a"));
        assert_eq!(facts.edition.as_deref(), Some("2021"));
        assert_eq!(facts.msrv.as_deref(), Some("1.95"));
        assert_eq!(facts.workspace_root, Some(PathBuf::from("/w")));
    }

    #[test]
    fn a_virtual_root_takes_the_highest_member_values() {
        let json = json!({"workspace_root": "/w", "packages": [
            {"name": "b", "edition": "2024", "rust_version": "1.10", "manifest_path": "/w/b/Cargo.toml"},
            {"name": "a", "edition": "2021", "rust_version": "1.9", "manifest_path": "/w/a/Cargo.toml"},
            {"name": "c", "edition": "2021", "rust_version": null, "manifest_path": "/w/c/Cargo.toml"},
        ]});
        let facts = from_metadata(&json, Path::new("/w/Cargo.toml"));
        assert_eq!(facts.name, None);
        assert_eq!(facts.edition.as_deref(), Some("2024"));
        assert_eq!(facts.msrv.as_deref(), Some("1.10"), "1.10 is above 1.9");
    }

    #[test]
    fn metadata_with_no_packages_gives_nothing() {
        let facts = from_metadata(&json!({}), Path::new("/w/Cargo.toml"));
        assert_eq!(facts, CrateFacts::default());
    }

    #[test]
    fn the_lint_table_decides_forbid() {
        let own = |level: &str| {
            table(&format!(
                "[package]\nname = \"x\"\n[lints.rust]\nunsafe_code = {level}\n"
            ))
        };
        for (level, want) in [
            ("\"forbid\"", Some("forbid")),
            ("{ level = \"forbid\", priority = 1 }", Some("forbid")),
            ("\"deny\"", Some("deny")),
        ] {
            assert_eq!(unsafe_level(&own(level), None).as_deref(), want, "{level}");
        }
        assert_eq!(
            unsafe_level(&table("[package]\nname = \"x\"\n"), None),
            None
        );
        assert_eq!(
            unsafe_level(&table("[lints.clippy]\nfoo = \"warn\"\n"), None),
            None
        );
    }

    #[test]
    fn workspace_lints_are_read_from_the_root_or_the_virtual_manifest() {
        let root = table(
            "[workspace]\nmembers = [\"m\"]\n[workspace.lints.rust]\nunsafe_code = \"forbid\"\n",
        );
        let member = table("[package]\nname = \"m\"\n[lints]\nworkspace = true\n");
        assert_eq!(
            unsafe_level(&member, Some(&root)).as_deref(),
            Some("forbid")
        );
        assert_eq!(unsafe_level(&member, None), None, "no root, no table");
        assert_eq!(unsafe_level(&root, Some(&root)).as_deref(), Some("forbid"));
        let plain = table("[workspace]\nmembers = []\n");
        assert_eq!(unsafe_level(&member, Some(&plain)), None);
    }

    #[test]
    fn a_pin_in_the_crate_or_a_parent_below_the_root_counts() {
        let root = scratch("pin");
        let krate = root.join("crates/k");
        fs::create_dir_all(&krate).expect("mkdir");
        assert!(!has_toolchain_pin(&krate, Some(&root)));
        fs::write(root.join("rust-toolchain"), "1.95\n").expect("write");
        assert!(has_toolchain_pin(&krate, Some(&root)), "pin at the root");
        fs::remove_file(root.join("rust-toolchain")).expect("rm");
        fs::write(root.join("crates/rust-toolchain.toml"), "").expect("write");
        assert!(has_toolchain_pin(&krate, Some(&root)), "pin in between");
        fs::remove_file(root.join("crates/rust-toolchain.toml")).expect("rm");
        fs::write(krate.join("rust-toolchain.toml"), "").expect("write");
        assert!(has_toolchain_pin(&krate, Some(&root)), "pin in the crate");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_pin_above_the_workspace_root_is_not_looked_for() {
        let outer = scratch("above");
        let root = outer.join("ws");
        fs::create_dir_all(&root).expect("mkdir");
        fs::write(outer.join("rust-toolchain.toml"), "").expect("write");
        assert!(!has_toolchain_pin(&root, Some(&root)));
        assert!(
            !has_toolchain_pin(&root, None),
            "no root reads only the dir"
        );
        assert!(!has_toolchain_pin(&root, Some(&outer.join("elsewhere"))));
        fs::remove_dir_all(&outer).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_pin_link_still_counts() {
        let root = scratch("dangling");
        std::os::unix::fs::symlink("nowhere", root.join("rust-toolchain.toml")).expect("link");
        assert!(has_toolchain_pin(&root, None));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_bare_directory_defaults_every_field_and_says_so() {
        let dir = scratch("bare");
        let plan = plan_for(&dir, None, "bare").expect("plan");
        assert_eq!(plan.name, "bare");
        assert_eq!(plan.settings.edition, None);
        assert_eq!(plan.settings.msrv.as_deref(), Some("1.99.1"));
        assert!(plan.write_toolchain);
        let mut notes = plan.notes.iter();
        assert!(notes
            .next()
            .is_some_and(|n| n.contains("crate.name bare") && n.contains("no Cargo.toml here")));
        assert!(notes
            .next()
            .is_some_and(|n| n.contains("crate.edition 2024")));
        assert!(notes
            .next()
            .is_some_and(|n| n.contains("crate.msrv 1.99.1")));
        assert!(notes.next().is_none());
        let named = super::plan(&Request {
            dir: &dir,
            name_flag: Some("given"),
            dir_name: "bare",
            lock_rustc: "1.99.1",
            laws: true,
        })
        .expect("plan");
        assert_eq!(named.name, "given");
        assert!(named.settings.laws);
        assert_eq!(named.notes.len(), 2, "--name needs no note");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_manifest_with_no_package_says_so_in_the_name_note() {
        let dir = scratch("virtual");
        fs::create_dir_all(dir.join("m/src")).expect("mkdir");
        fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"m\"]\nresolver = \"2\"\n",
        )
        .expect("write");
        fs::write(
            dir.join("m/Cargo.toml"),
            "[package]\nname = \"m\"\nversion = \"0.0.0\"\nedition = \"2021\"\nrust-version = \"1.95\"\n",
        )
        .expect("write");
        fs::write(dir.join("m/src/lib.rs"), "").expect("write");
        let plan = plan_for(&dir, None, "ws").expect("plan");
        assert_eq!(plan.name, "ws");
        assert_eq!(plan.settings.edition.as_deref(), Some("2021"));
        assert_eq!(plan.settings.msrv.as_deref(), Some("1.95"));
        assert_eq!(plan.notes.len(), 1, "{:?}", plan.notes);
        assert!(plan
            .notes
            .iter()
            .all(|n| n.contains("Cargo.toml has no package")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_crate_with_a_rust_version_needs_no_note() {
        let dir = scratch("krate");
        fs::create_dir_all(dir.join("src")).expect("mkdir");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"krate\"\nversion = \"0.0.0\"\nedition = \"2021\"\nrust-version = \"1.95\"\n[lints.rust]\nunsafe_code = \"forbid\"\n",
        )
        .expect("write");
        fs::write(dir.join("src/lib.rs"), "").expect("write");
        let plan = plan_for(&dir, None, "other").expect("plan");
        assert_eq!(plan.name, "krate");
        assert_eq!(plan.settings.edition.as_deref(), Some("2021"));
        assert_eq!(plan.settings.msrv.as_deref(), Some("1.95"));
        assert!(plan.settings.unsafe_forbid);
        assert!(plan.notes.is_empty(), "{:?}", plan.notes);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_crate_without_rust_version_takes_the_lock_rustc_and_says_so() {
        let dir = scratch("norv");
        fs::create_dir_all(dir.join("src")).expect("mkdir");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"norv\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        )
        .expect("write");
        fs::write(dir.join("src/lib.rs"), "").expect("write");
        let plan = plan_for(&dir, None, "norv").expect("plan");
        assert_eq!(plan.settings.msrv.as_deref(), Some("1.99.1"));
        assert!(!plan.settings.unsafe_forbid);
        assert_eq!(plan.notes.len(), 1, "{:?}", plan.notes);
        assert!(plan.notes.iter().all(|n| n.contains("no rust-version")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_edition_the_schema_lacks_is_refused() {
        let dir = scratch("e2018");
        fs::create_dir_all(dir.join("src")).expect("mkdir");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"old\"\nversion = \"0.0.0\"\nedition = \"2018\"\n",
        )
        .expect("write");
        fs::write(dir.join("src/lib.rs"), "").expect("write");
        let err = plan_for(&dir, None, "old").expect_err("2018");
        assert!(err.to_string().contains("edition 2018"), "{err}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_broken_manifest_is_an_error_not_a_default() {
        let dir = scratch("broken");
        fs::write(dir.join("Cargo.toml"), "not toml [").expect("write");
        let err = plan_for(&dir, None, "broken").expect_err("broken");
        assert!(err.to_string().contains("cargo metadata failed"), "{err}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_unsafe_note_is_printed_only_without_a_forbid_lint() {
        let mut settings = Settings::default();
        let note = unsafe_note(&settings, "forbid").expect("note");
        assert!(note.contains("unsafe_policy forbid"), "{note}");
        settings.unsafe_forbid = true;
        assert_eq!(unsafe_note(&settings, "forbid"), None);
    }

    #[test]
    fn the_lock_rustc_is_the_tools_entry() {
        assert_eq!(
            lock_rustc("[tools]\nrustc = \"1.95.0\"\n").as_deref(),
            Some("1.95.0")
        );
        assert_eq!(lock_rustc("[tools]\n"), None);
        assert_eq!(lock_rustc("not toml ["), None);
    }
}
