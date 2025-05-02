#!/usr/bin/env bash

cargo b --target wasm32-wasip1 --features wgpu/webgpu

mkdir -p build

wasm-bindgen ../target/wasm32-wasip1/debug/subrandr.wasm --target web --out-dir build/bindgen

[ -e build/setjmp.wasm ] ||
  wasm-ld --whole-archive --warn-unresolved-symbols --export-all deps/libsetjmp.a --no-entry -o build/setjmp.wasm --import-memory=subrandr,memory
[ -e build/harfbuzz.wasm ] ||
  wasm-ld --whole-archive --warn-unresolved-symbols --export-all deps/libharfbuzz.a --no-entry -o build/harfbuzz.wasm --import-memory=subrandr,memory
[ -e build/freetype.wasm ] ||
  wasm-ld --whole-archive --warn-unresolved-symbols --export-all deps/libfreetype.a --no-entry -o build/freetype.wasm --import-memory=subrandr,memory

wasm-merge -o build/subrandr.wasm build/bindgen/subrandr_bg.wasm subrandr build/setjmp.wasm env build/freetype.wasm env build/harfbuzz.wasm env -cw -n --enable-exception-handling --rename-export-conflicts
wasm-opt -O3 build/subrandr.wasm -o build/subrandr-opt.wasm
