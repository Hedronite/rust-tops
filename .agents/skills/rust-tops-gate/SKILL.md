---
name: rust-tops-gate
description: Run cargo tops gate and report each layer and law. Use when checking or fixing a Rust-TOPS gate. not-evaluated is not a pass. Requires Nushell. Run the script.
---

# rust-tops-gate

Nushell (`nu`) is required. Run this script. Do not read it to grade the gate, and do not trust a `cases.json` left from an earlier run.

```bash
nu rust-tops-gate.nu [--path .] [--max-tier 1]
```

It prints one JSON object. `command` is the exact `cargo tops gate` argv. `ok` is true only when that command exits 0 and every law row is `checked` with verdict `Allow` or `Ack`. Each `not-evaluated` law is a `data.fixes` item and forces `ok` false. The script does not read the report's top-level pass bit.

`ok` is false when the command exits non-zero, `rust-tops/cases.json` under the Cargo target directory is missing or not JSON, or `cargo` / `cargo tops` / `bend` is missing. Read `error`.

If `nu` is not on PATH, there is no JSON. Treat `nu: command not found` as `{"ok":false,"command":"nu rust-tops-gate.nu","error":"nu: command not found"}`.
