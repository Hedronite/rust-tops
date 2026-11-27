---
name: rust-tops-review
description: Review a diff with the Rust-TOPS review gate in docs/REVIEW.md, then run the style pass on the changed lines and answer the judgment rules. Use for a fresh review, including each fix commit. Requires Nushell. Run the script.
---

# rust-tops-review

Nushell (`nu`) is required. The reviewer did not write the diff. Run the script. Do not read it instead of reviewing.

```bash
nu rust-tops-review.nu files [--base origin/main] [--path .] [--committed-only]
nu rust-tops-review.nu diff [--base origin/main] [--path .] [--committed-only]
nu rust-tops-review.nu style [--base origin/main] [--path .] [--committed-only]
nu rust-tops-review.nu answers <file>
```

Run `files` first and write the inputs you will execute. Then run `diff` and follow `docs/REVIEW.md`. The script does not emit a verdict.

MIT. The style rules are distilled from BeautifulRust v1.5.0 (40db61c6), rewritten. Copyright (c) 2026 Morgan Linton. See LICENSES/BeautifulRust-MIT.txt.

## Style pass

1. Run `style`. It runs the style skill (`../rust-tops-style`) and keeps the findings on lines changed in the range. Fix each one, commit, and run it again.
2. `data.judgment` lists the rules no tool decides, as `{id, rule}`. Answer every one against the diff.
3. Write the answers as TOML, then run `answers <file>`:

```toml
[answers]
"PANIC-4" = { status = "ok", note = "" }
"PANIC-5" = { status = "finding", note = "src/lib.rs:40 expect names no value" }
```

`status` is `ok`, `finding` or `n/a`. A `finding` needs a note. `answers` gives `ok: false` for a missing or unknown ID. Report each `finding` as a review finding.

By default the range is `base...HEAD` plus the working tree: tracked edits and untracked files. `--committed-only` omits the working tree.

Each command prints JSON. `command` is the exact argv of each external command run, in order, joined by ` && `, with paths relative to `--path`; a command that runs none gives the script's own invocation. `ok` is false when a command fails or, for `style` and `answers`, finds a problem. `style` runs `cargo clippy`, so run it where this project's cargo builds belong, not on a machine that only edits or controls. An older or other-major Nushell than the lock's is `ok: false` with `nu <lock> required, found <version>`.

If `nu` is not on PATH, there is no JSON. Treat `nu: command not found` as `{"ok":false,"command":"nu rust-tops-review.nu","error":"nu: command not found"}`.
