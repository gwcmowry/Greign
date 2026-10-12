# JPEG-focused export and workflow polish

Restart with `./target/release/greign-app`. JPEG remains quality 95 by default;
the performance improvement does not depend on lowering quality or resolution.

## What changed

- Full-size JPEG: GPU finishing now includes sRGB conversion and explicit
  CPU-compatible rounding to RGB8. Only three bytes per pixel are read back,
  versus twelve for RGB32F: 75% less transfer data. The existing SIMD MozJPEG
  encoder and sRGB ICC profile are unchanged.
- Resized JPEG: GPU finishing emits **floating-point sRGB**, then the existing
  CPU Lanczos3 resize runs before final RGB8 quantization. This keeps the
  original processing order. An early-quantization prototype failed the
  decoded-JPEG comparison and was not retained in production.
- A bounded source-prefetch worker overlaps decoding/preparation of the next
  frame with GPU rendering and encoding. Only a few frames can be in flight,
  not an entire roll. The serial CPU benchmark disables both overlaps.
- TIFF/DNG keep their float path. Manual healing and GPU errors retain CPU
  fallback. Sources, output naming/publication, frozen recipes, selection,
  rejection handling and failed-only retries remain unchanged.

GPU operations still use the native context thread; upload/readback can briefly
pause UI painting. Full-size JPEG reduces that measured pause roughly in half.
There is no GPU JPEG bitstream encoder here, no reduced-resolution decode, and
no persistent prepared-source cache. Reading/decoding RAW remains CPU work.

## Workflow additions

- A saved JPEG quality slider (1–100), included in named presets and retries.
  Old libraries/presets load with quality 95.
- **Full JPEG · quality 95** and **Share JPEG · 2048 px** quick presets. Share
  explicitly sets quality 90; merely upgrading does not change existing files
  or their default quality. Quality 100 remains lossy and can be much larger.
- **Ctrl+E** opens export settings, without starting an export. Text/numeric
  fields retain their shortcuts; busy/analysis-comparison states are guarded.
- Completed-work progress, accumulated output megabytes and elapsed time as
  encoder jobs finish. Completion reports frame count, size and duration.
  The existing **Open export folder** button remains beside this summary.

## Measurements

RTX 5070 / Ryzen 9 7900X, release build, two actual Canon R7 RAW photographs
(`152A0123.CR3`, `152A0325.CR3`), positive mode, strong lighting/mixer edits.
All comparison JPEGs use **quality 95**, including the 2048-pixel benchmark,
so quality changes cannot explain the timing differences.

| Two-frame batch | Wall time |
| --- | ---: |
| Previous GPU export, full resolution | 3.937 s |
| New GPU JPEG export, full resolution | 3.003 s |
| New CPU pipeline, full resolution | 3.888 s |
| Sequential CPU reference, full resolution | 5.328 s |
| New GPU JPEG export, 2048-pixel longest edge | 3.279 s |
| New CPU pipeline, 2048-pixel longest edge | 3.850 s |

Full-resolution elapsed time fell about **24% versus the previous GPU export**.
The resized run is about **15% faster than the current CPU pipeline**. It is
not faster than full-size export in this sample: high-quality CPU resampling
and float readback add work even though the final JPEG is smaller.

Full-size GPU upload/draw/readback dropped from roughly 270 ms to 140–146 ms
per frame. Output finishing/encoding/writing dropped from 886–930 ms to about
330–350 ms. RAW decoding is now the main remaining cost (roughly 0.9–1.2 s
per frame); source preparation adds about 0.17–0.18 s. Stages overlap, so their
durations must not be summed to infer batch wall time. These are small,
warm-filesystem sample batches, not universal speed guarantees.

## Quality checks and artifacts

Twelve synthetic pre-codec cases cover positive, color-negative and B&W modes,
all four rotations, fractional crops, signed/HDR data and strong controls.
Both full-size RGB8 and float-resized RGB8 differ from CPU by at most one code.

After JPEG compression/decompression, the full-size comparison has mean
absolute differences below **0.0005 RGB8 codes**, with localized maxima of 7;
the sharing comparison is below **0.0002 codes**, maximum 3. JPEG's block
quantization can amplify tiny input differences locally. These are close
numerical agreement, not bit-identical JPEG files. Existing linear float/DNG
checks and the native preview parity suite also remain part of validation.

Viewable accepted outputs and raw stage logs:

- Full size: `exports/jpeg-export/18d3ccdadc7779af-0/`
- 2048 pixels: `exports/jpeg-export/18d3d2218afc09ab-0/`
- Previous implementation baseline: `exports/jpeg-export/18d3cc0bc2d85669-0/`

Each has `check.csv`, plus per-backend JPEGs and `stages.jsonl` files. The
older CSV parity rows are labelled `tiff-rgb16-parity` because the common
comparison decoder expands to RGB16 even for JPEG: divide those differences
by 257 for RGB8 units. The failed prototype folders are experimental outputs,
not the accepted implementation or visual references.

Reproduce in a graphical terminal; GPU runs require elevated approval when
invoked by the coding agent:

```sh
./target/release/greign-app --jpeg-export-check
./target/release/greign-app --jpeg-export-check --long-edge 2048
```

Tests, Clippy and release build pass. Logs: `artifacts/jpeg-tests.log`,
`artifacts/jpeg-clippy.log`, `artifacts/jpeg-build.log`. Hardware run logs are
`artifacts/jpeg-optimized-hardware.log`, `artifacts/jpeg-share-hardware.log`
and `artifacts/jpeg-preview-regression.log`.

The measurements use camera photographs, not real film negatives; they verify
export speed/fidelity rather than film-stock automatic-color accuracy.
