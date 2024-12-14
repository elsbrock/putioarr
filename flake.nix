{
  description = "Rust dev shell with rust-src and RUST_BACKTRACE";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs";

  outputs = { self, nixpkgs }: {
    devShells = {
      x86_64-linux.default = let
        pkgs = import nixpkgs { system = "x86_64-linux"; };
        rustPlatform = pkgs.rustPlatform; # Access Rust platform tools
      in pkgs.mkShell {
        buildInputs = with pkgs; [
          rustc          # Rust compiler
          cargo          # Rust package manager
          rust-analyzer  # Language server
        ];

        # Add rust-src to the environment
        RUST_SRC_PATH = "${rustPlatform.rustLibSrc}";

        # Set other environment variables
        RUST_BACKTRACE = "1";

        shellHook = ''
          echo "Rust development shell loaded. RUST_BACKTRACE=1 and rust-src are available."
        '';
      };
    };
  };
}
