---
name: rust-tops-style
description: Check Rust style with clippy and Nushell probes, then decide only the judgment rules. Use before a review or after a large edit. Requires Nushell. Run the script.
---

# rust-tops-style

Nushell (`nu`) is required: the version `rust-tops.lock` names under `[tools] nushell` (default 0.108.0), or a newer one of the same major. Run the script. Do not re-derive its findings by reading code.

The script runs `cargo clippy`, which builds the crate. Run it on the machine where this project's cargo builds belong, not on one that only edits or controls.

```bash
nu rust-tops-style.nu check --base <ref> [--path .] [--committed-only]   # writer
nu rust-tops-style.nu check [--audit] [--path .]                         # audit
```

`--path` is a crate or workspace directory. Findings outside it are dropped.

**Writer mode** (`--base <ref>`) is for a draft:

- It compares the working tree with `<ref>` itself: staged, unstaged and untracked `.rs` lines count. Use `--base HEAD` for uncommitted work. `--committed-only` takes `<ref>...HEAD` instead and needs a clean tree: with any tracked change it refuses, `ok: false`. Untracked files are allowed and not checked.
- Only findings on changed lines are kept, and `ok` speaks for those alone.
- Clippy restriction lints (`unwrap_used`, `indexing_slicing` and the like) run only if the crate turns them on: its `[lints]` tables, its source, or `gates.lint.clippy_deny` in `rust-tops.yaml`.
- `PANIC-1` and `PANIC-2` are reported for library and binary code only, not for `tests/`, `*_test.rs` or `#[cfg(test)]` code.
- `data.judgment` holds the rules the changed non-test lines give a reason to ask. A tests-only change gets none.

**Audit mode** (no `--base`, or `--audit`, which ignores `--base`) checks the whole tree with every catalog lint and every judgment rule.

MIT. Rules distilled from BeautifulRust v1.5.0 (40db61c6), rewritten. Copyright (c) 2026 Morgan Linton. See LICENSES/BeautifulRust-MIT.txt.

## Output

One JSON object: `ok`, `command` (the exact argv) and either `data` or `error`.

- `command`: the exact argv of each external command run, in order, joined by ` && `, with paths relative to `--path`; a command that runs none gives the script's own invocation.
- `data.findings`: `{rule, kind, file, line, col, message, fix}`, sorted by file, line, column, rule. `rule` is an ID in `rules.toml`, beside this file. `fix` is the catalog's fix for that rule.
- `data.judgment`: the rules no tool can decide, as `{id, rule}`.
- `ok` is true only when the script ran and found nothing. A failed build is `ok: false` with an `error`. An older or other-major Nushell is `ok: false` with `nu <lock> required, found <version>`.
- Same tree, same bytes. The script prints no verdict.

## What to do

1. Run `check`. Fix each finding the way its `fix` says, then run it again.
2. Fix at the cause. Do not add `#[allow]` or `#[expect]` to quiet a finding unless the reason names the rule.
3. Answer each `data.judgment` rule yourself, against the code you changed. These are all the IDs:

- `API-4`: A function must not take ownership and then clone, or borrow and then clone to get ownership.
- `API-5`: impl Into parameters earn their cost only in library entry points that are called often.
- `API-6`: A library function returns an iterator when it would otherwise build a Vec just to hand it over.
- `API-7`: Public fields, Rc, RefCell, Arc and third-party types do not leak through a public signature.
- `API-16`: A trait that outsiders should only use, never implement, is sealed.
- `API-17`: A library follows the API rules strictly; an application keeps its code plain.
- `CONC-3`: Callers' closures are not run while a lock is held, and a guard is not returned.
- `DOC-5`: A doc comment opens with a one-line summary that adds information beyond the name.
- `ERR-5`: Every public error type implements Display and Error, and is Send, Sync and 'static.
- `ERR-6`: Error text names the file, line or value that failed, and the underlying cause stays reachable.
- `IDIOM-10`: Pick match or a combinator by which reads better; use a loop when a chain gets long.
- `OWN-2`: No clone exists only to quiet the borrow checker.
- `PANIC-4`: Arithmetic on outside values must not overflow into a panic or a wrong answer.
- `PANIC-5`: Each expect, assert and panic message carries the values that made it fire.
- `PANIC-6`: An expected failure is returned as an error, never raised as a panic or caught with catch_unwind.
- `PANIC-7`: A Drop impl never panics, blocks or has a failure path of its own.
- `TEST-1`: Logic that decides something has a test that would fail if the logic broke.
- `TEST-2`: A test checks behavior, not a copy of the constants it was built from.
- `TRAIT-3`: A hand-written Ord agrees with the PartialEq and Hash on the same type.
- `TRAIT-4`: Public types derive the common traits that fit them: Clone, Eq, Hash, Default and so on.
- `UNSAFE-5`: A safe function must not allow undefined behavior through any sequence of safe calls.

Do not edit `rules.toml`. If a rule is wrong, tell the maintainer.

If `nu` is not on PATH, there is no JSON. Treat `nu: command not found` as `{"ok":false,"command":"nu rust-tops-style.nu","error":"nu: command not found"}`.
