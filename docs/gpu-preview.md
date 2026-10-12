# GPU-backed native preview

Launch `./target/release/greign-app` (close an older running window first).
The status bar says **GPU · full-resolution source** once the active image
is resident. No browser or separate rendering server is involved.

## Quality and behavior

The active converted positive uses its **full-resolution, 32-bit linear
Rec.2020 source throughout slider dragging**, rather than falling back to a
480-pixel image. The GPU applies lighting, color balance, the eight-band
color mixer, positive/B&W tone behavior, crop, rotation, output-color
conversion and gamma encoding. Settings become shader uniforms; they do not
require re-uploading the image or downloading/repacking preview pixels.

The shader draws directly into the native window. At Fit it samples a
mipmapped version appropriate for the on-screen image size; at 100% it uses
the full source detail. It does not need to render an invisible 32 MP output
buffer for every viewport refresh. Zoom and pan remain independent of image
refinement. The same GPU path can draw the active contact-sheet card.

The converted source is still prepared on the CPU when a frame is first
opened or its calibration changes. One full source is retained on the GPU;
switching frames may require a new load/preparation/upload. Upload is deferred
while the pointer is held down to avoid interrupting an ongoing slider drag.
Until ready, the adaptive CPU placeholder remains available. Cold uploads can
still cause a brief UI hitch; this is not an instant-open implementation.

## Deliberate fallbacks

- Frames with **manual healing** use the existing CPU renderer. Healing
  depends on the edited pixels and is not approximated, omitted, or baked
  before the adjustments just to make the GPU path work. The status bar
  identifies this fallback.
- Original-negative inspection also remains on the CPU.
- Non-active thumbnails and pinned-reference textures use CPU previews;
  affected thumbnails update after release.
- JPEG, TIFF and DNG exports now have a separate full-resolution float GPU
  finishing path; see [GPU export](gpu-export.md). Decoding, conversion-source
  preparation, output encoding and manual-healing fallback still use the CPU.
- Shader initialization/upload failures or texture-size limits trigger CPU
  fallback. To force it for troubleshooting:

```sh
GREIGN_CPU_PREVIEW=1 ./target/release/greign-app
```

## RTX 5070 measurements

Measured on the actual NVIDIA GeForce RTX 5070, OpenGL 3.3, driver
580.95.05. Each size used 3 warm-up draws followed by 30 measured draws,
changing the exposure uniform on every draw. Times exclude source preparation,
upload, readback and PNG encoding:

| Output | Mean GPU draw | Mean synchronized host time |
| --- | ---: | ---: |
| 960×640 | 0.029 ms | 0.081 ms |
| 1920×1280 | 0.255 ms | 0.278 ms |
| 3840×2560 | 1.026 ms | 1.094 ms |
| 6960×4640 (32.3 MP) | 0.935 ms | 0.977 ms |

GPU time uses `GL_TIME_ELAPSED`; host time includes submission and `glFinish`.
Do not extrapolate frame rates or linear scaling from this table. Different
output sizes sample different mip levels, and this is an offscreen shader
benchmark, not end-to-end input/display latency.

The matching cached CPU full-resolution render/output preparation took
936.7 ms in one sample. Initial full-source upload plus mipmap generation
took 423 ms (earlier runs were around 387 ms). In the native UI check, CPU
source loading/preparation took about 2.01 s and upload submission about
399 ms. The remaining major cost is
therefore **cold source preparation/upload**, not repeated slider rendering.
The RGB32 source payload alone is about 388 MB; mipmaps and driver storage
layout add memory. This version intentionally retains float precision.

These results support using the full-resolution source for interactive edits
instead of merely increasing the old draft resolution. They do not imply
that image opening, exports, or the entire UI became hundreds of times faster.

## Correctness and native validation

The benchmark compares against the CPU renderer using a full photograph plus
signed/neutral/color fixtures, all four rotations, non-integer crop bounds,
strong lighting/color settings, and color-negative/B&W/positive materials.
The full photo differed by mean **0.032545 RGB8 code values**, maximum **2**;
the twelve synthetic material/rotation cases and three clipped-view cases
had maximum **1**. Acceptance
limits are mean below 0.1 and maximum at most 3 code values. This is close
numerical agreement, not bit-exact output. Parity is tested at matching pixel
resolution, not by equating different downsampling filters.

33 CPU-only routine tests pass, including delivery/reuse of the unchanged
converted GPU source when slider values change. Actual GPU checks are opt-in.
An isolated native window displayed the resident GPU preview, captured
`exports/gpu-preview/native-window.png` plus `native-zoom.png`, and closed successfully without
OS mouse/keyboard automation or changing the normal library. Full hands-on
interaction acceptance remains useful.

The callback renders only the visible viewport, with an explicit source-window
mapping into the unclipped image. This avoids egui's screen-clamped GL viewport
changing the sampled area when panning or zooming. Fractional display scaling
is covered by a CPU coordinate test and the native zoom capture.

The sample is a simulated negative derived from the existing R7 positive,
not evidence of real film-stock color accuracy.

## Reproduction and artifacts

GPU/device access requires unsandboxed approval when run by the coding agent.
From a normal graphical desktop terminal:

```sh
./target/release/greign-app --gpu-benchmark
./target/release/greign-app --gpu-ui-check
```

The first opens a short-lived validation window and writes
`artifacts/gpu-preview-benchmark.csv` plus GPU/CPU proof PNGs under
`exports/gpu-preview/`. It returns failure if parity checks fail.
The second uses the isolated `artifacts/gpu-ui-library.json`, saves the native
Fit and zoomed screenshots, and closes. Both require the local R7 comparison JPEG fixture.

Other logs: `artifacts/gpu-preview-validation.log`, `gpu-native-ui.log`,
`gpu-cpu-tests.log`, `gpu-preview-clippy.log`, and `gpu-preview-build.log`
(all under `artifacts/`). No GPU readback occurs in normal interactive preview;
preview readback is used only for benchmark proofs and comparisons.

The separate export path also performs readback to pass full-resolution float
pixels to the encoder. Ordinary interactive preview still never reads back.

## Implementation boundary

`greign-gl-preview` shares the native eframe OpenGL context, avoiding an
interop/readback bridge from the separate existing wgpu RAW backend. Only
this adapter permits OpenGL FFI `unsafe` blocks; the editor and film algorithms
retain the workspace's `unsafe_code = forbid` policy. Calls are restricted to
the owning render thread, upload dimensions/slice lengths are validated, and
GL resources are explicitly destroyed during native shutdown. Worker threads
only prepare immutable CPU images; they never issue GL calls.

Changes to CPU creative algorithms must update the shader and rerun GPU parity
before release. Do not silently route unsupported operations through a shader
that ignores them.
