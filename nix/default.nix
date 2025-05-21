buildRevision:
{ rustPlatform
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
rustPlatform.buildRustPackage {
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
    cargo xtask install -p $out --target ${stdenv.targetPlatform.rust.rustcTarget}
  '';
}
