# Native UI validation

The updated native release build succeeds. The earlier window and contact
sheet were visually inspected on Linux; the September 7 changes use headless
validation only. Desktop-driven input automation
was unreliable, so full interaction acceptance is **not yet verified**.
Do not treat saved smoke-test screenshots as proof that all controls work.

## Automated checks (no desktop required)

```sh
cargo test --offline -p greign-app -p greign-film -p greign-export -p greign-gl-preview
cargo clippy --offline -p greign-app -p greign-film -p greign-export -p greign-gl-preview --all-targets -- -D warnings
```

All 47 routine tests pass (including the export and GL-adapter crates); the separate opt-in
interactive performance test also passes. They cover roll/frame adjustment
independence, save/reload and backup, undo/redo state, stale-preview rejection,
empty-library actions, crop/rotation coordinate transforms, explicit local
healing, inversion behavior, film-base sample rejection, and background
analysis followed by JPEG and 16-bit TIFF export. The export integration test
checks source preservation and skipping rejected frames.

New tests cover relative selection edits, selective paste, old-library
defaults, versions and comparison rendering without mutation, locked analysis
acceptance, tonal monotonicity, selective color adjustment, resized exports,
and failed-only retries. One native selection button is clicked through
egui's headless pointer-event input, and all three panels and the export
dialog are constructed without opening a desktop window.
The DNG update adds directory/float-sample checks and selected-only DNG export;
see [DNG interoperability validation](../dng-handoff.md) for the independent
darktable check and remaining Lightroom validation.
The [interactive-preview update](../interactive-preview.md) adds native
Ctrl/Command-wheel input, cursor anchoring, cached-render equivalence,
cancellation, separate-resolution lanes, and unchanged-neighbor checks.
The [GPU update](../gpu-preview.md) additionally validates source-cache reuse,
actual RTX 5070 numerical parity, and native GPU display. Hardware checks are
separate opt-in commands, never implicitly run by the CPU test suite.

The latest polish tests check exact lossless TIFF ZIP decoding, opening all
eight mixer groups, and native egui clipboard events: selected-frame paste,
undo, excluded crop, unchanged roll settings, and ordinary text-field paste.
Release build, tests, and Clippy logs are in `artifacts/polish-build.log`,
`artifacts/polish-tests.log`, and `artifacts/polish-clippy.log`.

The [GPU export update](../gpu-export.md) adds float export crop dimensions,
GPU-failure fallback and explicit-healing fallback tests. Its opt-in hardware
check exercises actual production export jobs, RGB16 TIFF parity and float DNG
strip preservation, with viewable outputs under `exports/gpu-export/`.

The [JPEG update](../jpeg-export.md) adds legacy/default quality and preset
round-trip coverage, native export shortcut dispatch, and full/resized JPEG
requests, GPU-error fallback, saved quality and completed-file progress. The
JPEG hardware check compares full-resolution and 2048-pixel exports with CPU
references, plus pre-codec color/crop/rotation fixtures.

The diagnostics/date update adds neutral-solver, scope/undo/stale-result and
calendar-validation tests. The clipping overlay was subsequently replaced by
an RGB histogram; its tests check bin counts, crop handling, stale-result
rejection and placement above Exposure. Existing JPEG/TIFF integration checks
date propagation, and independent ExifRead checks confirm DateTimeOriginal in
all three output formats. Apple Photos import is still a user-side check.

These tests do not verify native file dialogs, all pointer hit targets, display
scaling, visual responsiveness, or the perceived quality of real film color.
Clippy passes for the app and film crates; the vendored Rawler dependency
still emits pre-existing lifetime warnings. Logs are in
`artifacts/workflow-tests.log`, `artifacts/workflow-clippy.log`, and
`artifacts/workflow-native-build.log`.

## Hands-on acceptance checklist

Launch `./target/release/greign-app` from the repository, or use
`./scripts/launch-greign.sh` to rebuild first. The following are pending manual
checks, not claims of completed validation:

- Open the demo roll and run **Analyze & convert roll**. Check that the
  contact sheet fits the window and every frame finishes loading.
- Change warmth under **Whole roll**, then under **This frame**. Verify the
  first change affects every frame and the second only the selected frame.
  Check Undo and Redo after each.
- Ctrl/Shift-select several frames, choose **Selected**, and try Shadows
  and a color-mixer band. Verify that existing differences remain and that
  unselected frames do not change. Try selective copy/paste without crop.
- Open Color mixer and check its eight colored headings and inline controls.
  Copy with Ctrl+C, select destination frames, and paste with Ctrl+V. Check
  normal text copy/paste in a metadata field as well.
- Save a named version, edit, then hold backslash to compare and release to
  return. Re-analyze an existing conversion; preview/discard the proposal,
  then test accepting one after unlocking. Restart to check versions persist.
- Open **Single frame**, crop and rotate, then undo. Confirm the image and
  crop behave as expected. Try resizing the window and **100% detail**.
- Ctrl-scroll over a recognizable feature and verify it stays under the mouse.
  Pan, drag a lighting slider, and check that the draft stays responsive and
  the full refinement returns without changing zoom. Repeat after 100% detail
  and after returning to Fit; no full-resolution scan should pollute draft work.
- In **Dust correction**, click a small spot, move its source, remove it,
  and undo. Check that only marked areas change, including after a crop or
  rotation. No automatic dust detection/removal is implemented.
- Reject one frame and export the roll as JPEG and TIFF. Use the default
  folder under `exports/film-rolls/`; confirm the rejected frame is absent,
  exported images open correctly, and your edits appear in the files.
- Save an export preset with a size limit and filename prefix, restart, and
  reuse it. Mark a frame **Needs my review** without rejecting it.
- Close and reopen the app. Verify metadata, adjustments, crop, rejection,
  and manual spots persist. Separately test importing files with the native
  file chooser; choose **Positive** for ordinary photographs, not negatives.

For a problem report, include the control name, what you expected, what
happened, and a screenshot if useful. There is no need to test everything
at once; adjustment scope and dust correction are good first priorities.

## Color-validation limitation

The demo uses simulated negatives derived from positive photographs. It can
exercise editing and export, but cannot establish reliable film-base
estimation or color across real stocks, exposures, and scanning conditions.
Those checks remain pending the arrival of actual camera scans.
