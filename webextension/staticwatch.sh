#!/usr/bin/env sh
find static ../wasi32/dist/bundled ../target/wasm32-wasip1/debug/subrandr.wasm | entr -s 'mkdir -p dist && cp -r static/* ../wasi32/dist/bundled/* ../target/wasm32-wasip1/debug/subrandr.wasm dist'
