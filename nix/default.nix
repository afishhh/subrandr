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

  cargoLock.lockFile = ../Cargo.lock;
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
    PREFIX=$out TARGET=${stdenv.hostPlatform.rust.rustcTarget} bash ./install.sh
  '';
}
