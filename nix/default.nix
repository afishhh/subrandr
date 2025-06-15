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
  cargoToml = (builtins.fromTOML (builtins.readFile ../Cargo.toml));
in
rustPlatform.buildRustPackage {
  pname = cargoToml.package.name;
  version = cargoToml.workspace.package.version;

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
