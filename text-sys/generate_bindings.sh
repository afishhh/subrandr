#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "$0")"

if ! hash bindgen 2>/dev/null; then
	nix shell nixpkgs#rust-bindgen -c "$0"
fi

# NOTE: #![allow(improper_ctypes)] silences a warning about (u|i)128,
#       these types are generated but not used anywhere and it's easier
#       to just silence the warning and avoid using them.
# NOTE: Layout tests are disabled to allow building on architectures
#       with different integer sizes without regenering the definitions.
bindgen \
	--raw-line '#![allow(non_upper_case_globals)]' \
	--raw-line '#![allow(non_camel_case_types)]' \
	--raw-line '#![allow(non_snake_case)]' \
	--raw-line '#![allow(improper_ctypes)]' \
	--raw-line '#[cfg(target_family = "unix")]' \
	--raw-line 'pub mod unix;' \
	--no-prepend-enum-name \
	--no-layout-tests \
	./header.h >src/lib.rs

./generate_errordefs.py >>src/lib.rs

bindgen \
	--raw-line '#![allow(non_upper_case_globals)]' \
	--raw-line '#![allow(non_camel_case_types)]' \
	--raw-line '#![allow(non_snake_case)]' \
	--allowlist-item 'F[cC].*' \
	--no-prepend-enum-name \
	--no-layout-tests \
	./header-unix.h >src/unix.rs
