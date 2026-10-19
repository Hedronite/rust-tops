{
  description = "Rust-TOPS kit overlay: cargo-tops plus a provisional Bend2 laws pack. Protocol 1.0.0 is unchanged.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # nixpkgs 26.11 dropped Intel macOS. Keep x86_64-darwin on the last pin.
    nixpkgs-darwin-x64.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-darwin-x64,
    }:
    let
      inherit (nixpkgs) lib;

      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];

      forAllSystems = f: lib.genAttrs systems (system: f system);

      nixpkgsFor = system: if system == "x86_64-darwin" then nixpkgs-darwin-x64 else nixpkgs;

      pkgsFor =
        system:
        import (nixpkgsFor system) {
          inherit system;
          overlays = [ self.overlays.default ];
        };
    in
    {
      overlays.default = import ./nix/overlay.nix;

      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          inherit (pkgs) cargo-tops rust-tops-laws;
          default = pkgs.cargo-tops;
        }
      );

      apps = forAllSystems (system: {
        cargo-tops = {
          type = "app";
          program = lib.getExe self.packages.${system}.cargo-tops;
        };
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.cargo-tops
              pkgs.rustc
              pkgs.cargo
              pkgs.clippy
              pkgs.rustfmt
            ];
          };
        }
      );

      checks = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          cargo-tops = self.packages.${system}.cargo-tops;
          rust-tops-laws = self.packages.${system}.rust-tops-laws;

          # Fast local check. Not a proof. CI never treats this name as one.
          law-catalog-names = pkgs.runCommand "rust-tops-law-catalog-names" { } ''
            set -eu
            export BEND_GATE_NAMES_ONLY=1
            export RUST_TOPS_ROOT=${./.}
            bash ${./scripts/bend-gate.sh}
            echo ok >"$out"
          '';
        }
        // lib.optionalAttrs (system == "x86_64-linux" || system == "aarch64-linux") (
          let
            bendPin = {
              "x86_64-linux" = {
                plat = "linux-x64";
                hash = "sha256-AgidwO7Q/V/W1zx0zJz/yyxmON07Aq6D97jMV/vjgbo=";
              };
              "aarch64-linux" = {
                plat = "linux-arm64";
                hash = "sha256-oUoxHjm61F+WadTqs50Va1eMlF8Tore64g2cI1Z6RmQ=";
              };
            };
            pin = bendPin.${system};
            bendSrc = pkgs.fetchurl {
              url = "https://github.com/bendlang/bend/releases/download/v2.0.36/bend-2.0.36-${pin.plat}.tar.gz";
              hash = pin.hash;
            };
          in
          {
            # Real proof. Darwin has no bend-proof attr: the Bun binary was not
            # verified inside the Nix darwin sandbox, and a names-only pass must
            # not wear this name. No GitHub job runs `nix flake check`.
            bend-proof = pkgs.runCommand "rust-tops-bend-proof" { nativeBuildInputs = [ pkgs.patchelf ]; } ''
              set -eu
              tar -xzf ${bendSrc}
              export HOME=$TMPDIR
              mkdir -p "$HOME/.bend"
              cp -a bend/bin bend/bend2 bend/guide "$HOME/.bend/"
              chmod +x "$HOME/.bend/bin/bend"
              patchelf \
                --set-interpreter ${pkgs.stdenv.cc.bintools.dynamicLinker} \
                --set-rpath ${pkgs.glibc}/lib \
                "$HOME/.bend/bin/bend"
              export PATH="$HOME/.bend/bin:$PATH"
              export BEND_NO_TELEMETRY=1
              export RUST_TOPS_ROOT=${./.}
              bash ${./scripts/bend-gate.sh}
              echo ok >"$out"
            '';
          }
        )
      );
    };
}
