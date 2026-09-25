{
  lib,
  stdenvNoCC,
}:

stdenvNoCC.mkDerivation {
  pname = "rust-tops-laws";
  version = "0.1.3";

  src = ../laws;

  dontConfigure = true;
  dontBuild = true;

  installPhase = ''
    runHook preInstall
    mkdir -p $out/share/rust-tops/laws
    cp -R . $out/share/rust-tops/laws/
    runHook postInstall
  '';

  meta = {
    description = "Provisional rust-tops Bend2 laws pack (H-01..H-53). Not protocol 1.0.";
    homepage = "https://github.com/Hedronite/rust-tops";
    license = with lib.licenses; [
      mit
      asl20
    ];
  };
}
