//! Which report rows belong to the crate an instance names (§7.2, §16).
//!
//! The coverage run is workspace-wide, so a test in one member can cover
//! another, and its reports name files from every member. A row belongs to
//! the instance when its file is under the crate root: the directory of the
//! package `crate.name` names in `cargo metadata`, or the gate directory
//! when no package has that name. A file outside the root is another
//! crate's and is not judged here.
//!
//! A production file is a file under the root that no exclude glob
//! matches. A glob matches the path relative to the root, or any trailing
//! part of it that starts at a directory, so `tests/**` also matches
//! `src/tests/a.rs`, the way `cargo llvm-cov` leaves out every `tests/`
//! directory. `*` and `?` stay inside one path component; `**` spans any
//! number of components.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// Directories `cargo llvm-cov` leaves out of its report by default, so no
/// floor may count them as production.
pub(crate) const TEST_DIRS: [&str; 3] = ["tests/**", "benches/**", "examples/**"];

/// The crate root and the globs that leave files out of it.
#[derive(Debug)]
pub(crate) struct Scope {
    /// The gate directory. `cargo crap` paths are relative to it.
    dir: PathBuf,
    root: PathBuf,
    globs: Vec<String>,
}

impl Scope {
    /// `dir` is the gate directory; `root` is the crate root under it or
    /// equal to it. Both are canonicalized when they exist.
    pub(crate) fn new(dir: &Path, root: &Path, globs: &[String]) -> Self {
        let mut all: Vec<String> = TEST_DIRS.iter().map(|glob| (*glob).to_string()).collect();
        all.extend(globs.iter().cloned());
        Self {
            dir: canonical(dir),
            root: canonical(root),
            globs: all,
        }
    }

    /// The crate root this scope judges.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// `file` relative to the crate root, with `/` separators, when it is
    /// a production file. A relative `file` is relative to the gate
    /// directory, as `cargo crap` writes it.
    pub(crate) fn production(&self, file: &str) -> Option<String> {
        let rel = self.relative(file)?;
        (!self.globs.iter().any(|glob| glob_matches(glob, &rel))).then_some(rel)
    }

    fn relative(&self, file: &str) -> Option<String> {
        let path = self.dir.join(file);
        let path = canonical(&path);
        let rel = path.strip_prefix(&self.root).ok()?;
        let parts: Vec<String> = rel
            .components()
            .filter_map(|part| match part {
                Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        (!parts.is_empty()).then(|| parts.join("/"))
    }
}

/// The real path when there is one, else `path` as given.
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// The crate root for `name`: `dir` when it has no `Cargo.toml` or its
/// manifest is that package; otherwise the directory of that package's
/// manifest in `cargo metadata` run on `dir`, or `dir` when there is no such
/// package or `cargo metadata` does not answer. Only a workspace whose root
/// instance names a member spawns `cargo metadata`.
pub(crate) fn crate_root(dir: &Path, name: &str) -> PathBuf {
    let manifest = fs::read_to_string(dir.join("Cargo.toml"));
    let here = manifest
        .as_deref()
        .map_or(true, |raw| names_package(raw, name));
    if here {
        return dir.to_path_buf();
    }
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(dir)
        .output();
    output
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| package_dir(&String::from_utf8_lossy(&out.stdout), name))
        .unwrap_or_else(|| dir.to_path_buf())
}

/// Whether the manifest text declares `[package] name = "<name>"`.
fn names_package(manifest: &str, name: &str) -> bool {
    manifest
        .parse::<toml::Table>()
        .ok()
        .and_then(|table| {
            let found = table.get("package")?.get("name")?.as_str()? == name;
            Some(found)
        })
        .unwrap_or(false)
}

/// The manifest directory of package `name` in `cargo metadata` JSON.
pub(crate) fn package_dir(metadata: &str, name: &str) -> Option<PathBuf> {
    let doc: serde_json::Value = serde_json::from_str(metadata).ok()?;
    let manifest = doc
        .get("packages")?
        .as_array()?
        .iter()
        .find(|package| package.get("name").and_then(|n| n.as_str()) == Some(name))?
        .get("manifest_path")?
        .as_str()?;
    Path::new(manifest).parent().map(Path::to_path_buf)
}

/// Whether `glob` matches the `/`-separated `rel`, or a trailing part of it
/// that starts at a directory.
pub(crate) fn glob_matches(glob: &str, rel: &str) -> bool {
    let pattern: Vec<&str> = glob.split('/').filter(|part| !part.is_empty()).collect();
    let parts: Vec<&str> = rel.split('/').collect();
    (0..parts.len()).any(|start| {
        parts
            .get(start..)
            .is_some_and(|tail| path_matches(&pattern, tail))
    })
}

fn path_matches(pattern: &[&str], parts: &[&str]) -> bool {
    match pattern.split_first() {
        None => parts.is_empty(),
        Some((&"**", rest)) => (0..=parts.len()).any(|skip| {
            parts
                .get(skip..)
                .is_some_and(|tail| path_matches(rest, tail))
        }),
        Some((head, rest)) => parts.split_first().is_some_and(|(part, tail)| {
            name_matches(head.as_bytes(), part.as_bytes()) && path_matches(rest, tail)
        }),
    }
}

/// One path component against one glob component: `*` any run, `?` one byte.
fn name_matches(pattern: &[u8], name: &[u8]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some((b'*', rest)) => (0..=name.len()).any(|skip| {
            name.get(skip..)
                .is_some_and(|tail| name_matches(rest, tail))
        }),
        Some((want, rest)) => name
            .split_first()
            .is_some_and(|(got, tail)| (*want == b'?' || want == got) && name_matches(rest, tail)),
    }
}

#[cfg(test)]
mod tests {
    use super::{crate_root, glob_matches, names_package, package_dir, Scope};
    use std::fs;
    use std::path::Path;

    #[test]
    fn a_glob_matches_at_the_root_or_below_a_directory() {
        let cases: &[(&str, &str, bool)] = &[
            ("tests/**", "tests/a.rs", true),
            ("tests/**", "src/tests/a.rs", true),
            ("tests/**", "src/tests.rs", false),
            ("tests/**", "src/contests/a.rs", false),
            ("src/generated/**", "crates/c/src/generated/x/y.rs", true),
            ("src/generated/**", "src/gen/x.rs", false),
            ("**/build.rs", "build.rs", true),
            ("**/build.rs", "a/b/build.rs", true),
            ("src/*.pb.rs", "src/wire.pb.rs", true),
            ("src/*.pb.rs", "src/a/wire.pb.rs", false),
            ("src/?.rs", "src/a.rs", true),
            ("src/?.rs", "src/ab.rs", false),
            ("src/main.rs", "src/main.rs", true),
            ("src/main.rs", "src/main.rsx", false),
            ("src/lib.rs", "src", false),
        ];
        for &(glob, rel, want) in cases {
            assert_eq!(glob_matches(glob, rel), want, "{glob} vs {rel}");
        }
    }

    #[test]
    fn the_crate_root_is_the_named_package_directory() {
        let metadata = r#"{"packages":[
            {"name":"other","manifest_path":"/w/crates/other/Cargo.toml"},
            {"name":"tool","manifest_path":"/w/crates/tool/Cargo.toml"}]}"#;
        assert_eq!(
            package_dir(metadata, "tool").as_deref(),
            Some(Path::new("/w/crates/tool"))
        );
        assert_eq!(package_dir(metadata, "absent"), None);
        assert_eq!(package_dir("not json", "tool"), None);
    }

    #[test]
    fn production_files_are_under_the_root_and_not_excluded() {
        let base = std::env::temp_dir().join(format!("cargo-tops-scope-{}", std::process::id()));
        let root = base.join("crates/tool");
        let files = [
            "crates/tool/src/main.rs",
            "crates/tool/src/lib.rs",
            "crates/tool/src/tests/a.rs",
            "crates/tool/src/generated/x.rs",
            "crates/tool/benches/b.rs",
            "crates/other/src/lib.rs",
        ];
        for file in files {
            let path = base.join(file);
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(&path, "").expect("write");
        }
        let scope = Scope::new(&base, &root, &["src/generated/**".to_string()]);
        let abs = root.join("src/lib.rs").display().to_string();
        let cases: &[(&str, Option<&str>)] = &[
            ("./crates/tool/src/main.rs", Some("src/main.rs")),
            (abs.as_str(), Some("src/lib.rs")),
            ("crates/tool/src/tests/a.rs", None),
            ("crates/tool/src/generated/x.rs", None),
            ("crates/tool/benches/b.rs", None),
            ("crates/other/src/lib.rs", None),
            ("crates/tool", None),
        ];
        for &(file, want) in cases {
            assert_eq!(scope.production(file).as_deref(), want, "{file}");
        }
        assert!(scope.root().ends_with("crates/tool"));
        fs::remove_dir_all(&base).expect("cleanup");
    }

    #[test]
    fn a_manifest_names_its_package() {
        let manifest = "[package]\nname = \"tool\"\nversion = \"0.1.0\"\n";
        assert!(names_package(manifest, "tool"));
        assert!(!names_package(manifest, "other"));
        assert!(!names_package("[workspace]\nmembers = [\"a\"]\n", "tool"));
        assert!(!names_package("not = [toml", "tool"));
    }

    #[test]
    fn crate_root_asks_cargo_metadata_and_falls_back_to_the_gate_dir() {
        let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace = crate_dir.join("../..");
        let found = crate_root(&workspace, "cargo-tops");
        assert_eq!(
            found.canonicalize().expect("found"),
            crate_dir.canonicalize().expect("crate dir")
        );
        assert_eq!(crate_root(&workspace, "no-such-package"), workspace);
        let empty = std::env::temp_dir().join(format!("cargo-tops-noroot-{}", std::process::id()));
        fs::create_dir_all(&empty).expect("mkdir");
        assert_eq!(crate_root(&empty, "cargo-tops"), empty);
        fs::remove_dir_all(&empty).expect("cleanup");
    }
}
