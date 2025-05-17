buildRevision:
{ pkgs
, stdenv
, pkg-config
, jq
, harfbuzz
, freetype
, fontconfig
, ...
}:

let
  cargoTomlPackage = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).package;
in
pkgs.rustPlatform.buildRustPackage {
  pname = cargoTomlPackage.name;
  version = cargoTomlPackage.version;

  SUBRANDR_BUILD_REV = buildRevision;

  cargoLock = {
    lockFile = ../Cargo.lock;
    outputHashes = {
      "naga-25.0.0" = "sha256-0SIuQ9xn0ys3atmFwCi6Vb95dcuhPIOTFcR3k15k6d0=";
    };
  };
  src = ../.;

  nativeBuildInputs = [
    pkg-config
    jq
  ];

  buildInputs = [
    harfbuzz
    freetype
    fontconfig
  ];

  installPhase = ''
    cargo xtask install -p $out --target ${stdenv.hostPlatform.rust.rustcTarget}
  '';
}
