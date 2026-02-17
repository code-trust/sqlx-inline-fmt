{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        lib = pkgs.lib;
        cargo = lib.importTOML ./Cargo.toml;
        pkg = cargo.package;
      in
      {
        formatter = pkgs.nixfmt-rfc-style;

        packages.default = pkgs.rustPlatform.buildRustPackage rec {
          pname = pkg.name;
          version = pkg.version;
          src = lib.cleanSource ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          meta = with lib; {
            description = pkg.description;
            homepage = pkg.repository;
            license = licenses.mit;
            mainProgram = pname;
            platforms = platforms.unix;
          };
        };

        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/sqlx-inline-fmt";
        };
        apps."sqlx-inline-fmt" = self.apps.${system}.default;
      }
    );
}
