//! `cargo run -p xtask -- sync` against the committed tree and against
//! fixture copies of it.
//!
//! Every run goes through `cargo run -p xtask --locked`, the command CI runs.
//! Tests that drift a copy work on a fixture under `CARGO_TARGET_TMPDIR`, so
//! the committed tree is only read.
//!
//! The runs take turns on a file lock. On macOS 26.5, about one `cargo run`
//! in 300 is killed or stalls in dyld when several exec the same binary at
//! once; one at a time, 300 of 300 ran. The lock is a file so that it holds
//! across nextest's test processes too.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const TEMPLATES: &str = "crates/cargo-tops/templates";
const HARNESSES: [&str; 4] = [".agents", ".claude", ".cursor", ".grok"];
const SKILLS: [&str; 3] = ["rust-tops-adopt", "rust-tops-gate", "rust-tops-review"];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// `cargo run -p xtask --locked -- sync <args>` from the workspace. Returns
/// the exit code and stdout followed by stderr.
fn xtask(args: &[&str]) -> (i32, String) {
    let lock = fs::File::create(Path::new(env!("CARGO_TARGET_TMPDIR")).join("xtask-run.lock"))
        .expect("create the run lock");
    lock.lock().expect("take the run lock");
    let out = Command::new(env!("CARGO"))
        .current_dir(workspace())
        .args(["run", "--quiet", "-p", "xtask", "--locked", "--"])
        .args(args)
        .output()
        .expect("spawn cargo run -p xtask");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().expect("exit code"), text)
}

fn sync(root: &Path, check: bool) -> (i32, String) {
    let root = root.to_str().expect("utf-8 root");
    if check {
        xtask(&["sync", "--check", "--root", root])
    } else {
        xtask(&["sync", "--root", root])
    }
}

/// The `(source, dest)` pairs of `copies.toml` under `root`: each `to` of
/// each `[[copy]]`, with its `from`.
fn pairs(root: &Path) -> Vec<(String, String)> {
    let raw = fs::read_to_string(root.join("copies.toml")).expect("read copies.toml");
    let table: toml::Table = raw.parse().expect("parse copies.toml");
    table["copy"]
        .as_array()
        .expect("[[copy]]")
        .iter()
        .flat_map(|entry| {
            let from = entry["from"].as_str().expect("from").to_string();
            entry["to"]
                .as_array()
                .expect("to")
                .iter()
                .map(move |to| (from.clone(), to.as_str().expect("to path").to_string()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A root holding `copies.toml` and every listed source and dest, copied
/// from the committed tree with their modes.
fn fixture(label: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("xtask-sync-{label}"));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create fixture");
    fs::copy(workspace().join("copies.toml"), root.join("copies.toml")).expect("copy list");
    for (source, dest) in pairs(&workspace()) {
        for rel in [source, dest] {
            let to = root.join(&rel);
            fs::create_dir_all(to.parent().expect("parent")).expect("create dir");
            fs::copy(workspace().join(&rel), &to).expect("copy into fixture");
        }
    }
    root
}

fn flip_last_byte(path: &Path) {
    let mut bytes = fs::read(path).expect("read copy");
    *bytes.last_mut().expect("non-empty copy") ^= 1;
    fs::write(path, bytes).expect("write copy");
}

#[test]
fn the_committed_tree_matches_its_sources() {
    let (code, text) = xtask(&["sync", "--check"]);
    assert_eq!(code, 0, "{text}");
    let total = pairs(&workspace()).len();
    assert!(
        text.contains(&format!("sync --check: {total} copies match their sources")),
        "{text}"
    );
}

#[test]
fn one_byte_off_fails_the_check_until_the_byte_is_restored() {
    let root = fixture("one-byte");
    let total = pairs(&root).len();
    let copy = root.join("LAWS.bend");
    let committed = fs::read(&copy).expect("read LAWS.bend");
    flip_last_byte(&copy);

    let (code, text) = sync(&root, true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!("sync --check: 1 of {total} copies differ")),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "  - LAWS.bend differs from {TEMPLATES}/LAWS.bend\n"
        )),
        "{text}"
    );
    assert_ne!(fs::read(&copy).expect("read"), committed, "--check wrote");

    fs::write(&copy, &committed).expect("restore the byte");
    let (code, text) = sync(&root, true);
    assert_eq!(code, 0, "{text}");
}

#[test]
fn sync_rewrites_every_drifted_copy_from_its_source() {
    let root = fixture("every-copy");
    let pairs = pairs(&root);
    let sources: Vec<(String, Vec<u8>)> = pairs
        .iter()
        .map(|(source, _)| (source.clone(), fs::read(root.join(source)).expect("source")))
        .collect();
    for (_, dest) in &pairs {
        flip_last_byte(&root.join(dest));
    }

    let (code, text) = sync(&root, true);
    assert_eq!(code, 1, "{text}");
    let total = pairs.len();
    assert!(
        text.contains(&format!("{total} of {total} copies differ")),
        "{text}"
    );
    for (source, dest) in &pairs {
        assert!(
            text.contains(&format!("  - {dest} differs from {source}\n")),
            "{dest}: {text}"
        );
    }

    let (code, text) = sync(&root, false);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains(&format!("sync: wrote {total} of {total} copies")),
        "{text}"
    );
    for (source, dest) in &pairs {
        assert!(
            text.contains(&format!("wrote {dest} from {source}\n")),
            "{text}"
        );
        assert_eq!(
            fs::read(root.join(dest)).expect("dest"),
            fs::read(root.join(source)).expect("source"),
            "{dest}"
        );
    }
    for (source, bytes) in &sources {
        assert_eq!(
            &fs::read(root.join(source)).expect("source"),
            bytes,
            "sync wrote {source}"
        );
    }
    let (code, text) = sync(&root, true);
    assert_eq!(code, 0, "{text}");
}

#[test]
fn the_schema_template_is_written_from_schema_not_the_reverse() {
    let root = fixture("schema");
    let schema = root.join("schema/rust-tops.schema.json");
    let committed = fs::read(&schema).expect("schema");
    flip_last_byte(&schema);
    let drifted = fs::read(&schema).expect("schema");

    let (code, text) = sync(&root, false);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains(&format!(
            "wrote {TEMPLATES}/rust-tops.schema.json from schema/rust-tops.schema.json"
        )),
        "{text}"
    );
    assert_eq!(
        fs::read(&schema).expect("schema"),
        drifted,
        "sync wrote schema/"
    );
    assert_eq!(
        fs::read(root.join(TEMPLATES).join("rust-tops.schema.json")).expect("template"),
        drifted
    );
    assert_ne!(drifted, committed);
}

#[test]
fn a_missing_copy_fails_the_check_and_sync_writes_it() {
    let root = fixture("missing");
    let total = pairs(&root).len();
    fs::remove_file(root.join("scripts/fetch-bend.sh")).expect("remove");
    fs::remove_dir_all(root.join(".grok")).expect("remove .grok");

    let (code, text) = sync(&root, true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!(
            "  - scripts/fetch-bend.sh is missing; its source is {TEMPLATES}/fetch-bend.sh\n"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("7 of {total} copies differ")),
        "{text}"
    );

    let (code, text) = sync(&root, false);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains(&format!("sync: wrote 7 of {total} copies")),
        "{text}"
    );
    assert!(root.join(".grok/skills/rust-tops-gate/SKILL.md").is_file());
    let (code, text) = sync(&root, true);
    assert_eq!(code, 0, "{text}");
}

#[cfg(unix)]
#[test]
fn the_executable_bit_is_part_of_the_copy() {
    use std::os::unix::fs::PermissionsExt;
    let root = fixture("mode");
    let script = root.join("scripts/fetch-bend.sh");
    let mode = |path: &Path| fs::metadata(path).expect("stat").permissions().mode() & 0o777;
    assert_eq!(
        mode(&root.join(TEMPLATES).join("fetch-bend.sh")) & 0o111,
        0o111
    );
    fs::set_permissions(&script, fs::Permissions::from_mode(0o644)).expect("chmod");

    let (code, text) = sync(&root, true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!(
            "  - scripts/fetch-bend.sh has a different executable bit from {TEMPLATES}/fetch-bend.sh\n"
        )),
        "{text}"
    );

    let (code, text) = sync(&root, false);
    assert_eq!(code, 0, "{text}");
    assert_eq!(mode(&script), 0o755);
    assert_eq!(mode(&root.join("LAWS.bend")), 0o644, "{text}");
    let (code, text) = sync(&root, true);
    assert_eq!(code, 0, "{text}");

    // A plain file that turned executable is drift too.
    fs::set_permissions(root.join("LAWS.bend"), fs::Permissions::from_mode(0o755)).expect("chmod");
    let (code, text) = sync(&root, false);
    assert_eq!(code, 0, "{text}");
    assert_eq!(mode(&root.join("LAWS.bend")), 0o644, "{text}");
}

#[cfg(unix)]
#[test]
fn a_symlinked_copy_fails_and_sync_does_not_follow_it() {
    let root = fixture("symlink");
    let total = pairs(&root).len();
    // Same bytes when followed: only the file type is wrong.
    let skill = root.join(".claude/skills/rust-tops-gate/SKILL.md");
    fs::remove_file(&skill).expect("remove");
    std::os::unix::fs::symlink(
        root.join(TEMPLATES).join("skills/rust-tops-gate/SKILL.md"),
        &skill,
    )
    .expect("symlink");
    // Different bytes: sync must not write through the link.
    let outside = root.join("outside.toml");
    fs::write(&outside, "outside\n").expect("write outside");
    let clippy = root.join("config/drop-in/clippy.toml");
    fs::remove_file(&clippy).expect("remove");
    std::os::unix::fs::symlink(&outside, &clippy).expect("symlink");
    // A directory where a copy belongs.
    fs::remove_file(root.join("docs/REVIEW.md")).expect("remove");
    fs::create_dir(root.join("docs/REVIEW.md")).expect("mkdir");

    for check in [true, false] {
        let (code, text) = sync(&root, check);
        assert_eq!(code, 1, "{text}");
        assert!(
            text.contains(
                ".claude/skills/rust-tops-gate/SKILL.md is a symlink; copies are regular files"
            ),
            "{text}"
        );
        assert!(
            text.contains("config/drop-in/clippy.toml is a symlink; copies are regular files"),
            "{text}"
        );
        assert!(
            text.contains("docs/REVIEW.md is not a regular file"),
            "{text}"
        );
        if check {
            assert!(
                text.contains(&format!("3 of {total} copies differ")),
                "{text}"
            );
        } else {
            assert!(
                text.contains(&format!("sync: wrote 0 of {total} copies")),
                "{text}"
            );
        }
    }
    for link in [&skill, &clippy] {
        let meta = fs::symlink_metadata(link).expect("stat");
        assert!(
            meta.file_type().is_symlink(),
            "sync replaced {}",
            link.display()
        );
    }
    assert_eq!(fs::read_to_string(&outside).expect("outside"), "outside\n");
    assert!(root.join("docs/REVIEW.md").is_dir());
}

#[cfg(unix)]
#[test]
fn a_symlinked_directory_above_a_copy_fails_and_sync_does_not_write_through_it() {
    let root = fixture("linked-dir");
    let total = pairs(&root).len();
    // Same bytes when followed: `.cursor/skills` is a link to `.agents/skills`.
    let skills = root.join(".cursor/skills");
    fs::remove_dir_all(&skills).expect("remove .cursor/skills");
    std::os::unix::fs::symlink(root.join(".agents/skills"), &skills).expect("symlink");
    // Different bytes: `config/drop-in` is a link to a drifted directory.
    let outside = root.join("outside-drop-in");
    fs::rename(root.join("config/drop-in"), &outside).expect("move config/drop-in");
    flip_last_byte(&outside.join("clippy.toml"));
    let drifted = fs::read(outside.join("clippy.toml")).expect("read");
    std::os::unix::fs::symlink(&outside, root.join("config/drop-in")).expect("symlink");
    let linked: Vec<String> = pairs(&root)
        .into_iter()
        .map(|(_, dest)| dest)
        .filter(|dest| dest.starts_with(".cursor/skills/") || dest.starts_with("config/drop-in/"))
        .collect();
    assert!(linked.len() > 6, "{linked:?}");

    for check in [true, false] {
        let (code, text) = sync(&root, check);
        assert_eq!(code, 1, "{text}");
        for dest in &linked {
            let dir = if dest.starts_with(".cursor/") {
                ".cursor/skills"
            } else {
                "config/drop-in"
            };
            assert!(
                text.contains(&format!(
                    "  - {dest} is under {dir}, which is a symlink; copies sit in real directories, so remove the link {dir}\n"
                )),
                "{dest}: {text}"
            );
        }
        if check {
            assert!(
                text.contains(&format!("{} of {total} copies differ", linked.len())),
                "{text}"
            );
        } else {
            assert!(
                text.contains(&format!("sync: wrote 0 of {total} copies")),
                "{text}"
            );
        }
    }
    for link in [&skills, &root.join("config/drop-in")] {
        let meta = fs::symlink_metadata(link).expect("stat");
        assert!(
            meta.file_type().is_symlink(),
            "sync replaced {}",
            link.display()
        );
    }
    assert_eq!(
        fs::read(outside.join("clippy.toml")).expect("read"),
        drifted
    );
}

#[test]
fn a_list_that_writes_a_source_or_a_non_copy_is_refused() {
    let root = fixture("bad-list");
    let pair =
        |source: &str, dest: &str| format!("[[copy]]\nfrom = \"{source}\"\nto = [\"{dest}\"]\n");
    let drop_in = format!("{TEMPLATES}/drop-in/rust-tops.yml");
    let golden = "crates/cargo-tops/tests/golden/init/binary-cli/.github/workflows/rust-tops.yml";
    let cases = [
        (
            pair(
                &format!("{TEMPLATES}/rust-tops.schema.json"),
                "schema/rust-tops.schema.json",
            ),
            "schema/rust-tops.schema.json",
            "sync never writes under schema/",
        ),
        (
            pair(&drop_in, ".github/workflows/rust-tops.yml"),
            ".github/workflows/rust-tops.yml",
            "sync never writes under .github/",
        ),
        (
            pair(&drop_in, golden),
            golden,
            "sync never writes under crates/cargo-tops/tests/golden/",
        ),
        (
            pair("config/drop-in/clippy.toml", "clippy.toml"),
            "clippy.toml",
            "a source is under crates/cargo-tops/templates/ or is schema/rust-tops.schema.json",
        ),
        (
            pair(&format!("{TEMPLATES}/AGENTS.md"), "../AGENTS.md"),
            "AGENTS.md",
            "a listed path is relative, with no `.` or `..`",
        ),
        (
            pair(
                &format!("{TEMPLATES}/AGENTS.md"),
                "/tmp/xtask-sync-AGENTS.md",
            ),
            "AGENTS.md",
            "a listed path is relative",
        ),
        (
            pair(&format!("{TEMPLATES}/AGENTS.md"), ""),
            "AGENTS.md",
            "a listed path is relative",
        ),
        (
            pair("", "AGENTS.md"),
            "AGENTS.md",
            "a listed path is relative",
        ),
        (
            pair(&format!("{TEMPLATES}/AGENTS.md"), "./AGENTS.md"),
            "AGENTS.md",
            "a listed path is relative",
        ),
        (
            pair(&format!("{TEMPLATES}/AGENTS.md"), "AGENTS.md")
                + &pair(&format!("{TEMPLATES}/CLAUDE.md"), "AGENTS.md"),
            "AGENTS.md",
            "dest AGENTS.md: listed twice",
        ),
        (
            pair(&format!("{TEMPLATES}/AGENTS.md"), "AGENTS.md")
                + &pair(
                    &format!("{TEMPLATES}/CLAUDE.md"),
                    &format!("{TEMPLATES}/AGENTS.md"),
                ),
            "AGENTS.md",
            "it is also a source",
        ),
        (
            "copy = []\n".to_string(),
            "AGENTS.md",
            "copies.toml lists no copy",
        ),
        (
            format!("[[copy]]\nfrom = \"{TEMPLATES}/AGENTS.md\"\nto = []\n"),
            "AGENTS.md",
            "its `to` lists no copy",
        ),
        (
            pair(&format!("{TEMPLATES}/AGENTS.md"), "AGENTS.md")
                + &pair(&format!("{TEMPLATES}/AGENTS.md"), "docs/AGENTS.md"),
            "AGENTS.md",
            "listed twice; one table per source",
        ),
        (
            format!("[[copy]]\nfrom = \"{TEMPLATES}/AGENTS.md\"\nto = [\"AGENTS.md\"]\nmode = 1\n"),
            "AGENTS.md",
            "parse",
        ),
        (
            format!("[[copy]]\nsource = \"{TEMPLATES}/AGENTS.md\"\ndest = \"AGENTS.md\"\n"),
            "AGENTS.md",
            "parse",
        ),
    ];
    flip_last_byte(&root.join("AGENTS.md"));
    flip_last_byte(&root.join("clippy.toml"));
    for (list, guarded, message) in cases {
        fs::write(root.join("copies.toml"), &list).expect("write list");
        let before = fs::read(root.join(guarded)).ok();
        let (code, text) = sync(&root, false);
        assert_eq!(code, 2, "{list}\n{text}");
        assert!(text.contains(message), "{list}\n{text}");
        assert!(!text.contains("wrote"), "{list}\n{text}");
        assert_eq!(fs::read(root.join(guarded)).ok(), before, "{list}");
    }
    assert!(!Path::new("/tmp/xtask-sync-AGENTS.md").exists());
}

#[test]
fn a_source_that_is_missing_or_a_link_is_an_error() {
    let root = fixture("bad-source");
    fs::remove_file(root.join(TEMPLATES).join("CLAUDE.md")).expect("remove");
    let (code, text) = sync(&root, true);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("templates/CLAUDE.md"), "{text}");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            root.join("CLAUDE.md"),
            root.join(TEMPLATES).join("CLAUDE.md"),
        )
        .expect("symlink");
        let (code, text) = sync(&root, false);
        assert_eq!(code, 2, "{text}");
        assert!(
            text.contains(&format!(
                "source {}",
                root.join(TEMPLATES).join("CLAUDE.md").display()
            )),
            "{text}"
        );
        assert!(text.contains("is not a regular file"), "{text}");
    }
}

#[test]
fn a_copy_that_cannot_be_read_is_an_error() {
    // `.grok` is a file, so every `.grok/...` dest fails to stat with
    // ENOTDIR. That is an error, not a missing copy to write.
    let root = fixture("unreadable");
    fs::remove_dir_all(root.join(".grok")).expect("remove .grok");
    fs::write(root.join(".grok"), "a file\n").expect("write .grok");
    for check in [true, false] {
        let (code, text) = sync(&root, check);
        assert_eq!(code, 2, "{text}");
        assert!(text.contains("error: stat "), "{text}");
        assert!(text.contains(".grok/skills"), "{text}");
        assert!(!text.contains("missing"), "{text}");
    }
    assert_eq!(
        fs::read_to_string(root.join(".grok")).expect("read"),
        "a file\n"
    );
}

#[test]
fn bad_arguments_exit_2_and_name_the_usage() {
    let root = fixture("args");
    let root = root.to_str().expect("utf-8");
    for args in [
        vec![],
        vec!["nope"],
        vec!["sync", "--root"],
        vec!["sync", "--bogus"],
    ] {
        let (code, text) = xtask(&args);
        assert_eq!(code, 2, "{args:?}: {text}");
        assert!(
            text.contains("usage: cargo run -p xtask -- sync [--check] [--root DIR]"),
            "{text}"
        );
    }
    let empty = Path::new(env!("CARGO_TARGET_TMPDIR")).join("xtask-sync-no-list");
    fs::create_dir_all(&empty).expect("mkdir");
    let (code, text) = xtask(&["sync", "--check", "--root", empty.to_str().expect("utf-8")]);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("read "), "{text}");
    assert!(text.contains("copies.toml"), "{text}");
    let (code, text) = xtask(&["sync", "--root", root, "--check"]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("copies match their sources"), "{text}");
}

#[test]
fn the_list_names_the_hand_copies_and_nothing_else() {
    let pairs = pairs(&workspace());
    let has = |source: &str, dest: &str| pairs.iter().any(|(s, d)| s == source && d == dest);
    for skill in SKILLS {
        for file in ["SKILL.md".to_string(), format!("{skill}.nu")] {
            for harness in HARNESSES {
                assert!(
                    has(
                        &format!("{TEMPLATES}/skills/{skill}/{file}"),
                        &format!("{harness}/skills/{skill}/{file}")
                    ),
                    "{harness} {skill} {file}"
                );
            }
        }
    }
    for dest in ["LAWS.bend", "laws/LAWS.bend"] {
        assert!(has(&format!("{TEMPLATES}/LAWS.bend"), dest), "{dest}");
    }
    assert!(has(
        &format!("{TEMPLATES}/drop-in/rust-tops.yml"),
        "config/drop-in/rust-tops.yml"
    ));
    assert!(has(
        "schema/rust-tops.schema.json",
        &format!("{TEMPLATES}/rust-tops.schema.json")
    ));
    for (_, dest) in &pairs {
        assert!(
            !dest.starts_with("crates/cargo-tops/tests/golden/"),
            "{dest}"
        );
        assert!(!dest.starts_with(".github/"), "{dest}");
        assert!(!dest.contains("LICENSE"), "{dest}");
        assert_ne!(dest, "deny.toml");
        assert_ne!(dest, "rust-toolchain.toml");
    }
    assert_eq!(pairs.len(), 46);
}

/// A link at `config` that comes and goes while sync writes
/// `config/AGENTS.md` is refused or never there: sync never writes through
/// it. The flicker never removes a real directory, so once sync makes
/// `config` the link cannot come back.
#[cfg(unix)]
#[test]
fn a_directory_link_that_appears_while_sync_writes_is_never_followed() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("xtask-sync-flicker");
    let _ = fs::remove_dir_all(&root);
    let source = format!("{TEMPLATES}/AGENTS.md");
    fs::create_dir_all(root.join(TEMPLATES)).expect("create templates");
    fs::copy(workspace().join(&source), root.join(&source)).expect("copy source");
    fs::write(
        root.join("copies.toml"),
        format!("[[copy]]\nfrom = \"{source}\"\nto = [\"config/AGENTS.md\"]\n"),
    )
    .expect("write list");
    let outside = root.join("outside-config");
    fs::create_dir_all(&outside).expect("mkdir");
    let config = root.join("config");

    for round in 0..25 {
        let stop = Arc::new(AtomicBool::new(false));
        let links = {
            let (at, to, stop) = (config.clone(), outside.clone(), Arc::clone(&stop));
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let _ = std::os::unix::fs::symlink(&to, &at);
                    let _ = fs::remove_file(&at);
                }
            })
        };
        let (code, text) = sync(&root, false);
        stop.store(true, Ordering::Relaxed);
        links.join().expect("flicker thread");
        assert!(
            code == 0
                || text.contains("config/AGENTS.md is under config, which is a symlink")
                || text.contains("config/AGENTS.md: ")
                    && text.contains(
                        "config is a symlink or not a directory; sync does not follow it"
                    ),
            "round {round}: {text}"
        );
        assert_eq!(
            fs::read_dir(&outside).expect("read outside").count(),
            0,
            "round {round}: sync wrote through a link that appeared while it ran: {text}"
        );
        let _ = fs::remove_file(&config);
        let _ = fs::remove_dir_all(&config);
    }
}
