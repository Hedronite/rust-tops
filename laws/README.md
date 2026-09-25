# rust-tops Bend2 pack (provisional)

**Not protocol 1.0.** Does not amend [`../RUST_TOPS.md`](../RUST_TOPS.md) or the schema.

Harvest H-01 through H-53 encoded as fail-closed Bend laws. Hard rows map the forbidden case to `Refuse`. Soft rows map it to `Ack` (not `Allow`). Agents under an AI-ops seat treat `Refuse` as stop. Humans read the pack as advisory.

| File | What |
| --- | --- |
| [`LAWS.bend`](LAWS.bend) | Types, verdict functions, 53 `law` statements + proofs |
| [`PROOF.bend`](PROOF.bend) | Names every law. `bend PROOF.bend` exits 0 |
| [`NAMES`](NAMES) | Harvest law names, one per line, H-01..H-53 order |

```bash
./scripts/bend-gate.sh
```

Nix overlay: `overlays.default` exposes `cargo-tops` and `rust-tops-laws` (`$out/share/rust-tops/laws`). Adopting flakes:

```nix
{
  inputs.rust-tops.url = "github:Hedronite/rust-tops";
  outputs = { nixpkgs, rust-tops, ... }:
    let
      pkgs = import nixpkgs {
        system = "aarch64-darwin";
        overlays = [ rust-tops.overlays.default ];
      };
    in {
      packages.aarch64-darwin.default = pkgs.cargo-tops;
    };
}
```

Regenerate from the catalog: `cargo +nightly -Zscript scripts/gen-laws.rs` (native Cargo script, RFC 3502, std-only; stable `cargo script` is not on 1.96/1.98). Do not weaken a law to make a proof pass.
