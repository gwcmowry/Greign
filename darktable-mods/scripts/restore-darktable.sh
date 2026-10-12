#!/bin/sh
# Reproduce the independent darktable work without distributing build outputs.
set -eu
cd "$(dirname "$0")/.."
test ! -e darktable || { echo "darktable/ already exists; leave it intact." >&2; exit 1; }
git clone --filter=blob:none --no-checkout https://github.com/darktable-org/darktable.git darktable
git -C darktable checkout -b greign/film-workflow "$(cat patches/upstream-commit.txt)"
git -C darktable apply --check ../patches/film-workflow.patch
git -C darktable apply ../patches/film-workflow.patch
git -C darktable submodule update --init --recursive
echo "Restored darktable sources and Greign modifications. Build from a fresh directory."
