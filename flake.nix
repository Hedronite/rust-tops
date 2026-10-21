{
  description = "Rust-TOPS kit overlay: cargo-tops, the Bend2 laws pack, and (Linux) the pinned Bend that judges the protocol 1.1.0 laws layer.";

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
        // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          inherit (pkgs) rust-tops-bend;
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
            ]
            ++ lib.optional pkgs.stdenv.hostPlatform.isLinux pkgs.rust-tops-bend;
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
        // lib.optionalAttrs (system == "x86_64-linux" || system == "aarch64-linux") {
          # Real proof. Darwin has no bend-proof attr: the Bun binary was not
          # verified inside the Nix darwin sandbox, and a names-only pass must
          # not wear this name. No GitHub job runs `nix flake check`.
          # nix/bend.nix holds the pin (laws/bend.lock sha256, patchelf).
          bend-proof = pkgs.runCommand "rust-tops-bend-proof" { } ''
            set -eu
            export HOME=$TMPDIR
            mkdir -p "$HOME/.bend"
            cp -a ${pkgs.rust-tops-bend}/bin ${pkgs.rust-tops-bend}/bend2 ${pkgs.rust-tops-bend}/guide "$HOME/.bend/"
            chmod -R u+w "$HOME/.bend"
            export PATH="$HOME/.bend/bin:$PATH"
            export BEND_NO_TELEMETRY=1
            export RUST_TOPS_ROOT=${./.}
            bash ${./scripts/bend-gate.sh}
            echo ok >"$out"
          '';
        }
      );
    };
}
