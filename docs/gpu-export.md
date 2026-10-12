# GPU-assisted exports

JPEG-specific finishing and source prefetch were subsequently added; see
[JPEG export](jpeg-export.md) for current behavior and measurements. The
measurements below document the initial GPU-export implementation.

Launch the rebuilt app with `./target/release/greign-app`. Export automatically
uses the available native GL context; no new setting is required. To isolate
CPU-only export while keeping GPU previews:

```sh
GREIGN_CPU_EXPORT=1 ./target/release/greign-app
```

## Processing and boundaries

RAW decoding and film conversion-source preparation remain CPU work. The GPU
then applies lighting, color balance, the eight-band mixer, material-specific
finishing, integer-aligned crop and rotation. It renders into an RGBA32F
offscreen target and returns RGB float samples, not a screen capture or RGB8
preview. Exact source texels are used, with no mipmap/downsample approximation.
TIFF color-space conversion and optional Lanczos resizing remain CPU-side;
JPEG now uses GPU sRGB conversion, with floating-point readback when resizing
and compact RGB8 readback at full size. DNG receives linear Rec.2020 directly. Its existing writer retains values above
one and floors negative channels at zero, unchanged from CPU export.

The source and target are allocated per frame, independently of the active
preview texture. GPU operations run on the native context's owner thread;
framebuffer/viewport and readback state are restored before normal painting.
This avoids cross-thread GL access but can briefly pause UI painting during
upload/render/readback (about 278 ms per full-resolution frame in this check).
A separate export context and asynchronous transfers are future optimizations.
Prepared-source cache reuse is not implemented: each exported file is decoded
afresh, so export does not accidentally use a stale preview source.

One background encoder and bounded queues overlap output conversion,
compression and writing with the next frame's preparation/render. A source
prefetch worker now also overlaps decoding/preparation with rendering. Memory is
bounded by a few frames, not the roll size. The queue is drained before export
reports completion. Frozen recipes, selection/rejection rules, output sizing,
pending-file publication and failed-only retries are preserved.

Manual-healing frames use the CPU reference renderer, never a GPU path that
omits the repairs. Missing GPU support, allocation errors or a timed-out GPU
request fall back to CPU finishing. Backend/fallback reason and stage durations
are recorded in `.greign/timings.jsonl`.

## Measurements (2026-09-09)

Actual RTX 5070, Ryzen 9 7900X, release build. Two Canon R7 RAW files,
`152A0123.CR3` and `152A0325.CR3`, full resolution, positive mode, lighting and
all mixer bands enabled, lossless RGB16 ZIP TIFF output:

| Scheduling / rendering | Two-frame wall time |
| --- | ---: |
| Sequential CPU reference | 6.166 s |
| Overlapped CPU | 4.666 s |
| Overlapped GPU | 4.356 s |

GPU plus overlapping work reduced elapsed time by about 29% versus the
sequential reference. GPU alone reduced it about 7% versus the overlapped CPU
run. An earlier overlapped run measured 4.795 s CPU and 4.346 s GPU. These are
small, warm-filesystem sample batches, not a universal speed guarantee. The
sequential reference uses the same current renderer/encoder with overlap
disabled, rather than an older executable.

Per-frame GPU-run stage timings:

- RAW decode: 935–1182 ms.
- CPU source preparation: 196–199 ms.
- GPU upload, draw and synchronous readback: about 278 ms.
- CPU output conversion, compression and write: 1259–1302 ms, overlapping the
  next frame. Stage durations therefore must not be summed as batch wall time.

Remaining bottlenecks are decoding and encoding, followed by GPU transfer work.
This is not the 1 ms shader-only preview benchmark: full export must move and
encode its pixels. Next optimizations should target those measured costs.

## Validation and reproduction

Run from a graphical terminal (the coding agent requires elevated permission
for GPU/native context access):

```sh
./target/release/greign-app --gpu-export-check
```

This creates a short-lived native validation window, uses isolated roll
objects (never the user's library), and saves outputs in a new directory under
`exports/gpu-export/`. New runs print that directory and store `check.csv`
inside it; each backend folder contains its raw `stages.jsonl` timing records.
The original measurement log remains in `artifacts/gpu-export-check.csv`.

The latest measured directory is
`exports/gpu-export/18d3c8159aac9d76-0/`, with `serial-cpu`, `cpu`, and `gpu`
TIFF folders and small CPU/GPU DNG fixtures at its root.

- Both full-resolution GPU TIFFs differ from CPU by at most **one RGB16 code
  value**; mean absolute differences are 0.001406 and 0.001454 codes.
- Twelve signed/HDR float fixtures cover three material modes and all four
  rotations with a fractional crop and strong controls. Maximum absolute
  linear error was 0.00000343. Acceptance is 0.00001 absolute plus 0.00002
  relative, per sample; this is numerical agreement, not bit identity.
- DNG strips independently read back exactly the samples supplied to the
  writer, after its documented negative-channel floor; HDR values survive.
- CPU tests cover GPU failure fallback, manual-healing preservation, crop
  dimensions and the existing export/retry/selection cases.

Tests, Clippy and release-build logs are `artifacts/gpu-export-tests.log`,
`artifacts/gpu-export-clippy.log` and `artifacts/gpu-export-build.log`.
The existing GPU preview parity suite is also run after the shared-shader
change; its log is `artifacts/gpu-export-preview-regression.log`.

These checks validate rendering and file preservation, not automatic color
quality on real film scans. The speed sample uses actual camera photographs,
while the negative-mode finishing fixtures are synthetic.
