{
  description = "A very basic flake";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    fenix = {
        url = "github:nix-community/fenix";
        inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs = { self, nixpkgs, fenix, flake-parts, ... }@inputs:
  flake-parts.lib.mkFlake { inherit inputs ; } {
    systems = [ "x86_64-linux" ];

    perSystem = { config, self', inputs', pkgs, system, ... }:
        let
            pkgs = import inputs.nixpkgs {
                inherit system;
                config.allowUnfree = true;
            };
            fenixLib = fenix.packages.${system};
            rustToolchain = fenixLib.stable.withComponents [
                "cargo"
                "clippy"
                "rust-src"
                "rustc"
                "rustfmt"
            ];
        in
        {
            devShells.default = pkgs.mkShell {
                nativeBuildInputs = with pkgs; [
                    rustToolchain
                    rust-analyzer
                    pkg-config
                    jetbrains.rust-rover
                    gcc
                    direnv
                    nix-direnv
                ];

                buildInputs = with pkgs; [
                    openssl
                    glib
                ];

                shellHook = ''
                    # stable toolchain path for IDE's
                    mkdir -p ~/.rust-rover/toolchain
                    ln -sfn ${rustToolchain}/lib ~/.rust-rover/toolchain
                    ln -sfn ${rustToolchain}/bin ~/.rust-rover/toolchain

                    # Needed by some tools (e.g., rust-analyzer) to find the standard library sources
                    export RUST_SRC_PATH="$HOME/.rust-rover/toolchain/lib/rustlib/src/rust/library"

                    # Print nice message
                    BOLD='\033[1m'
                    CYAN='\033[0;36m'
                    RESET='\033[0m'
                    echo ""
                    echo -e "''${BOLD}''${CYAN}🦀 Rust Dev Shell (Default) Activated''${RESET}"
                    echo ""
                '';
            };
        };
  };
}
