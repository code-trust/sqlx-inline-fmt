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
      in
      {
        formatter = pkgs.nixfmt-rfc-style;

        packages.default = pkgs.rustPlatform.buildRustPackage rec {
          pname = "sqlx-inline-fmt";
          version = "0.1.0";
          src = lib.cleanSource ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          meta = with lib; {
            description = "Format inline sqlx query strings in Rust";
            homepage = "https://github.com/code-trust/sqlx-inline-fmt";
            license = licenses.mit;
            mainProgram = "sqlx-inline-fmt";
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
