---
name: rust-tops-style
description: Check Rust style with clippy and Nushell probes, then decide only the judgment rules. Use before a review or after a large edit. Requires Nushell. Run the script.
---

# rust-tops-style

Nushell (`nu`) is required, at the minor `rust-tops.lock` names under `[tools] nushell` (default 0.108.0). Run the script. Do not re-derive its findings by reading code.

```bash
nu rust-tops-style.nu check [--path .] [--base <ref>]
```

`--path` is a crate or workspace directory. Findings outside it are dropped. `--base <ref>` keeps only findings on lines changed since `<ref>`.

MIT. Rules distilled from BeautifulRust v1.5.0 (40db61c6), rewritten. Copyright (c) 2026 Morgan Linton. See LICENSES/BeautifulRust-MIT.txt.

## Output

One JSON object: `ok`, `command` (the exact argv) and either `data` or `error`.

- `data.findings`: `{rule, kind, file, line, col, message, fix}`, sorted by file, line, column, rule. `rule` is an ID in `rules.toml`, beside this file. `fix` is the catalog's fix for that rule.
- `data.judgment`: the rules no tool can decide, as `{id, rule}`.
- `ok` is true only when the script ran and found nothing. A failed build is `ok: false` with an `error`. A wrong Nushell minor is `ok: false` with `nu <lock> required, found <version>`.
- Same tree, same bytes. The script prints no verdict.

## What to do

1. Run `check`. Fix each finding the way its `fix` says, then run it again.
2. Fix at the cause. Do not add `#[allow]` or `#[expect]` to quiet a finding unless the reason names the rule.
3. Answer each `data.judgment` rule yourself, against the code you changed. These are the IDs:

- `PANIC-5`: Each expect, assert and panic message carries the values that made it fire.
- `OWN-2`: No clone exists only to quiet the borrow checker.

Do not edit `rules.toml`. If a rule is wrong, tell the maintainer.

If `nu` is not on PATH, there is no JSON. Treat `nu: command not found` as `{"ok":false,"command":"nu rust-tops-style.nu","error":"nu: command not found"}`.
