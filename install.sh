#!/usr/bin/env bash

set -euo pipefail

version=$(cargo metadata --no-deps --offline --format-version 1 | jq -r '.packages.[] | select(.name == "subrandr").version')

target_dir=target
if [[ -n ${TARGET:-} ]]; then
  target_dir="target/$TARGET"
fi

mkdir -p "$PREFIX/lib/pkgconfig" "$PREFIX/include/subrandr"
cp "${target_dir}/release/libsubrandr.a" "$PREFIX/lib"
cp -r ./include/* "$PREFIX/include/subrandr"
cat >"$PREFIX/lib/pkgconfig/subrandr.pc" <<EOF
prefix=$PREFIX
libdir=$PREFIX/lib
includedir=$PREFIX/include

Name: subrandr
Description: A subtitle rendering library
Version: $version
Requires: freetype2 >= 26, harfbuzz >= 10, fontconfig >= 2
Cflags: -I\${includedir}
Libs: -L\${libdir} -lsubrandr
EOF
