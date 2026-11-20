{
  description = "Export PostgreSQL data";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forEachSystem = nixpkgs.lib.genAttrs systems;
      packageFor = system:
        let pkgs = import nixpkgs { inherit system; };
        in pkgs.callPackage ./release/nix/package.nix { };
    in {
      packages = forEachSystem (system: {
        pgex = packageFor system;
        default = packageFor system;
      });
      apps = forEachSystem (system: {
        default = {
          type = "app";
          program = "${packageFor system}/bin/pgex";
          inherit (packageFor system) meta;
        };
      });
      checks = forEachSystem (system: { pgex = packageFor system; });
    };
}
