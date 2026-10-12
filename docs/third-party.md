# Third-party data

## colord sRGB ICC profile

`greign-export` embeds the standard sRGB profile distributed by the colord
project as `data/profiles/sRGB.icc`. Debian's `colord-data` copyright metadata
declares `data/profiles/*` under CC0-1.0. The embedded bytes were taken from
colord-data 1.4.7-1build2 on Ubuntu 24.04.

Source: <https://gitlab.freedesktop.org/colord/colord>

License: <https://creativecommons.org/publicdomain/zero/1.0/>

## MozJPEG

`greign-export` uses MozJPEG 4 through the `mozjpeg` and `mozjpeg-sys` crates
for the measured high-throughput JPEG export path. The wrapper and bundled
codec sources carry the permissive IJG, zlib, and BSD-3-Clause licenses. The
reference `image` encoder remains available for deterministic oracle tests.

Source: <https://github.com/mozilla/mozjpeg>

License notices are included by the Cargo source packages and must be carried
into binary distribution notices.

## Rayon

`greign-gpu` uses Rayon to parallelize CPU-side packed RAW upload preparation
and removal of RGBA row padding/alpha after export readback. Rayon is dual
licensed MIT or Apache-2.0.

Source: <https://github.com/rayon-rs/rayon>

## Darktable highlight reconstruction

Greign's opposed-plane clipped-highlight reconstruction is derived from the
algorithm and cube-domain opposing-plane estimator in Darktable 4.6.1
`src/iop/hlreconstruct/opposed.c`, rewritten for Greign's Rust CPU oracle and
fused WGSL sensor kernel. Copyright remains with the Darktable contributors.
Darktable and Greign are both licensed GPL-3.0-or-later.

Darktable is also used as an optional external render oracle during local
quality testing; it is not linked into or distributed with Greign.

Source: <https://github.com/darktable-org/darktable/tree/release-4.6.1>

Greign's version-2 regional boundary estimator keeps a separate connected-region
context for the opposed-plane reconstruction. The integer RAW decoder is now
the vendored Rawler 0.7.0 source with one Canon ColorData version-48 white-level
offset correction; see `vendor/rawler/GREIGN-PATCHES.md`. Original Rawler
license notices and source are retained.

License: <https://www.gnu.org/licenses/gpl-3.0.html>
