# Third-party data

## Local upright-detection model

`greign-film` embeds OpenCV Zoo's 232,589-byte `face_detection_yunet_2023mar.onnx`
model. YuNet detects faces and five landmarks; Greign uses their evidence for
quarter-turn orientation, with abstention when the orientation is uncertain.
The weights are MIT licensed, copyright 2020 Shiqi Yu. Source, checksum, input
conventions, and the complete license are in
[`crates/greign-film/models`](../crates/greign-film/models/README.md).
Greign also embeds Duarte Barbosa’s EfficientNetV2 scene-orientation weights
(MIT, copyright 2025 Duarte Barbosa). The binary notice collector includes
`YUNET-LICENSE` and `SCENE-LICENSE`.

Tract 0.22.2 (MIT or Apache-2.0) executes the bundled model on the CPU. There are
no runtime downloads, Python dependency, cloud calls, or image uploads for CPU inference. The
lockfile pins `psm` 0.1.24 because newer versions pull in an archive writer that
requires Rust syntax beyond the workspace's former Rust 1.85 minimum.

Optional Linux CUDA inference uses `ort` 2.0.0-rc.10 (MIT or Apache-2.0) to load
ONNX Runtime 1.22.0 (MIT) dynamically. `scripts/setup-upright-gpu.py` installs
the pinned optional runtime under the user's workspace with UV. NVIDIA CUDA
and cuDNN packages retain their vendor license files in that local installation;
they are not embedded in the Greign binary or macOS bundle. The model and its
MIT notice are shared by both inference paths. Runtime CUDA uses FP32 with TF32
disabled; CUDA initialization/compute failures return to the CPU implementation.

## Binary distribution notices

macOS bundles include `ThirdPartyNotices.txt`, generated from dependency
license files (including nested C codec and font notices), and Greign's GPL
license. Release source downloads include the complete patched Rawler source.
The dependency policy accepts the Boost, CC0, NCSA and bundled font licenses,
and has an explicit exception for the vendored Rawler 0.7.0 LGPL-2.1 license.

## Maintenance notices reviewed for the initial macOS release

The October 2026 audit found no known security vulnerabilities. It reported
maintenance notices RUSTSEC-2024-0436 (`paste` 1.0.15) and RUSTSEC-2026-0192
(`ttf-parser` 0.25.1) in the existing dependency graph. These are recorded as
specific cargo-deny exceptions; security advisories and yanked dependencies
continue to fail. Cargo-audit still reports the maintenance warnings.
Revisit both when upgrading the rendering and font dependencies.

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
