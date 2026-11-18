---
name: rust-tops-port
description: Port code from C, C++, Python, Go, JavaScript or TypeScript to Rust with a checked test map. Lists the original tests, then verifies each has a passing Rust counterpart. Requires Nushell. Run the script.
---

# rust-tops-port

Nushell (`nu`) is required. Run the script; do not re-derive its lists by hand. Each command prints one JSON object with `ok`, `command` and `data` or `error`. The script never prints a verdict.

```bash
nu rust-tops-port.nu inventory --from <dir> --lang <c|cpp|python|go|js|ts> [--write port-map.toml]
nu rust-tops-port.nu verify --map port-map.toml [--path .]
```

`inventory` finds original tests with fixed patterns per language and sorts them. `--write` writes a map skeleton with one entry per test, and refuses to overwrite a file. Keep the map beside the Rust crate and give `from` as a path relative to the map.

`verify` runs in the crate at `--path`. It fails when an original test has no entry or an empty `rust` name, when a mapped name is missing from `cargo test -- --list`, or when `cargo test` fails. Each finding carries a catalog `rule` and `fix`. `ok` is true only when all of that passes.

## Procedure

1. Understand the original (PORT-1, PORT-2, PORT-3).
2. Run `inventory --write`, then port every test first, one Rust test per original, named so it points back (PORT-4).
3. Design the Rust API and implement it (PORT-5, PORT-6, PORT-7, PORT-8, PORT-9, PORT-10).
4. Run `verify` until `ok` is true, check the size (PORT-11), then build, test and lint the port (PORT-13).
5. Write `PORTING.md` (PORT-12).

## Judgment rules

Rules are in `../rust-tops-style/rules.toml`, and `data.judgment` lists these. You decide them; the script cannot.

- PORT-1: read the original, its tests and callers; list every observable behavior.
- PORT-2: drop what only exists because of the source language.
- PORT-3: keep an original bug and report it.
- PORT-5, PORT-6, PORT-7: map C and C++, Python, and Go or JS idioms to Rust forms.
- PORT-8: use the standard library, not a rebuilt runtime.
- PORT-9: expose the original's surface, nothing more.
- PORT-10: keep the algorithm unless it is wrong.
- PORT-11: compare code size with the original.
- PORT-13: build, test and lint clean; re-run the original tests on the same inputs.

Distilled from BeautifulRust v1.5.0 (40db61c6), rewritten. MIT, Copyright (c) 2026 Morgan Linton. See `LICENSES/BeautifulRust-MIT.txt`.

If `nu` is not on PATH, there is no JSON. Treat `nu: command not found` as `{"ok":false,"command":"nu rust-tops-port.nu","error":"nu: command not found"}`.
