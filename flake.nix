{
  description = "mdview - terminal Markdown previewer";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  };

  outputs = { self, nixpkgs }:
    let
      lib = nixpkgs.lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = lib.genAttrs systems;
      pkgsFor = system: import nixpkgs { inherit system; };
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "mdview";
            version = "0.1.11";
            src = pkgs.lib.cleanSourceWith {
              src = ./.;
              filter = path: type:
                let
                  name = baseNameOf path;
                in
                !(type == "directory" && (name == "target" || name == ".direnv"))
                && !(type == "symlink" && name == "result");
            };
            cargoLock.lockFile = ./Cargo.lock;
            nativeCheckInputs = [
              pkgs.clippy
              pkgs.rustfmt
              pkgs.tmux
            ];

            postCheck = ''
              cargo fmt --check
              cargo clippy --offline -- -D warnings
              unset CARGO_BUILD_TARGET
              cargo build --offline
              MDVIEW_BIN="$PWD/target/debug/mdview" \
                MDVIEW_SKIP_BUILD=1 \
                ${pkgs.bash}/bin/bash scripts/integration-tmux.sh
            '';

            meta = {
              description = "A minimal terminal Markdown previewer for Linux";
              mainProgram = "mdview";
              platforms = systems;
            };
          };
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/mdview";
          meta.description = "Run mdview";
        };
      });

      devShells = forAllSystems (system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.clippy
              pkgs.rust-analyzer
              pkgs.rustc
              pkgs.rustfmt
              pkgs.tmux
            ];

            RUST_BACKTRACE = "1";
          };
        });

      checks = forAllSystems (system: {
        default = self.packages.${system}.default;
      });
    };
}
