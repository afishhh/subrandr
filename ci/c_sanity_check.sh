#!/usr/bin/env bash

set -euo pipefail

cd ci
cargo xtask install --prefix pfx
mkdir -p pfx/bin
"${CC:-cc}" -I pfx/include ./c_sanity_check.c -L pfx/lib -lsubrandr -o pfx/bin/sanity_check.exe

export LD_LIBRARY_PATH=$PWD/pfx/lib
cd pfx/bin

./sanity_check.exe
