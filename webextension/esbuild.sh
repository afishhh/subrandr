#!/usr/bin/env sh

./node_modules/esbuild/bin/esbuild --sourcemap --bundle --watch=forever src/content.ts --outdir=./dist &
./node_modules/esbuild/bin/esbuild --bundle --watch=forever src/worker.ts --format=esm --outdir=./dist &

wait
