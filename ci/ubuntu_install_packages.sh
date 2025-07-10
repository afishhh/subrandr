#!/usr/bin/env bash

set -euo pipefail

packages=(
  build-essential
)
library_suffix=""

case "$CROSS_ARCH" in
  i686)
    sudo dpkg --add-architecture i386
    packages+=(gcc-multilib g++-multilib)
    library_suffix=":i386"
    ;;
  "");;
  *)
    echo "$CROSS_ARCH" not recognised >&2
    ;;
esac

for library in $LIBRARIES; do
  packages+=("$library$library_suffix")
done

sudo apt-get update
# shellcheck disable=SC2086
sudo apt-get install -y build-essential "${packages[@]}" $EXTRA_PACKAGES
