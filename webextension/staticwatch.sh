#!/usr/bin/env sh
find static ../wasi32/dist/bundled ../target/wasm32-wasip1/release/subrandr.wasm | entr -s 'mkdir -p dist && cp -r static/* ../wasi32/dist/bundled/* ../target/wasm32-wasip1/release/subrandr.wasm dist'
