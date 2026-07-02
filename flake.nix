{
  description = "mdview - terminal Markdown previewer";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in
    {
      packages.${system}.default = pkgs.rustPlatform.buildRustPackage {
        pname = "mdview";
        version = "0.1.0";
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

        meta = {
          description = "A minimal terminal Markdown previewer for Linux";
          mainProgram = "mdview";
          platforms = pkgs.lib.platforms.linux;
        };
      };

      apps.${system}.default = {
        type = "app";
        program = "${self.packages.${system}.default}/bin/mdview";
        meta.description = "Run mdview";
      };

      devShells.${system}.default = pkgs.mkShell {
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

      checks.${system}.default = self.packages.${system}.default;
    };
}
