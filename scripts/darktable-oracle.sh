#!/bin/sh
set -eu

if [ "$#" -ne 2 ]; then
    printf '%s\n' "usage: $0 INPUT_RAW OUTPUT_TIFF" >&2
    exit 2
fi

input=$1
output=$2
expected_version=${GREIGN_DARKTABLE_VERSION:-4.6.1}

if ! command -v darktable-cli >/dev/null 2>&1; then
    printf '%s\n' "darktable-cli is required for the external render oracle" >&2
    exit 1
fi
if [ ! -f "$input" ]; then
    printf 'input does not exist: %s\n' "$input" >&2
    exit 1
fi
if [ -e "$output" ]; then
    printf 'refusing to overwrite oracle output: %s\n' "$output" >&2
    exit 1
fi

oracle_tmp=$(mktemp -d /tmp/greign-darktable-oracle.XXXXXX)
cleanup() {
    rm -rf -- "$oracle_tmp"
}
trap cleanup EXIT HUP INT TERM

export XDG_CONFIG_HOME="$oracle_tmp/config"
export XDG_CACHE_HOME="$oracle_tmp/cache"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"

actual_version=$(darktable-cli --version | sed -n '1s/^darktable //p')
if [ "$actual_version" != "$expected_version" ]; then
    printf 'darktable version mismatch: expected %s, found %s\n' \
        "$expected_version" "$actual_version" >&2
    exit 1
fi

mkdir -p "$(dirname "$output")"
darktable-cli "$input" "$output" \
    --apply-custom-presets false \
    --hq true \
    --out-ext tif \
    --icc-type sRGB \
    --core \
    --disable-opencl \
    --library :memory:

printf 'darktable=%s input=%s output=%s\n' "$actual_version" "$input" "$output"
