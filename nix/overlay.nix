# rust-tops overlay: cargo-tops, the Bend laws pack, and on Linux the pinned
# Bend that judges the `laws` layer (protocol 1.1.0, RUST_TOPS.md §3.1).
final: prev:
{
  cargo-tops = final.callPackage ./cargo-tops.nix { };
  rust-tops-laws = final.callPackage ./rust-tops-laws.nix { };
}
# Keyed on `prev` so the attribute names do not depend on the fixpoint.
// prev.lib.optionalAttrs prev.stdenv.hostPlatform.isLinux {
  rust-tops-bend = final.callPackage ./bend.nix { };
}
