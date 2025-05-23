{
  description = "A basic flake";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";

  outputs = { self, nixpkgs, flake-utils }:
    with flake-utils.lib;
    eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        runtimeLibs = with pkgs; [
          alsa-lib
          udev
          vulkan-loader
          gcc-unwrapped
          xorg.libX11
          xorg.libXcursor
          xorg.libXrandr
          xorg.libXpresent
          xorg.libXi
          xorg.libxcb
          libGL
          libxkbcommon
          freetype
          harfbuzz
          fontconfig
        ];
      in
      with pkgs.lib; {
        packages.default = pkgs.callPackage
          (import ./nix/default.nix self.shortRev or self.dirtyShortRev)
          { };
        devShell = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            bashInteractive
            rust-bindgen
            pkg-config
            xdotool # useful for testing
            web-ext
          ] ++ runtimeLibs;
          buildInputs = runtimeLibs;
          shellHook = ''
            export FREETYPE_PATH=${pkgs.freetype.dev}
            export HARFBUZZ_PATH=${pkgs.harfbuzz.dev}
            export LD_LIBRARY_PATH="$LD_LIBRARY_PATH:${pkgs.lib.makeLibraryPath runtimeLibs}"'';
        };
      });
}
