{
  description = "Development environment for scene-engine";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = {
    self,
    nixpkgs,
  }: let
    systems = [
      "x86_64-linux"
      "aarch64-linux"
    ];

    forAllSystems = nixpkgs.lib.genAttrs systems;
  in {
    devShells = forAllSystems (system: let
      pkgs = import nixpkgs {inherit system;};

      runtimeLibraries = with pkgs; [
        wayland
        wayland-protocols
        libxkbcommon
        libGL
        vulkan-loader
        mesa
      ];
    in {
      default = pkgs.mkShell {
        packages = with pkgs; [
          rustc
          cargo
          rustfmt
          clippy
          rust-analyzer
          pkg-config
          vulkan-tools
          bash
          networkmanager
          iw
          dejavu_fonts
        ];

        buildInputs = runtimeLibraries;

        shellHook = ''
          export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibraries}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
          export RUST_BACKTRACE=1
          export SCENE_ENGINE_FONT_DIR="${pkgs.dejavu_fonts}/share/fonts/truetype"

          rustc --version
          cargo --version
        '';
      };
    });
  };
}
