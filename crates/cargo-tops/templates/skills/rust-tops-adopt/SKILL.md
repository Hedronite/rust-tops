---
name: rust-tops-adopt
description: Adopt Rust-TOPS on a crate. Use when choosing a class or running cargo tops init and cargo tops check. Requires Nushell. Run the script; do not re-implement init.
---

# rust-tops-adopt

Nushell (`nu`) is required. Run this script. Do not read it and do not re-implement `cargo tops`.

```bash
nu rust-tops-adopt.nu adopt --class <class> [--path .] [--name <crate>] [--force]
```

`<class>` is `library-core`, `parser-codec`, `no-std-embedded`, `unsafe-kernel`, or `binary-cli`.

The script runs `cargo tops init --skills`, then `cargo tops check`. It prints one JSON object. `command` is the exact argv. `ok` is true only when both commands exit 0.

If `nu` is not on PATH, the shell reports `nu: command not found` and there is no JSON. Treat that as `{"ok":false,"command":"nu rust-tops-adopt.nu","error":"nu: command not found"}`. Do not write the kit files yourself.
