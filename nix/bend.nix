# Bend 2.0.36 as pinned in laws/bend.lock: the judge of the `laws` layer
# (protocol 1.1.0, RUST_TOPS.md §3.1). Linux only, the systems where this
# flake already builds bend for `checks.bend-proof`. Darwin adopters run
# scripts/fetch-bend.sh. The hashes are the lock's sha256 values in SRI form.
{
  lib,
  stdenv,
  fetchurl,
  patchelf,
}:

let
  version = "2.0.36";
  pins = {
    "x86_64-linux" = {
      plat = "linux-x64";
      hash = "sha256-AgidwO7Q/V/W1zx0zJz/yyxmON07Aq6D97jMV/vjgbo=";
    };
    "aarch64-linux" = {
      plat = "linux-arm64";
      hash = "sha256-oUoxHjm61F+WadTqs50Va1eMlF8Tore64g2cI1Z6RmQ=";
    };
  };
  system = stdenv.hostPlatform.system;
  pin = pins.${system} or (throw "rust-tops-bend: laws/bend.lock has no Nix pin for ${system}");
in
stdenv.mkDerivation {
  pname = "rust-tops-bend";
  inherit version;

  src = fetchurl {
    url = "https://github.com/bendlang/bend/releases/download/v${version}/bend-${version}-${pin.plat}.tar.gz";
    inherit (pin) hash;
  };

  nativeBuildInputs = [ patchelf ];

  dontConfigure = true;
  dontBuild = true;
  # A Bun single-file executable. Stripping it, or any rpath edit, breaks it:
  # `patchelf --set-rpath` makes Bun 1.3.14 segfault on aarch64-linux
  # (measured 2026-10-21). Only the interpreter is rewritten; Nix's ld.so
  # finds libc and friends in its own glibc.
  dontStrip = true;
  dontPatchELF = true;

  installPhase = ''
    runHook preInstall
    mkdir -p $out
    cp -a bin bend2 guide $out/
    chmod +x $out/bin/bend
    patchelf --set-interpreter ${stdenv.cc.bintools.dynamicLinker} $out/bin/bend
    runHook postInstall
  '';

  meta = {
    description = "Bend ${version}, TOFU-pinned by rust-tops laws/bend.lock (judge of the laws layer)";
    homepage = "https://github.com/bendlang/bend";
    platforms = builtins.attrNames pins;
    sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
    mainProgram = "bend";
  };
}
