//! Portable agent skills written by `cargo tops init --skills`.
//!
//! `.agents/skills/<name>/` is the canonical tree. `.claude/skills/<name>/`,
//! `.cursor/skills/<name>/`, and `.grok/skills/<name>/` are the same bytes,
//! written as ordinary files.
//! A symlink is not enough: Windows and some packaging drop it. The drift
//! test compares every copy. `CLAUDE.md` is one line that imports `AGENTS.md`.

use std::path::Path;

use anyhow::Result;

use crate::write_file;

/// Skill roots `init --skills` fills with the same files.
const SKILL_ROOTS: &[&str] = &[
    ".agents/skills",
    ".claude/skills",
    ".cursor/skills",
    ".grok/skills",
];

pub const CLAUDE_MD: &str = include_str!("../templates/CLAUDE.md");
pub const REVIEW_MD: &str = include_str!("../templates/docs/REVIEW.md");

struct Skill {
    name: &'static str,
    skill_md: &'static str,
    script: &'static str,
}

const SKILLS: &[Skill] = &[
    Skill {
        name: "rust-tops-adopt",
        skill_md: include_str!("../templates/skills/rust-tops-adopt/SKILL.md"),
        script: include_str!("../templates/skills/rust-tops-adopt/rust-tops-adopt.nu"),
    },
    Skill {
        name: "rust-tops-gate",
        skill_md: include_str!("../templates/skills/rust-tops-gate/SKILL.md"),
        script: include_str!("../templates/skills/rust-tops-gate/rust-tops-gate.nu"),
    },
    Skill {
        name: "rust-tops-review",
        skill_md: include_str!("../templates/skills/rust-tops-review/SKILL.md"),
        script: include_str!("../templates/skills/rust-tops-review/rust-tops-review.nu"),
    },
];

pub fn emit(dir: &Path, force: bool) -> Result<()> {
    for skill in SKILLS {
        write_skill(dir, skill, force)?;
    }
    write_file(dir.join("CLAUDE.md"), CLAUDE_MD, force)?;
    write_file(dir.join("docs/REVIEW.md"), REVIEW_MD, force)?;
    Ok(())
}

fn write_skill(dir: &Path, skill: &Skill, force: bool) -> Result<()> {
    let script = format!("{}.nu", skill.name);
    for root in SKILL_ROOTS {
        let skill_dir = dir.join(root).join(skill.name);
        write_file(skill_dir.join("SKILL.md"), skill.skill_md, force)?;
        write_file(skill_dir.join(&script), skill.script, force)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn claude_md_imports_agents_md() {
        assert_eq!(CLAUDE_MD, "@AGENTS.md\n");
    }

    #[test]
    fn review_doc_matches_the_workspace_copy() {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("docs/REVIEW.md");
        if !source.is_file() {
            eprintln!(
                "review drift check skipped; {} is outside the package",
                source.display()
            );
            return;
        }
        let body = std::fs::read_to_string(&source).expect("read docs/REVIEW.md");
        assert_eq!(REVIEW_MD, body);
    }
}
