#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "$0")"

if ! hash bindgen 2>/dev/null; then
	nix shell nixpkgs#rust-bindgen -c "$0"
fi

bindgen \
	--raw-line '#![allow(non_upper_case_globals)]' \
	--raw-line '#![allow(non_camel_case_types)]' \
	--raw-line '#![allow(non_snake_case)]' \
	--raw-line '#[cfg(target_family = "unix")]' \
	--raw-line 'pub mod unix;' \
	./header.h >src/lib.rs

bindgen \
	--raw-line '#![allow(non_upper_case_globals)]' \
	--raw-line '#![allow(non_camel_case_types)]' \
	--raw-line '#![allow(non_snake_case)]' \
	--allowlist-item 'F[cC].*' \
	--no-prepend-enum-name \
	./header-unix.h >src/unix.rs
