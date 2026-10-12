# Roll workflow and creative controls

Launch the rebuilt native app with `./target/release/greign-app`.
This is a desktop window, not a browser app. Originals remain unchanged.

## Selection and edit scope

Click a contact-sheet image to make it active. Ctrl-click toggles membership;
Shift-click adds the range from the active image. Select all / Clear selection
are above the contact sheet. Double-click opens the active frame.

In Adjust, choose Whole roll, This frame, or Selected. Roll settings and frame
offsets remain separate. In Selected mode the sliders show the active frame's
offsets; moving a slider adds that change to every selected frame, preserving
their existing differences. Reset explicitly zeroes adjustments in the chosen
scope. Composition tools still apply only to the active frame.

Copy / paste frame corrections copies frame offsets, not roll settings.
Choose lighting, color, composition, and/or manual dust groups. Paste replaces
the selected groups on the destination frame(s); it does not paste rejection,
review flags, or identity. Crop and dust are excluded by default. Only copy
dust between scans with matching alignment; it is never automatically applied.

Ctrl+C copies the active frame's corrections to the system clipboard. Ctrl+V
applies the enabled paste groups to multiple selected frames, or the active
frame otherwise. Roll settings remain unchanged. Text and numeric entry fields
keep normal text copy/paste; unrelated clipboard text is ignored elsewhere.

## Lighting and color

**Pick neutral gray / white…** in Color balance activates a one-click tool in
the positive view. It adjusts warmth/tint in Whole roll or This frame scope;
Selected scope switches to This frame when the tool is activated. Undo restores
the previous values. Use an unclipped, genuinely neutral patch outside a dust
repair, not the orange film base. See [diagnostics and capture dates](diagnostics-and-dates.md).

An **RGB histogram** sits at the top of Adjust, above the lighting controls.
It reflects the active frame's displayed preview and crop, with shadows on
the left and highlights on the right. It updates after slider release on a
worker thread. There is no clipping overlay on the photograph.

- Exposure and Contrast retain the existing behavior.
- Highlights and Shadows apply broad, smooth exposure changes in working
  luminance. Zero bypasses the new processing; equal channel scaling preserves
  chromaticity before output gamut clipping. These are tonal controls, not
  Lightroom's proprietary algorithm or reconstruction of clipped scan data.
- Color balance retains Warmth, Tint, and overall Saturation.
- Color mixer has Red, Orange, Yellow, Green, Aqua, Blue, Purple, and Magenta.
  Opening the mixer exposes all eight groups with colored headings; there are
  no individual color dropdowns.
  Each band has Hue (degrees), Saturation, and Brightness. Neighboring bands
  blend smoothly, including the wrap around red; nearly neutral pixels are
  protected from unstable hue shifts. Hue/saturation preserve working
  luminance; Brightness deliberately changes it. The mixer uses working-RGB
  hue, not a perceptually uniform or spatially masked selection. A blue
  adjustment can affect both a blue shirt and the sky. Extreme adjustments
  can still exceed the output sRGB gamut.

All controls use the same renderer for previews and final JPEG/TIFF output.
Old libraries load new controls at zero. No dust-review mode was added.

## Calibration and versions

Conversion walks through inspecting the original for clipping, measuring a
clean film-base patch, setting the analysis-area inset, and analyzing. The
original view shows the uncropped, unrotated scan so a previous creative crop
cannot hide the film border. Analysis area is independent of export crop.

The first successful negative analysis creates an Initial conversion version.
Existing converted libraries receive an Opening look on load; positive rolls
receive an initial version after loading. Subsequent analysis is a proposal:
preview it, accept it, or discard it. Accept saves the previous look as a
version. Adding scans never clears an accepted calibration.

Lock accepted conversion to prevent replacement or calibration-input changes.
You can still generate and preview a proposal, but must unlock before accepting
it or restoring a complete version. Exports are unavailable while viewing a
proposal or holding a saved-version comparison.

Saved versions / before & after stores named, complete roll-edit snapshots.
Select one, then hold the compare button or backslash (`\`) outside a text
field. Release to return to current edits. Restore creates a safety version
first and is undoable. Frames added after a version was saved remain present;
matching frames regain their saved corrections. Metadata and export presets
are not changed by restoring a look. Versions persist across restarts; the
short-term Undo/Redo stack does not.

## Review and export

The export dialog now offers Whole roll, Selected frames, and Active frame.
Single-frame view also has an Export frame button. Linear DNG is available
alongside JPEG/TIFF; see [DNG handoff](dng-handoff.md). DNG always keeps full
cropped resolution and bypasses the JPEG/TIFF size limit.

TIFF uses lossless ZIP/Deflate compression with a horizontal predictor,
retaining 16-bit RGB and the embedded sRGB profile. Existing exports are not
rewritten. Full-resolution files can still be large: 32.3 million RGB16 pixels
occupy about 194 MB before compression, and grain/detail limits the savings.
DNG compression is unchanged. See [TIFF compression measurements](tiff-compression.md).

Exports automatically use GPU finishing when available, with CPU fallback for
manual healing or GPU errors. Encoding overlaps preparation of the next frame,
including in CPU-only mode. See [GPU export measurements](gpu-export.md).

Ctrl+E opens export settings outside text/numeric entry fields. JPEG quality
is saved with each preset and frozen into an export/retry recipe; older presets
retain quality 95. **Full JPEG** selects full resolution at quality 95;
**Share JPEG** explicitly selects a 2048-pixel longest edge at quality 90.
Quality 100 is still lossy and is not the default. The status bar reports
completed work, output bytes and elapsed time as files finish, then shows an
export summary beside the existing **Open export folder** button.
See [JPEG optimization and validation](jpeg-export.md).

**Date photographed** in roll metadata is now embedded as EXIF DateTimeOriginal
in every newly exported JPEG, TIFF and DNG. Enter `YYYY-MM-DD` or
`YYYY-MM-DD HH:MM:SS`; date-only uses noon with no invented time zone. All frames
share this roll timestamp. Invalid nonempty dates block export; blank dates
omit capture-date tags. Existing exports are not rewritten.

Needs my review is a manual flag, separate from rejection. Scan-warning badges
report possible clipping; roll-analysis warnings are in Conversion. These are
diagnostics, not judgments of whether a scene's colors are correct.

Export remembers format, destination, longest edge (0 means full resolution),
and filename prefix per roll. Full TIFF and Web JPEG are quick choices;
named presets can also be saved per roll. Output is never enlarged by the
size setting. Only letters, numbers, dash and underscore are used from the
prefix. Each export gets a new folder and recipe under `exports/film-rolls/`
unless another destination is selected.

Failures are reported per frame. Retry failed frames only reuses the frozen
roll recipe and settings from that export, leaving successful files alone.
Fix missing source files or destination problems first. Retry state currently
lasts for the running session; export presets and edit recipes persist.

## Validation and CPU timing (2026-09-07)

19 headless tests passed, including native egui pointer input for the selection
button, panel construction, selection offsets, lock/accept, backward-compatible
loading, saved-state restoration, and failed-only export retry. This does not
replace hands-on validation of every control or native file dialog.

Raw stage samples are in `artifacts/film-controls-profile.csv`. Reproduce with:

```sh
cargo run --offline --release -p greign-film --example controls_check > artifacts/film-controls-profile.csv
```

This CPU-only benchmark uses a simulated negative derived from the existing
152A0123 positive, not a real negative RAW. Seven warmed render samples per
configuration gave these means:

| Stage | 960 × 640 | 3840 × 2560 |
| --- | ---: | ---: |
| Load, resize, synthesize negative (one sample) | 186.6 ms | 738.7 ms |
| Analyze (one sample) | 3.2 ms | 14.8 ms |
| Render, neutral new controls | 11.1 ms | 260.8 ms |
| Render, lighting + mixer active | 12.6 ms | 281.5 ms |

Initial loading/synthesis is the largest measured stage. During cached edits,
rendering dominates; the new controls add about 1.5 ms at preview size and
21 ms at the larger size in this test. PNG conversion/encoding added roughly
5 ms and 91–97 ms respectively. These are stage timings, not a flamegraph or
measurements of full-resolution Canon RAW decoding or native frame latency.

Viewable comparisons are saved under `exports/film-controls/`: baseline,
lighting, mixer, and combined at both sizes. Lighting uses Shadows +60 and
Highlights -60; the mixer stress test shifts all eight bands +15 degrees,
Saturation +20 and Brightness +5. They demonstrate control effects, not a
recommended automatic look. Actual film-color validation still needs scans.
