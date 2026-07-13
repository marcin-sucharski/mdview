{
  description = "mdview - terminal and web Markdown previewers";

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
          package = pkgs.rustPlatform.buildRustPackage {
            pname = "mdview";
            version = "0.1.12";
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
              pkgs.curl
              pkgs.rustfmt
              pkgs.tmux
            ];

            postCheck = ''
              cargo fmt --check
              cargo clippy --offline -- -D warnings
              unset CARGO_BUILD_TARGET
              cargo build --offline --bins
              MDVIEW_BIN="$PWD/target/debug/mdview" \
                MDVIEW_SKIP_BUILD=1 \
                ${pkgs.bash}/bin/bash scripts/integration-tmux.sh
              MDVIEW_WEB_BIN="$PWD/target/debug/mdview-web" \
                MDVIEW_SKIP_BUILD=1 \
                CURL_BIN="${pkgs.curl}/bin/curl" \
                ${pkgs.bash}/bin/bash scripts/integration-web.sh
            '';

            meta = {
              description = "Terminal and web Markdown previewers with live reload";
              mainProgram = "mdview";
              platforms = systems;
            };
          };
        in
        {
          default = package;
          mdview = package;
          mdview-web = package;
        });

      apps = forAllSystems (system:
        let
          package = self.packages.${system}.default;
          mdview = {
            type = "app";
            program = "${package}/bin/mdview";
            meta.description = "Run the mdview terminal previewer";
          };
          mdview-web = {
            type = "app";
            program = "${package}/bin/mdview-web";
            meta.description = "Run the mdview-web server";
          };
        in
        {
          default = mdview;
          inherit mdview mdview-web;
          web = mdview-web;
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
              pkgs.curl
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
