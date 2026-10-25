---
name: rust-tops-gate
description: Run cargo tops gate and report each layer and law. Use when checking or fixing a Rust-TOPS gate. not-evaluated is not a pass. Requires Nushell. Run the script.
---

# rust-tops-gate

Nushell (`nu`) is required. Run this script. Do not read it to grade the gate, and do not trust a `cases.json` left from an earlier run.

```bash
nu rust-tops-gate.nu [--path .] [--max-tier 1]
```

It prints one JSON object. `command` is the exact `cargo tops gate` argv. Do not read the script; it is long. `ok` is true only when that command exits 0 and every law is `checked` with verdict `Allow` or `Ack`. The script does not read the report's top-level pass bit.

`data.gate_green` is true when the command exits 0 and every checked law passed. A law with `reason_code` `no_fact_reader` is a `data.gaps` item, not a fix. A law with `reason_code` `run_dependent` stays in `data.fixes`. Either list keeps `ok` false.

If the command exits non-zero, or `cargo tops` is missing, `error` is that exit code and the stderr tail (the stdout tail when stderr is empty). That is reported before `cases.json` is read. A zero exit with a missing or non-JSON report is `ok` false for that report.

If `nu` is not on PATH, there is no JSON. Treat `nu: command not found` as `{"ok":false,"command":"nu rust-tops-gate.nu","error":"nu: command not found"}`.
