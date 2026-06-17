{ rustPlatform
, stdenv
, pkg-config
, harfbuzz
, freetype
, fontconfig
, buildRevision ? null
, ...
}:

let
  cargoToml = (builtins.fromTOML (builtins.readFile ../Cargo.toml));
in
rustPlatform.buildRustPackage {
  pname = cargoToml.package.name;
  version = cargoToml.package.version;

  SUBRANDR_BUILD_REV = buildRevision;

  cargoLock.lockFile = ../Cargo.lock;
  src = ../.;

  nativeBuildInputs = [
    pkg-config
  ];

  buildInputs = [
    harfbuzz
    freetype
    fontconfig
  ];

  buildPhase = ''
    cargo xtask build --target ${stdenv.targetPlatform.rust.rustcTarget}
  '';

  installPhase = ''
    cargo xtask install -p $out --target ${stdenv.targetPlatform.rust.rustcTarget}
  '';
}
