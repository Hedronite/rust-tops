//! Diff the Bend pack between two trees.
//!
//! Std-only. A pure end-of-file append is allowed. Any other change to
//! `laws/LAWS.bend`, a NAMES removal or reorder, or a removed or rewritten
//! claim in `PROOF.bend`'s `main` type needs the `laws-change-approved` label.
//! Proof-body-only edits pass. The label is declared intent, not authorization.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            let _ = writeln!(io::stderr(), "{msg}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mut base: Option<PathBuf> = None;
    let mut head: Option<PathBuf> = None;
    let mut approved = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--approved" => approved = true,
            "--base-dir" => {
                base = Some(PathBuf::from(
                    args.next().ok_or("law-diff: --base-dir needs a path")?,
                ));
            }
            "--head-dir" => {
                head = Some(PathBuf::from(
                    args.next().ok_or("law-diff: --head-dir needs a path")?,
                ));
            }
            other => return Err(format!("law-diff: unknown argument {other}")),
        }
    }
    let base = base.ok_or("law-diff: --base-dir is required")?;
    let head = head.ok_or("law-diff: --head-dir is required")?;
    let msgs = diff_pack(&base, &head)?;
    if msgs.is_empty() {
        return Ok(());
    }
    if approved {
        println!(
            "law-diff: laws-change-approved (declared intent; code-owner review still required)"
        );
        return Ok(());
    }
    Err(msgs.join("\n"))
}

fn diff_pack(base: &Path, head: &Path) -> Result<Vec<String>, String> {
    let mut msgs = Vec::new();
    msgs.extend(diff_laws(
        &read(base, "laws/LAWS.bend")?,
        &read(head, "laws/LAWS.bend")?,
    ));
    msgs.extend(diff_names(
        &read(base, "laws/NAMES")?,
        &read(head, "laws/NAMES")?,
    ));
    msgs.extend(diff_proof(
        &read(base, "laws/PROOF.bend")?,
        &read(head, "laws/PROOF.bend")?,
    ));
    Ok(msgs)
}

fn read(root: &Path, rel: &str) -> Result<String, String> {
    let path = root.join(rel);
    fs::read_to_string(&path)
        .map_err(|err| format!("law-diff: cannot read {}: {err}", path.display()))
}

fn diff_laws(base: &str, head: &str) -> Vec<String> {
    if let Some(rest) = append_rest(base, head) {
        if rest.trim().is_empty() {
            return Vec::new();
        }
        // A law and its `def` share a name in this pack. That pair is one
        // new declaration. Re-declare means the name already exists in BASE.
        let known: Vec<String> = decl_names(base);
        let mut msgs = Vec::new();
        for name in decl_names(rest) {
            if known.iter().any(|have| have == &name) {
                msgs.push(format!("law-diff: append re-declares existing name {name}"));
            }
        }
        if !msgs.is_empty() {
            return msgs;
        }
        if !append_is_blocks_only(rest) {
            return vec!["law-diff: non-append change in laws/LAWS.bend".to_string()];
        }
        return Vec::new();
    }

    let base_laws = law_bodies(base);
    let head_laws = law_bodies(head);
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut added = Vec::new();
    for (name, body) in &base_laws {
        match head_laws.get(name) {
            None => removed.push(name.clone()),
            Some(got) if got != body => changed.push(name.clone()),
            Some(_) => {}
        }
    }
    for name in head_laws.keys() {
        if !base_laws.contains_key(name) {
            added.push(name.clone());
        }
    }
    if !removed.is_empty() {
        let mut msgs: Vec<String> = removed
            .iter()
            .map(|name| format!("law-diff: removed law {name} (needs laws-change-approved)"))
            .collect();
        msgs.extend(
            added
                .iter()
                .map(|name| format!("law-diff: added law {name} (needs laws-change-approved)")),
        );
        return msgs;
    }
    if !changed.is_empty() {
        return changed
            .iter()
            .map(|name| format!("law-diff: changed law {name} (needs laws-change-approved)"))
            .collect();
    }
    vec!["law-diff: non-append change in laws/LAWS.bend".to_string()]
}

fn diff_names(base: &str, head: &str) -> Vec<String> {
    let base_rows = rows(base);
    let head_rows = rows(head);
    if head_rows.starts_with(&base_rows) {
        return Vec::new();
    }
    let removed: Vec<&str> = base_rows
        .iter()
        .filter(|name| !head_rows.iter().any(|got| got == *name))
        .map(String::as_str)
        .collect();
    if !removed.is_empty() {
        return removed
            .iter()
            .map(|name| format!("law-diff: removed NAMES row {name} (needs laws-change-approved)"))
            .collect();
    }
    vec!["law-diff: reordered NAMES (needs laws-change-approved)".to_string()]
}

fn diff_proof(base: &str, head: &str) -> Vec<String> {
    let base_claims = claims(base);
    let head_claims = claims(head);
    if head_claims.starts_with(&base_claims) {
        return Vec::new();
    }
    let removed = base_claims.iter().any(|claim| !head_claims.contains(claim));
    if removed {
        vec!["law-diff: removed proof claim (needs laws-change-approved)".to_string()]
    } else {
        vec!["law-diff: changed proof claim (needs laws-change-approved)".to_string()]
    }
}

fn append_rest<'a>(base: &str, head: &'a str) -> Option<&'a str> {
    let base = strip_one_nl(base);
    let head = strip_one_nl(head);
    let rest = head.strip_prefix(base)?;
    // The matched bytes must end on a line boundary. A longer last line is an
    // edit of that line, not a new declaration appended after it.
    if rest.is_empty() || rest.starts_with('\n') {
        Some(rest)
    } else {
        None
    }
}

fn strip_one_nl(text: &str) -> &str {
    text.strip_suffix('\n').unwrap_or(text)
}

fn append_is_blocks_only(rest: &str) -> bool {
    let mut saw_decl = false;
    for line in rest.lines() {
        if line.is_empty() {
            continue;
        }
        if !saw_decl {
            // The first content line has to open a new top-level declaration.
            // An indented line would extend the last block of BASE.
            if decl_name(line).is_none() {
                return false;
            }
            saw_decl = true;
            continue;
        }
        if !(line.starts_with(' ') || line.starts_with('\t') || decl_name(line).is_some()) {
            return false;
        }
    }
    true
}

fn decl_names(src: &str) -> Vec<String> {
    src.lines().filter_map(decl_name).collect()
}

fn decl_name(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("type ")
        .or_else(|| line.strip_prefix("def "))
        .or_else(|| line.strip_prefix("law "))?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn law_bodies(src: &str) -> BTreeMap<String, String> {
    let lines: Vec<&str> = src.lines().collect();
    let mut bodies = BTreeMap::new();
    let mut index = 0;
    while index < lines.len() {
        if !lines[index].starts_with("law ") {
            index += 1;
            continue;
        }
        let Some(name) = decl_name(lines[index]) else {
            index += 1;
            continue;
        };
        let start = index;
        index += 1;
        while index < lines.len() && !is_top_decl(lines[index]) {
            index += 1;
        }
        bodies
            .entry(name)
            .or_insert_with(|| lines[start..index].join("\n"));
    }
    bodies
}

fn is_top_decl(line: &str) -> bool {
    !line.is_empty()
        && !line.starts_with(' ')
        && !line.starts_with('\t')
        && decl_name(line).is_some()
}

fn rows(src: &str) -> Vec<String> {
    src.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn claims(proof: &str) -> Vec<String> {
    let Some(header) = main_header(proof) else {
        return Vec::new();
    };
    let Some((_, ty)) = header.split_once("->") else {
        return Vec::new();
    };
    split_top(ty.trim(), " & ")
}

/// `def main(` through the header colon, which is the first `:` at brace and
/// paren depth 0. The colons inside `{... : Laws.Verdict}` are deeper.
/// Lines whose first non-space is `#` are dropped and newlines become spaces,
/// so a signature split one claim per line reads the same as one line.
fn main_header(proof: &str) -> Option<String> {
    let mut header = String::new();
    let mut depth = 0i32;
    let lines = proof
        .lines()
        .skip_while(|line| !line.starts_with("def main("));
    for line in lines {
        if line.trim_start().starts_with('#') {
            continue;
        }
        for (i, c) in line.char_indices() {
            match c {
                '{' | '(' => depth += 1,
                '}' | ')' => depth -= 1,
                ':' if depth == 0 => {
                    header.push_str(&line[..i]);
                    return Some(header);
                }
                _ => {}
            }
        }
        header.push_str(line);
        header.push(' ');
    }
    None
}

fn split_top(text: &str, sep: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let bytes = text.as_bytes();
    let sep_b = sep.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        if depth == 0 && bytes[i..].starts_with(sep_b) {
            out.push(text[start..i].trim().to_string());
            i += sep_b.len();
            start = i;
            continue;
        }
        i += 1;
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }
    out
}
