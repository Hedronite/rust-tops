{
  lib,
  rustPlatform,
}:

rustPlatform.buildRustPackage {
  pname = "cargo-tops";
  version = "0.5.0";

  src = lib.cleanSource ../.;

  cargoLock.lockFile = ../Cargo.lock;

  buildAndTestSubdir = "crates/cargo-tops";

  # cargo-tops' tests run git, clippy and python3, and its integration tests the pinned
  # cargo-nextest and cargo-llvm-cov. CI runs them outside the build sandbox.
  doCheck = false;

  meta = {
    description = "init / check / gate for the Rust-TOPS protocol";
    homepage = "https://github.com/Hedronite/rust-tops";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "cargo-tops";
  };
}
