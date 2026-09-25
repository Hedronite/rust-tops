# rust-tops overlay: cargo-tops + the provisional Bend2 laws pack.
# Protocol plane stays 1.0.0. This overlay does not amend RUST_TOPS.md.
final: _prev: {
  cargo-tops = final.callPackage ./cargo-tops.nix { };
  rust-tops-laws = final.callPackage ./rust-tops-laws.nix { };
}
