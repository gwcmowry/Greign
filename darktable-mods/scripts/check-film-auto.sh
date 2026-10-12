#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

native_test=/tmp/greign-test-film-analysis
native_roll_test=/tmp/greign-test-film-roll-analysis
uv_cache=/tmp/greign-uv-cache
output_dir=artifacts/film-conversions

cc -std=c99 -O2 -Wall -Wextra -Werror \
  -Idarktable/src \
  darktable/src/common/film_analysis.c \
  darktable/src/tests/unittests/common/test_film_analysis.c \
  -o "$native_test" \
  $(pkg-config --cflags --libs cmocka) -lm
"$native_test"

cc -std=c99 -O2 -Wall -Wextra -Werror \
  -Idarktable/src \
  darktable/src/common/film_roll_analysis.c \
  darktable/src/tests/unittests/common/test_film_roll_analysis.c \
  -o "$native_roll_test" \
  $(pkg-config --cflags --libs cmocka) -lm
"$native_roll_test"

UV_CACHE_DIR="$uv_cache" uv run --offline --python 3.12 scripts/test_film_auto.py
UV_CACHE_DIR="$uv_cache" uv run --offline --python 3.12 scripts/film_auto.py \
  fixtures/downloads/color-kodak-gold-snow-nikon-d780.nef \
  fixtures/downloads/color-aged-negative-canon.cr2 \
  fixtures/downloads/bw-negative-fujifilm-x-t2.raf \
  --output-dir "$output_dir" \
  --reference-dir "$output_dir/community-reference"
