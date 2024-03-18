{
  description = "AMD Experiment Manager Dev Environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs"; # also valid: "nixpkgs"
    
    flake-utils = {
      url = "github:numtide/flake-utils";
    };
  };

  outputs = { self, nixpkgs, flake-utils,... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
          config.cudaSupport = true;
        };
        
        # [HELP] Other variable definitions can go here
        
      in
        {
          # Development environment output
          devShell = pkgs.mkShell {
            # The Nix packages provided in the environment
            packages = with pkgs; [
              bash        # The standard shell
              gnumake     # Make
              mold        # Faster linker
              jq          # JSON processor
              openssl     # Dependency for AWS SDK
              pkg-config
              shellcheck

              # CUDA packages
              # cudaPackages.cudatoolkit
              # cudaPackages.cuda_cudart
              # cudaPackages.cudnn
              # cudaPackages.cuda_nsight

              # Rust
              cargo
              rustc
              rust-analyzer
              clippy
              rustfmt

              # AWS
              awscli2
            ];

            shellHook = ''
              # export LD_LIBRARY_PATH="${pkgs.cudatoolkit.lib}/lib:${pkgs.cudatoolkit}/lib:$LD_LIBRARY_PATH"
              # export CUDA_HOME="${pkgs.cudatoolkit}"
              # export CUDA_PATH="${pkgs.cudatoolkit}"
              # export NVCC_GENCODE="-gencode=arch=compute_86,code=sm_86"
              export RUST_SRC_PATH="${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
              export MOLD_HOME="${pkgs.mold}"
              export JQ_LIB_DIR="${pkgs.jq.lib}/lib"
            '';
          };
        }
    );
}
