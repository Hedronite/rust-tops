{
  lib,
  stdenvNoCC,
}:

stdenvNoCC.mkDerivation {
  pname = "rust-tops-laws";
  version = "0.2.0";

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
    description = "rust-tops Bend2 laws pack (H-01..H-53), judged by the laws layer of protocol 1.1.0";
    homepage = "https://github.com/Hedronite/rust-tops";
    license = with lib.licenses; [
      mit
      asl20
    ];
  };
}
