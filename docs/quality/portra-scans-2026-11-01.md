# Portra scan preview and holder investigation

Seven private Canon EOS R7 C-RAW scans exposed two independent defects. The
original-negative display fed unbalanced camera channels into the camera color
matrix, producing a strong green cast. Display now applies as-shot white balance;
the linear scan, film-base measurements, and positive inversion remain independent
of that display operation. The camera's embedded preview independently confirms
the orange film border.

The dark holder was also included in density-range estimation whenever it
extended beyond the roll's fixed analysis inset. In this set, the shared range
was 3.105 before exclusion and approximately 0.95 after exclusion. The new
detector found a holder in all seven scans, including the smaller, off-center
opening in IMG_0005. Analysis version 2 intersects each detected opening with
the configured analysis area. Accepted version-1 calibrations are preserved
until the user accepts a new proposal.

The holder detector uses linear-channel signal, dark perimeter checks, and agreement
between 41 scanlines per axis. It returns an axis-aligned crop with a small
safety inset, retaining the orange film border. It declines borderless,
partial-border, uniformly dark, or irregular cases. It does not identify the
inner exposure boundary, deskew film, or guarantee a pixel-exact edge for every
holder. It remains the calibration exclusion detector. The inner-picture
autocrop added below replaces its former use by the Auto crop button.
The native UI exposes manual crop and auto crop in the image toolbar;
manual crop now works on the full original so an existing crop can be expanded.
macOS AppleDouble files such as `._IMG_0005.CR3` are excluded from import.

## Lab reference comparison

The supplied JPEGs cover the whole roll and include light user edits. Visual
matches are:

| Camera scan | Lab JPEG |
| --- | --- |
| IMG_0005, IMG_0006 | 000018870003.jpg |
| IMG_0007 | 000018870004.jpg |
| IMG_0008 | 000018870002.jpg |
| IMG_0009 | 000018870001.jpg |
| IMG_0010 | 000018870011.jpg |
| IMG_0011 | 000018870010.jpg |

The RAW images are mirrored relative to the lab scans, with rotation also
needed for viewing. Comparison images may correct orientation, but the new
crop behavior does not automatically choose quarter-turn rotation or mirroring.

The lab references reveal remaining conversion limitations. Estimated red
film-base levels range from approximately 0.120 to 0.411 (about 3.4×), with
similar changes in green and blue. These are estimates from scene/border
samples, not a definitive exposure-meter reading. A shared base can brighten
the lower-exposure captures excessively. The app now reports this variation;
it does not silently normalize exposure or replace accepted calibration.
Individual-frame diagnostic conversions help separate that effect from the
remaining film-color model differences. Neither holder exclusion nor display
white balance is a claim of lab-matched positive color.

The subsequent UI update exposes **Optimize frames individually** as a
conversion mode above Analyze, and **Invert orientation** as a mirror toggle
beside Crop. Per-frame calibration is persisted with accepted/proposed
conversion state and used in all previews and export formats. It addresses
varying scan exposures without claiming to reproduce the lab's color treatment.
The shared calibration remains available for consistently exposed scans.

## Reproduction and checks

Private photographs and generated outputs remain ignored by Git. From the
repository root:

```sh
cargo run --offline --release -p greign-film --example scan_check -- \
  /path/to/raws exports/portra-investigation/after \
  > artifacts/portra-after.txt
cargo test --offline -p greign-film -p greign-app
./scripts/check.sh
```

The diagnostic example saves full negative previews, shared-roll positives,
and cropped shared/individual-calibration comparisons. Cropped proof images
are rotated 180 degrees for this fixture set; that is not an application edit.
Regression coverage includes camera-WB display without mutating calibration,
holder-induced density inflation, conservative detection failures, native
pointer-driven expansion of a rotated crop, crop persistence and Undo/Redo,
and preservation of locked accepted conversions.

Validation on this workstation: all repository checks passed (formatting,
Clippy, 144 passing tests with 9 opt-in tests ignored, documentation, and
debug/release schema agreement). Native replay on an isolated test library
confirmed the corrected orange preview and visible crop toolbar. Local visual
evidence is in `exports/portra-investigation/ui-crop.png` and
`exports/portra-investigation/lab-comparison.jpg`; logs are in
`artifacts/portra-*.log` and `artifacts/portra-after.txt`.

After the individual-mode and orientation update, all repository checks pass
again (148 tests). `greign-app --orientation-check` exercises the GPU against
the CPU for all four rotations, with and without mirroring, in all three
material modes; float, JPEG, resized JPEG, and DNG roundtrip checks pass.
Run hardware checks only with explicitly permitted GPU access. Native replay
selected individual mode, analyzed all seven scans, applied auto crop and
mirroring, and saved the result in the isolated
`artifacts/portra-individual-library.json` test library. Screenshot:
`exports/portra-investigation/ui-individual-mirrored.png`.

## Inner-picture autocrop and straightening, 2026-11-04

Auto crop now fits the exposed picture's four edges inside the film base.
It samples 61 scanlines per side, compares several local base/contrast estimates,
and selects straight boundaries that agree on tilt and film-base brightness.
Consensus fitting rejects interrupted borders and scene details. Detection is
bounded to a 960-pixel analysis dimension and permits up to 5 degrees of tilt.
A small inward margin excludes soft edges and the interpolation footprint;
estimated tilt below 0.1 degrees is left unrotated. Missing or inconsistent
boundaries leave the existing crop intact.

The crop stores a backward-compatible angle, shared by CPU rendering, GPU
preview, exports, source-coordinate tools, and the original-scan crop outline.
Integer crops preserve their previous pixel selection. Tilted crops interpolate
finished linear pixels before quarter turns and mirroring. GPU minification
retains mipmap filtering. Reset, manual replacement, clipboard, persistence, and
Undo/Redo cover the new angle without changing accepted conversion state.

All seven private scans produce an inner crop. Detected angles range from
-0.61 to +0.27 degrees. Updated local renders are in
`exports/portra-investigation/frame-crop/`; native replay screenshots are
`autocrop-ui.png`, `autocrop-outline.png`, `autocrop-undo.png`, and
`autocrop-redo.png` in the parent directory. Saved replay snapshots confirm
that Undo restores the earlier holder crop and Redo restores the inner crop.

Regression cases include both tilt directions through 4.5 degrees, color and
monochrome base, uneven illumination, dust, faint blurred boundaries, portrait
and larger scans, missing borders, invalid data, coordinate inversion, and
legacy JSON. Repository checks pass with 153 tests and 9 opt-in tests ignored.
Hardware checks cover 72 combinations of three materials, four quarter turns,
mirroring, and three tilt angles. Maximum CPU/GPU linear difference is
0.00002665 on the discontinuous HDR fixture; preview, JPEG RGB8, and resized
JPEG differ by at most one 8-bit level. DNG float roundtrips preserve samples.
The tilted float test allows 0.00005 absolute error plus the existing relative
tolerance for GPU fused multiply-add rounding; zero-tilt tolerance is unchanged.

Repeated detection took roughly 6–10 ms per local preview scan during concurrent
builds. `cargo-flamegraph` was attempted, but kernel perf permissions blocked
sampling and sudo required a password. Temporary timing instrumentation instead
recorded 5,040 edge fits: 72% of measured time was scanline sampling/thresholding
and 28% consensus line fitting. Raw measurements are saved in
`artifacts/perf/autocrop-stages.csv`; instrumentation was removed afterward.
The diagnostic example accepts an optional repetition count for timing:

```sh
cargo run --offline --release -p greign-film --example scan_check -- \
  fixtures/private/portra-2026-11-01 exports/portra-investigation/frame-crop 30
```

Final check logs are `artifacts/autocrop-repository-checks.log`,
`artifacts/autocrop-gpu.log`, and `artifacts/autocrop-gpu-preview.log`.

## Whole-roll controls

New rolls now default to individual optimization. Explicitly saved choices and
legacy shared-calibration libraries retain their mode. The top **Whole roll**
toolbar exposes **Autocrop all frames** and a mixed-state **Mirror all frames**
checkbox in both contact-sheet and Single frame views. Mirroring sets one
consistent state while preserving individual quarter turns and crop angles.
Each whole-roll operation is one Undo step.

Batch autocrop runs on the background worker, reusing cached scans and loading
missing previews when needed. It includes all frames, including rejected ones.
Uncertain boundaries, read failures, or crops edited while detection was running
are retained and named in the completion message. There is no separate holder
crop detector anymore. Analysis version 3 uses the same picture geometry to
exclude non-picture density samples and estimates film base from the uncropped
capture. Effectively opaque samples are rejected if picture detection fails.
Accepted conversions remain intact until a new proposal is accepted.

All repository checks pass: 157 tests, 9 opt-in tests ignored, formatting,
Clippy, documentation, and schema agreement. New coverage includes default/legacy
modes, batch processing with mixed cached/uncached/uncertain/unreadable inputs,
preservation of newer crops, mixed mirror states, atomic Undo/Redo, tilted
analysis geometry, and pointer clicks on both top controls at 1080 and 1440
logical-pixel window widths. Logs: `artifacts/roll-tools-repository-checks.log`.

Native replay on a separate seven-frame library cropped all seven, changed a
mixed 4-of-7 mirror state to 7-of-7, undid both edits, and restored them with
Redo. Snapshots verify unchanged accepted calibration and quarter turns.
Screenshots and snapshots are `exports/portra-investigation/roll-tools-*.png`
and `.json`; replay log is `artifacts/roll-tools-ui.log`. Version-3 diagnostic
renders are under `exports/portra-investigation/roll-tools/`.

## Local automatic upright detection

The **Auto upright** button now uses the MIT-licensed YuNet model bundled in
`greign-film/models/`. It runs on the CPU through Tract 0.22.1; no runtime
downloads, Python environment, network service, or GPU inference are needed.
YuNet's five face landmarks are sufficient for this prototype, so the larger
MediaPipe face-mesh pipeline discussed initially was not needed.

Each frame is rendered from its accepted conversion with neutral creative
controls, its picture crop, and zero quarter-turn/mirror. Four quarter-turn
variants are evaluated. Non-maximum suppression removes duplicate faces;
landmark checks reject implausible layouts. A strong face score (at least 0.90)
and a winning margin (at least 0.08, with limited second-face corroboration)
are required. These scores are evidence, not calibrated rotation probabilities.
Uncertain/no-face frames retain rotation and get a review flag. Existing review
flags, crop/tilt, mirroring, edits, and calibration are preserved. Stale results
cannot overwrite newer composition edits. One Undo restores the entire batch.

On the seven original positive previews, all six people photos received the
correct orientation, including the sideways IMG_0008 portrait; the skyline
received no rotation suggestion. A 56-case check covers each picture at four
starting rotations, with and without horizontal mirroring: 44 confident
decisions were correct, with no wrong decisions. Twelve cases were deferred:
the eight skyline variants and four mirrored variants of a blurry frame.
Conservative abstention is allowed by the regression check.

Native replay began with mixed quarter turns and mirror settings. It rotated
four frames, recognized two as already upright, and marked the skyline for
review. Snapshots confirm unchanged crop, mirror, edits, and accepted conversion;
Undo and Redo restore the complete orientation/review state. Evidence:
`exports/portra-investigation/upright/ui-upright.png`, `ui-undo.png`,
`ui-redo.png`, and their JSON snapshots.

Native CPU detection across four rotations had a median of roughly 0.43 seconds
per picture during concurrent builds; model initialization took about 27 ms.
Raw timings and scores are in
`exports/portra-investigation/upright/upright.jsonl`. Cargo-flamegraph was
attempted (`artifacts/upright-profile.log`) but kernel perf permissions blocked
sampling. The CPU OpenCV prototype was separately profiled with cProfile:
face inference took 0.481 s across 28 calls versus 0.009 s for resizing, making
model inference the dominant measured prototype cost. Raw profile:
`artifacts/perf/upright-prototype.prof`. The production implementation is Rust.

All repository checks pass with 162 tests and 9 opt-in tests ignored. The film
engine also passes a Rust 1.85 check. New tests cover actual bundled-model
inference on blank input, competing/weak face evidence, invalid landmark
layouts, atomic application, review flags, stale edits, missing conversion,
engine errors, and the visible button at both supported window widths.
The binary-notice collector includes the model's MIT license. Logs:
`artifacts/upright-final-checks.log`, `upright-msrv.log`, and
`upright-license-check.log`.

```sh
cargo run --offline --release -p greign-film --example upright_check -- \
  exports/portra-investigation/roll-tools \
  exports/portra-investigation/upright --verify-portra
```

## Rotation shortcut and CPU latency

**R** now rotates the contact-sheet selection clockwise; **Shift+R** reverses
the turn. Single frame view (or an empty selection) targets the active frame.
Mirrored frames use the opposite source quarter turn to keep the visible
clockwise direction consistent. Each key press is one persisted Undo step.
Text/numeric editing, modified shortcuts, key repeats, and read-only states
are guarded. Keep/reject moved to **X**. A visible **Rotate selected · R** button
uses the same action. Native replay verified multi-selection, Undo, and Shift+R;
evidence is `exports/portra-investigation/rotation-shortcut-*.png` and `.json`.

The upright detector now evaluates the four independent orientations in a
dedicated Rayon pool capped at four workers (or available CPU parallelism).
The model, 640-pixel input resolution, thresholds, and per-view computation
are unchanged. Across the same 56 variants, serial and four-worker results
have exactly equal decisions, face counts, and scores: 44 correct decisions,
12 abstentions, zero wrong decisions.

Controlled release timings on this workstation:

| Execution | Median/photo | 95th percentile |
| --- | ---: | ---: |
| One CPU worker | 378.4 ms | 397.5 ms |
| Four CPU workers | 110.3 ms | 114.2 ms |

That is about 3.4× faster. A separate native seven-frame run, including positive
preview preparation from cached scans, measured a 112.4 ms median (110.9–122.3
ms range). RAW file loading and the one-time model initialization are additional.
Automatic orientation remains CPU-only; the app display uses its existing GPU
preview. Raw measurements are saved in
`artifacts/perf/upright-parallel-comparison.json` and `.greign/timings.jsonl`;
native proof is `exports/portra-investigation/upright-parallel/native-fast-upright.png`.

Kernel perf restrictions still prevented sampling. The standalone Tract
operation profiler saves raw node timings to
`artifacts/perf/upright-ops-640.jsonl`: depthwise convolutions accounted for
55.9% of measured execution, internal resize operations 16.6%, and matrix
multiplication 16.4%. These are the next kernel/backend optimization targets.
An OpenCV CPU prototype at 320, 384, and 448 pixels reduced computation but
only accepted 32 of 56 variants, losing the blurry-frame detections retained
at 640 pixels (44/56). Smaller inputs were therefore not adopted. The bundled
Tract graph also has fixed intermediate shape annotations; smaller inputs
would require re-exporting/inference-shape changes.

The final release build and repository checks pass: 164 tests, 9 opt-in tests
ignored, formatting, Clippy, documentation, and schema agreement. Logs are
`artifacts/rotation-performance-checks.log` and
`artifacts/rotation-performance-build.log`.

```sh
cargo run --offline --release -p greign-film --example upright_profile -- \
  exports/portra-investigation/roll-tools/IMG_0007-individual.jpg \
  artifacts/perf/upright-ops-640.jsonl
cargo run --offline --release -p greign-film --example upright_check -- \
  exports/portra-investigation/roll-tools /tmp/upright-serial --verify-portra 1
cargo run --offline --release -p greign-film --example upright_check -- \
  exports/portra-investigation/roll-tools /tmp/upright-parallel --verify-portra 4
```

## Optional CUDA auto upright

Auto upright now uses an optional ONNX Runtime 1.22 CUDA backend on Linux
x86-64. The bundled YuNet model, 640-pixel inputs, and Rust decoder are shared
with the CPU path. CUDA runs in FP32 with TF32 disabled. A local UV installer
provides the native runtime and CUDA/cuDNN libraries under `.greign`; Python
is not involved in app inference. Sessions persist between batches, and
startup warms the model in the background. Missing libraries, unavailable
devices, or inference errors fall back to CPU. The completion status reports
the actual backend.

On the RTX 5070, all 56 rotation/mirror variants matched the CPU decisions and
face counts: 44 correct accepted orientations, 12 abstentions, no incorrect
accepted orientations. The largest confidence difference was 7.2e-7.
Detector-only median latency was **12.2 ms/photo**, including preprocessing,
four orientations, output transfers, and decoding. The native app measured
**22.2 ms/photo** including positive preview preparation from cached scans,
versus **111.2 ms/photo** during the CPU fallback replay (about 5× faster).
RAW loading remains additional. Native background warmup took 747 ms in
this run; the very first CUDA prototype initialization took roughly 25 seconds
with cold kernel caches, so cold-start latency must not be confused with
steady-state throughput.

The saved ONNX Runtime profile contains 23,850 CUDA node events, including
11,925 convolution events, with no CPU kernel events. The earlier CPU
convolution bottleneck now runs on the GPU. Raw profiles are in
`artifacts/perf/upright-native-cuda*.json`, per-variant measurements in
`exports/portra-investigation/upright-cuda/upright.jsonl`, and native timings
in `.greign/timings.jsonl`. Native replay confirms CUDA and a deliberately
unavailable GPU produce identical frame state; Undo restores the starting
state and Redo restores the CUDA result. Crops, mirrors, and adjustments remain
unchanged. Screenshots and state snapshots are `ui-cuda`, `ui-cuda-undo`,
`ui-cuda-redo`, and `ui-fallback` in the same export directory.

All repository checks pass: 166 tests, 9 opt-in tests ignored, formatting,
Clippy, documentation, and schema agreement. The film engine passes the
Rust 1.85 check, the release app builds, and rerunning the installer is
idempotent. Logs: `artifacts/upright-cuda-repository-checks.log`,
`artifacts/upright-cuda-msrv.log`, `artifacts/upright-gpu-release.log`,
`artifacts/upright-cuda-ui.log`, and `artifacts/upright-fallback-ui.log`.

```sh
uv run --python 3.12 scripts/setup-upright-gpu.py
cargo build --release -p greign-film --example upright_check
uv run --python 3.12 scripts/run-upright-gpu-check.py \
  exports/portra-investigation/roll-tools \
  exports/portra-investigation/upright-cuda --verify-portra gpu
```
