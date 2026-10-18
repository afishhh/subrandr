{
  description = "A basic flake";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";

  outputs = { self, nixpkgs, flake-utils }:
    {
      overlays.default = final: prev: {
        subrandr = prev.callPackage (import ./nix/default.nix) { buildRevision = self.shortRev or self.dirtyShortRev; };
      };
    } // (flake-utils.lib.eachDefaultSystem (system:
      let
        devPkgs = import nixpkgs {
          inherit system;
          overlays = [
            (final: prev: {
              freetype = prev.freetype.overrideAttrs (old: {
                # TODO: Building FreeType with HarfBuzz support regresses one of our tests!
                #       See https://gitlab.freedesktop.org/freetype/freetype/-/work_items/1482.
                configureFlags = (old.configureFlags or [ ]) ++ [
                  "--with-harfbuzz=no"
                ];
              });
            })
            self.overlays.default
          ];
        };
        runtimeLibs = with devPkgs; [
          alsa-lib
          udev
          vulkan-loader
          gcc-unwrapped
          libX11
          libXcursor
          libXrandr
          libXpresent
          libXi
          libxcb
          libGL
          libxkbcommon
          freetype
          harfbuzz
          fontconfig
        ];
      in
      {
        packages.default = (import nixpkgs { inherit system; overlays = [ self.overlays.default ]; }).subrandr;
        # TODO: This mess is only here because of sbr-overlay, get rid of it?
        devShells.default = devPkgs.mkShell {
          nativeBuildInputs = with devPkgs; [
            bashInteractive
            rust-bindgen
            pkg-config
            web-ext
          ] ++ runtimeLibs;
          buildInputs = runtimeLibs;
          shellHook = ''
            export FREETYPE_PATH=${devPkgs.freetype.dev}
            export HARFBUZZ_PATH=${devPkgs.harfbuzz.dev}
            export LD_LIBRARY_PATH="$LD_LIBRARY_PATH:${devPkgs.lib.makeLibraryPath runtimeLibs}"'';
        };
        devShells.ci = (devPkgs.subrandr.override {
          # Save on build time
          harfbuzz = devPkgs.harfbuzz.override {
            withGraphite2 = false;
            withIntrospection = false;
          };
        }).overrideAttrs (old: {
          nativeBuildInputs = old.nativeBuildInputs ++ [ devPkgs.cargo-nextest ];
        });
      }));
}
