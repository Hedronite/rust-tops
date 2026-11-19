---
name: rust-tops-gate
description: Run cargo tops gate and report each layer and law. Use when checking or fixing a Rust-TOPS gate. not-evaluated is not a pass. Requires Nushell. Run the script.
---

# rust-tops-gate

Nushell (`nu`) is required. Run this script. Do not read it to grade the gate, and do not trust a `report.json` left from an earlier run; the script deletes it first.

```bash
nu rust-tops-gate.nu [--path .] [--max-tier 1]
```

It prints one JSON object. `command` is the exact `cargo tops gate` argv. Do not read the script; it is long. `ok` is true only when that command exits 0 and every law is `checked` with verdict `Allow` or `Ack`. The script reads only `rust-tops/report.json` under the Cargo target directory, and never the report's top-level `ok`.

`data.layers` lists each layer's `id`, `tier`, `verdict` (`pass`, `fail`, `not-run`, `vacuous`, `tool-missing` or `not-evaluated`) and `seconds`, with the argv it ran and a remedy for a failing layer. `data.gate_green` is true when the report's `exit_code` is 0 and every checked law passed. A law with `reason_code` `no_fact_reader` is a `data.gaps` item, not a fix. A law with `reason_code` `run_dependent` stays in `data.fixes`. Either list keeps `ok` false.

Exit code 1 means the gate ran and failed; read `data.layers` and `data.fixes`. Exit code 2 (the config is invalid) and exit code 3 (the environment lacks a required tool or `cargo metadata`) are `error`, with the code's meaning and the stderr tail (the stdout tail when stderr is empty). A missing `cargo tops` is `error` too. A zero exit with a missing or non-JSON report is `ok` false for that report.

If `nu` is not on PATH, there is no JSON. Treat `nu: command not found` as `{"ok":false,"command":"nu rust-tops-gate.nu","error":"nu: command not found"}`.
