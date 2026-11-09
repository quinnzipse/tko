{
  description = "A ticket management CLI backed by git-service issue trackers";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      cargoToml = lib.importTOML ./Cargo.toml;
    in
    {
      packages = forAllSystems (pkgs: {
        tko = pkgs.rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;

          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./src
              ./tests
            ];
          };

          cargoLock.lockFile = ./Cargo.lock;

          meta = {
            inherit (cargoToml.package) description;
            homepage = "https://github.com/wyomatthew/tko";
            license = lib.licenses.mit;
            mainProgram = "tko";
          };
        };
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.tko;
      });

      overlays.default = final: _prev: {
        tko = self.packages.${final.stdenv.hostPlatform.system}.tko;
      };

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.tko ];
          packages = with pkgs; [
            clippy
            rustfmt
            rust-analyzer
          ];
        };
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
