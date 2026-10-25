# Adopt the rust-tops floor

Floor for a workspace that adopts protocol 1.1.0 with `cargo-tops` 0.2.0. Coverage, CRAP, mutants, and Bend stay optional.

## Floor

These five are mandatory CI jobs:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings` (the deny flag is on the clippy command, not `RUSTFLAGS`)
3. `cargo deny check`
4. `cargo nextest run --workspace --all-features --profile ci`
5. an MSRV job that installs this repo's toolchain and runs `cargo check --workspace --all-features --locked`

## Opt-in layers

Coverage, CRAP, mutants, and Bend laws are advisory. Do not add a line floor, a CRAP threshold, or a mutation kill-rate gate. A job that only checks a report file is nonempty may stay. Do not add `--fail-under-lines`.

Do not enable the `laws` layer. `cargo tops init` turns that layer on and fetches Bend. Do not run `init` at the root of a multi-class workspace. If a crate file already has `laws`, leave `enabled: false`.

## Class map

One `rust-tops.yaml` per crate, protocol 1.1.0, beside that crate. A binary crate is `binary-cli`. A library crate is `library-core`. `library-util` has no profile, so those crates use `library-core`. Do not add a root `workspace-root` file.

`crate.msrv` is that repo's own MSRV. Do not set a product toolchain to 1.99.1. That version is only the compiler that builds `cargo-tops`.

`parser-codec` stays only where the crate is a parser and Bend stays off. Turning Bend on for that class demands a fuzz job.

## cargo-tops pin

`cargo-tops` 0.2.0 is not a dependency inside the product repo. On the build host:

```
cargo install cargo-tops --version 0.2.0 --locked
```

## CI operating systems

When a repo adds an OS, add Arch (Omarchy) and macOS first, then Nix, then Fedora, then Ubuntu. A green Ubuntu job that already exists stays. Do not delete it to match this order.

## MSRV

Each repo pins its own MSRV and tests that pin with the MSRV job. There is no single fleet compiler version.
