#!/usr/bin/env bash

set -euo pipefail

target_arg=()
if [[ -n "${CROSS_TARGET:-}" ]]; then
  target_arg=(--target "$CROSS_TARGET")
fi

cd ci
cargo xtask install --prefix pfx "${target_arg[@]}"
mkdir -p pfx/bin
# shellcheck disable=SC2086
"${CC:-cc}" ${CFLAGS:-} -I pfx/include ./c_sanity_check.c -L pfx/lib -lsubrandr -o pfx/bin/sanity_check.exe

export LD_LIBRARY_PATH=$PWD/pfx/lib
cd pfx/bin

file sanity_check.exe
./sanity_check.exe
