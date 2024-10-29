#!/usr/bin/env bash

set -euo pipefail

wid=$(xdotool search --limit 1 --class mpv || true)
if [[ -z "$wid" ]]; then
  echo "No mpv window found" >&2
  exit 1
fi

self="$(basename "$(realpath -s "$0")")"
if [[ $self == *release* ]]; then
  release_flag="--release"
else
  release_flag=""
fi

echo -n cargo run "$release_flag" -q --example=x11-transparent-window -- \
  --overlay "$wid"

for arg in "$@"; do
  printf " %q" "$arg"
done

echo

exec cargo run $release_flag -q --example=x11-transparent-window -- \
  --overlay "$wid" \
  "$@"
