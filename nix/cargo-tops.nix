{
  lib,
  rustPlatform,
}:

rustPlatform.buildRustPackage {
  pname = "cargo-tops";
  version = "0.1.2";

  src = lib.cleanSource ../.;

  cargoLock.lockFile = ../Cargo.lock;

  buildAndTestSubdir = "crates/cargo-tops";

  doCheck = true;

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
