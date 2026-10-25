---
name: rust-tops-review
description: Review a diff with the Rust-TOPS review gate in docs/REVIEW.md. Use for a fresh review, including each fix commit. Write inputs before reading the diff. Requires Nushell. Run the script.
---

# rust-tops-review

Nushell (`nu`) is required. The reviewer did not write the diff. Run the script. Do not read it instead of reviewing.

```bash
nu rust-tops-review.nu files [--base origin/main] [--path .] [--committed-only]
nu rust-tops-review.nu diff [--base origin/main] [--path .] [--committed-only]
```

Run `files` first and write the inputs you will execute. Then run `diff` and follow `docs/REVIEW.md`. The script does not emit a verdict.

By default the range is `base...HEAD` plus the working tree. That is `git diff HEAD` for staged and unstaged changes to tracked files. Untracked paths are listed in `data.untracked` and are not part of the patch. `--committed-only` omits the working tree.

Each command prints JSON. `command` is the exact `git` argv. `ok` is false when `git` fails.

If `nu` is not on PATH, there is no JSON. Treat `nu: command not found` as `{"ok":false,"command":"nu rust-tops-review.nu","error":"nu: command not found"}`.
