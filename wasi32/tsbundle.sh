#!/usr/bin/env sh
./node_modules/esbuild/bin/esbuild src/subrandr.ts --bundle --format=esm --minify --sourcemap --outdir=dist/bundled
