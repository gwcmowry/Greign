# Neutral picker, histogram, and original film dates

Restart `./target/release/greign-app` to use the new controls.

## Neutral-color picker

Under Color balance, choose **Pick neutral gray / white…**, then click the
positive. Esc cancels. It updates only warmth and tint; analysis, film base,
exposure, contrast, mixer and geometry are untouched. Whole-roll scope changes
the shared balance while preserving frame offsets; frame scope subtracts the
roll's contribution to obtain the correct frame offset. Activating the picker
in Selected scope switches to the active frame, rather than unexpectedly
applying one frame's sampled color to every selected image.

The worker samples a 5×5 patch in the cached scan preview, uses channel medians
in the converted linear working image, and solves the same exponential channel
gains used by the renderer. This is preview-resolution sampling, not a single
full-resolution pixel picker; choose a reasonably large clean patch. B&W,
clipped or very dark patches are rejected, as are clicks overlapping a manual
dust repair. The ±50 balance-control limits still apply; a limited correction
reports that it could not reach the requested neutral. Undo is supported.
Results from an older edit/frame/scope are discarded instead of overwriting
newer work. Sampling runs on a worker thread, not during UI painting.

## Histogram

The clipping overlay was removed at the user's request. An always-visible RGB
histogram is now at the top of Adjust, above lighting and exposure controls.
Its three curves show 256 display-code bins, using one shared vertical scale.
Shadows are on the left and highlights on the right.

It represents the active frame at cached preview resolution, including current
edits and crop; saved-version/proposed-analysis comparisons use their displayed
settings. Original-scan view uses that displayed scan preview, not unprocessed
sensor values. It is not a full-resolution RAW clipping audit or a DNG-headroom
measurement, and it describes the whole crop rather than only the zoomed area.

There is at most one histogram job in flight. Slider dragging pauses requests;
the histogram updates after release, and stale results are discarded. All
rendering/binning occurs on a worker, not the UI thread. A release-build CPU
check on the 960-pixel R7 preview averaged 14.61 ms for render/conversion/binning
(10 samples, 12.46–19.21 ms). Raw timings: `artifacts/histogram-cpu-profile.csv`.
The photograph itself is never overpainted or altered by this diagnostic.

## Date photographed → capture metadata

Previously the roll date existed only in Greign's saved library/recipe. New
exports embed it directly in the image:

- `EXIF DateTimeOriginal`: the original film capture timestamp.
- `Image DateTime`: the same value for compatibility.
- No scanning timestamp is substituted, and no timezone/GPS is inferred.
- Filesystem creation/modification times remain export times.

Accepted input is `YYYY-MM-DD`, optionally followed by ` HH:MM:SS`. Calendar
dates, leap years and clock ranges are validated. Date-only input uses
**12:00:00 as a placeholder**, clearly shown in the export dialog; it does not
claim knowledge of the actual shooting time. Empty input omits these tags.
Invalid nonempty input must be corrected before export. Saved retry recipes
retain the original roll-date value from the export that failed.

Every frame receives the same roll timestamp. This supports chronological
placement of a roll, but does not guarantee ordering among its frames when
they have identical timestamps. No per-frame dates or artificial staggered
times were added. Existing exports and already-imported Photos items are not
modified; re-export to create files with the new metadata.

Apple Photos can sort by capture date or by when items were added; those are
different views. Use capture-date sorting for the film chronology.
[Apple's sorting guide](https://support.apple.com/en-ie/guide/iphone/iph2e66e2f2c/ios)
documents the distinction. The tag's original-capture meaning is documented in
[ExifTool's EXIF reference](https://exiftool.org/TagNames/EXIF.html).

Apple Photos itself cannot be validated on this Linux machine. Before importing
a whole roll, inspect one newly exported JPEG in Photos' information panel.
The supplied `exports/capture-date-check/dated.jpg` should report July 3, 1998,
at noon, subject to the receiving application's timezone interpretation.

## Validation

47 routine CPU tests pass. New coverage includes neutral correction math,
clipped-sample rejection, scope separation, undo, stale-result rejection,
histogram bin counts, crop/stale-result handling and date/calendar validation. Existing JPEG/TIFF export tests
also check the roll timestamp reaches the file.

The headless native-widget test verifies that the histogram heading is above
Exposure and that the old overlay control is absent. Overlay shader code has
also been removed. The old clipping GPU logs document the previous feature,
not current behavior. Current checks: `artifacts/histogram-tests.log`,
`artifacts/histogram-clippy.log`, and `artifacts/histogram-build.log`.

An independent reader, ExifRead, confirms DateTimeOriginal and Image DateTime
in all three dated fixtures without an invented OffsetTimeOriginal:

```sh
cargo run --offline --release -p greign-export --example date_check
uv run --python 3.12 --no-project --with exifread scripts/check_capture_date.py
```

Files remain viewable under `exports/capture-date-check/`; reader output is in
`artifacts/capture-date-independent.log`. Tests, Clippy and release build logs
are `artifacts/tools-date-tests.log`, `artifacts/tools-date-clippy.log` and
`artifacts/tools-date-build.log`. Native hit-target/layout checks and an Apple
Photos import are useful hands-on follow-ups; these tests do not establish
automatic film-color accuracy on real scans.
