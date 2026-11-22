# Autocrop regression: 261104 roll 001

Input: `/media/grant/T7 Shield/261104_roll_001/2026_11_05`, containing 28 CR3
captures plus AppleDouble sidecars. Originals were read only; diagnostic
previews, cached linear samples, and native test state stay in the workspace.

The previous detector declined all 28 captures. Its per-side scanlines required
a bright film-base plateau followed by image density. These captures place the
picture against a narrow dark surround, particularly on the left and right;
there is no reliable broad plateau on those sides. Clipped perforation tips
also interfere with the narrow top/bottom borders.

The revised detector retains the established film-base method and adds a
transition fit for this framing. It estimates straight-edge slope from the
first dark-to-light transition and the inner offset from the local image
plateau. Boundary variation and interior contrast reject blank film and empty
openings. Nearby film-base edges refine the result when visible, and a short
inward search removes clipped perforation tips. Four compatible edges are
still required, and the existing inward margin and shared CPU/GPU crop geometry
are unchanged. A much smaller, spurious rectangle formed by scene features is
rejected when supported outer picture geometry is available.

## Results

- All 26 photographs now receive picture crops. The two blank captures,
  `IMG_0014.CR3` and `IMG_0029.CR3`, remain unchanged.
- No clipped perforation pixels were found in 52,000 samples along the 26 crop
  perimeters. All boundaries were also visually reviewed on diagnostic overlays;
  the difficult perforated captures were inspected as cropped positives.
- The seven earlier Portra validation crops are exactly unchanged.
- Native **Autocrop all frames** produced the same 26 crop rectangles. Undo
  restored the complete pre-crop state and Redo restored the cropped state.
  Quarter turns, mirroring, creative edits, accepted calibration, metadata, and
  review/rejection flags were preserved.
- 174 repository tests pass, with 9 opt-in tests ignored. Formatting, Clippy,
  documentation, schema agreement, and the native release build pass.

New synthetic tests cover color and monochrome pictures with narrow borders,
positive and negative tilt, clipped perforations, blank illumination gradients,
wide film-base borders, and a clipped strip preceded by a short clear gap.

The accepted conversion in an existing library is intentionally preserved.
New analysis uses the detected picture instead of including the dark surround;
**Re-analyze & convert** allows the user to preview and accept that updated
conversion independently of cropping.

## Timing and reproduction

Twenty detector repetitions per capture measured a median **6.7 ms per image**
(maximum per-image average 8.4 ms), using cached linear previews on the Ryzen
7900X. Baseline RAW preview loading measured a 332.5 ms median, making loading
the dominant cost when previews are not already cached. RAW loading, rendering,
and disk writes are excluded from detector timings. `cargo-flamegraph` was
attempted first, but the kernel's perf-event permissions prevented sampling;
its failure log is preserved and no stack-profile result is claimed.

```sh
cargo build --release -p greign-film --example crop_check
./target/release/examples/crop_check \
  '/media/grant/T7 Shield/261104_roll_001/2026_11_05' \
  exports/autocrop-261104/final artifacts/autocrop-261104-cache
```

Use a separate cache directory for each source set. A fourth argument specifies
repetitions and suppresses image output for timing/profiling.

Evidence:

- `exports/autocrop-261104/final/boundaries.jpg`, individual boundary overlays,
  and cropped positive previews. Blank captures have no green outline.
- `exports/autocrop-261104/native/{before,cropped,undo,redo}.png` and JSON state
  snapshots from an isolated copy of the user's roll.
- `artifacts/autocrop-261104-{baseline,final,timing}.jsonl`,
  `autocrop-261104-verification.json`, and the comparison
  `autocrop-portra-{baseline,final}.jsonl`.
- `artifacts/autocrop-final-build.log`, `autocrop-repository-final-checks.log`,
  `autocrop-tight-tests.log`, `autocrop-261104-ui.log`, and
  `autocrop-flamegraph.log`.
