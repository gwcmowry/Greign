# Roll workflow and creative controls

Launch the rebuilt native app with `./target/release/greign-app`.
This is a desktop window, not a browser app. Originals remain unchanged.

## Folder import and a collection of converted rolls

Click **Import a roll → Choose roll folder…**, select the folder itself, then
click **Import roll**. Greign uses its folder name as the roll name and selects
the supported image files directly inside it. Subfolders remain separate rolls;
non-image files and AppleDouble sidecars are ignored. You can change the name,
film stock, or material before importing. **Choose individual files…** remains
available, and **Add folder…** adds scans to the current roll. Canceling the
picker creates no empty roll. The roll list scrolls as the collection grows.

Set up exports once with **Export settings…**:

1. Choose a **Master export folder**, such as `Pictures/Converted rolls`.
2. Keep the default **Roll folder suffix**, ` - converted`, or enter your own.
3. Pick JPEG, TIFF, or another option, then click **Use these settings for every
   roll**. This saves the settings for existing and future rolls.

After that, **Export roll** exports the current roll in one click. For example,
an imported `Kyoto 2026` roll goes to
`Pictures/Converted rolls/Kyoto 2026 - converted/`. Each folder contains the
images, roll recipe, and export selection. Repeating an export creates
`Kyoto 2026 - converted (2)`; earlier outputs are preserved. Duplicate roll
names also receive numbered folders. Unsupported path characters in names and
suffixes are replaced with underscores.

Use **Export ready rolls…** below the roll list to export every ready roll with
the library defaults. The dialog lists included rolls and why others are
skipped. A roll is ready when it has kept frames, a valid or blank capture date,
and accepted conversion for every kept negative frame. Positive rolls do not
need conversion. Review flags do not exclude frames; rejection does. This
does not analyze or accept conversions automatically.

The batch freezes edits and settings when it starts, then exports rolls
sequentially. Progress shows the roll number and current frame count. A failed
frame or destination does not stop later rolls. Frame failures across rolls
can be retried together using their original settings and output directories.
Destination-creation errors are reported with the roll name; correct the
destination and export that roll again. Retry state lasts for the app session.

## Selection and edit scope

Click a contact-sheet image to make it active. Ctrl-click toggles membership;
Shift-click adds the range from the active image. Select all / Clear selection
are above the contact sheet. Double-click opens the active frame.

Press **R** to rotate clockwise, or **Shift+R** to rotate counterclockwise.
On the contact sheet this rotates the selected frames together; with no
selection, it rotates the active frame. Single frame view always rotates the
active frame. The visible direction stays consistent for mirrored photos.
Each press is one Undo step. **Rotate selected · R** is also above the contact
sheet. Text/numeric entry retains its keyboard input; holding R does not repeat
rotations. **X** now toggles keep/reject, replacing its former R shortcut.

In Adjust, choose Whole roll, This frame, or Selected. Roll settings and frame
offsets remain separate. In Selected mode the sliders show the active frame's
offsets; moving a slider adds that change to every selected frame, preserving
their existing differences. Reset explicitly zeroes adjustments in the chosen
scope. Rotation follows contact-sheet selection; manual crop and dust tools
apply to the active frame.

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

The **Conversion** mode is visible above the preview button:

- **Optimize frames individually** estimates a separate film base and density
  range for every kept frame and is the default for new rolls. Use it when capture exposures vary, or when you
  prefer independent automatic conversions. Roll and frame creative adjustments
  remain available afterward.
- **Shared roll calibration** uses one film base and density range for the roll.
  Use it with consistent camera exposure and backlight settings. Existing saved
  choices and older shared-calibration libraries retain their mode.

For faded color negatives, open **Conversion** and click **Preview faded-film
restoration**. This sets individual optimization, 100% faded-film color balance,
and uneven film-base lighting correction, then builds a preview. The result is
shown automatically; it is not applied to the saved conversion yet.

For other settings, choose a mode and adjust the controls, then click **Build
new preview** (or **Build conversion preview** for an unconverted roll). The
review card above the images clearly identifies **NEW PREVIEW · not applied**
or **CURRENT CONVERSION**. Switch between **Current conversion** and **New
preview** to compare. With no applied conversion, the first choice shows the
original scan.

Individual previews start with neutral lighting and color. Check **Keep my
lighting and color edits** to include existing exposure, contrast, white balance,
saturation, highlights/shadows, and color-mixer adjustments. **Apply conversion**
commits the shown proposal and the chosen edit behavior in one Undo step. Crops,
rotation, mirroring, healing, and review/rejection flags are preserved. The prior
look is saved as a version. Discarding a preview leaves the applied conversion
intact. Export is disabled while viewing a new preview; apply it to export that
look, or switch to Current conversion to export the existing result.

Changing conversion settings after building a preview marks it **Settings
changed**. Click **Update preview**; Apply is disabled until the preview matches
those settings. This check includes the mode, optional corrections, analysis
area, shared film-base selection, and requested frames. Creative edits do not
invalidate calibration. New input metadata is optional when loading older
recipes. Conversion locks continue to protect both settings and application of
a preview.

The side panel places the main controls first. Faded-film color balance is shown
as **0–100%**, with an explicit Off label at zero. **Selected frame** reports
whether lighting correction was applied to that frame. **Across this roll**
shows the corrected/analyzed count, names frames whose lighting was unchanged,
and groups warnings by frame. A warning from another frame is not presented as
the selected frame's result. Original-scan inspection, manual base selection,
analysis inset, and restoration of the previous conversion live under
**Advanced calibration**.

In individual mode, **Frame film base** at the top of Conversion provides an
override for the active frame:

1. Select the problem frame and click **Sample frame film base**.
2. Greign opens its full, uncropped original. Drag a small rectangle over clean,
   unexposed film outside the picture, avoiding perforations and lettering.
3. A preview is built automatically for that frame. Compare it with Current
   conversion, then click **Apply to this frame**.

This affects only the active frame, even with multiple frames selected. **Keep
this frame's lighting and color edits** is on by default. Unchecking it resets
only this frame's combined lighting/color, compensating its frame offsets for
any roll-level adjustments. Other frames retain both their conversion and
creative edits. **Preview this frame** can also rebuild one frame using the
current conversion settings without changing the other applied conversions.
Other frames can still retain earlier roll settings until a whole-roll preview
is applied.

**Use automatic for this frame** removes its override and previews the automatic
estimate. Explicit frame bases are saved in the library and export recipes,
honored by later whole-roll analysis, and restored by Undo and named versions.
Shared roll mode uses the shared base from Advanced calibration; stored frame
overrides remain available when returning to individual mode. Creative edit
copy/paste does not transfer a film-base calibration between frames.

Analysis version 6 makes an explicit base a color reference: independent
per-channel black-point subtraction no longer cancels the chosen base. It uses
one common density offset after channel balancing and a shared print range that
contains the restored channel highlights. Existing applied analyses keep their
stored appearance until a new preview is applied. A base sample calibrates the
film/capture; scene lighting can still need the neutral picker or color controls.

Calibrations are reused by previews, histograms, JPEG/TIFF/DNG exports, and export
retries. New frames need their own analysis in individual mode. Locks, versions,
and Undo/Redo retain settings and calibrations.

Uneven-lighting correction fits clean unexposed border patches and declines
insufficient or inconsistent evidence. Shared mode also requires consistent
fields across the roll. Faded-film color balance blends toward separate channel
density ranges; a scene dominated by one color can mislead the estimate, so use
less when needed. These are estimates rather than a measured film-stock profile.
See the [faded-roll investigation](quality/faded-roll-261114.md).

**Mirror all frames** is in the **Whole roll** toolbar above the contact sheet
and Single frame view. It sets mirroring consistently across every frame while
preserving each frame's own rotation and crop. A mixed checkbox means some
frames are mirrored; clicking it mirrors them all. Clicking a checked box clears
mirroring for the roll. Each change is one Undo step.

**Auto upright**, in the same toolbar, checks converted positive previews using
a bundled face-and-landmark model. It compares 0°, 90°, 180°, and 270° and sets
quarter-turn rotation when the face evidence clearly favors one view. For a
weaker match, it checks whether the same face is detectable after mirroring.
Unresolved frames get a second check at a smaller scale for close-up faces.
If two distinct faces each favor the same turn at both scales, Greign can use
their agreement to settle a tie. When two leading turns disagree about one
face, Greign checks that face in a crop from the original preview.
A bundled scene classifier checks remaining frames. Each of four rotated views must
predict the same source correction with a softmax score of at least 0.90. Both
models run locally.
An optional NVIDIA CUDA backend is available on Linux; the completion message
and timing log identify **CUDA**, **CPU**, or **CPU fallback**. GPU initialization
and inference failures fall back to the CPU path.
Analyze negative scans first. Photos with unresolved orientation
keep their rotation and receive **Needs my review**. Existing review flags are
preserved. The entire operation, including new review flags, is one Undo step;
crop, fine straightening, mirroring, and accepted calibration are preserved.
A suggested rotation can always be adjusted manually.

Install the optional GPU runtime once from the repository:

```sh
uv run --python 3.12 scripts/setup-upright-gpu.py
```

Restart Greign afterward. CUDA/cuDNN and ONNX Runtime are installed under
`.greign/upright-gpu/`, with pinned package versions and local license notices.
Python/UV are used for installation; actual inference is native Rust/CUDA.
The app warms the GPU model in the background during startup and reuses its
session across batches. First-ever kernel setup can take tens of seconds;
subsequent starts are much quicker. After warmup, the native detector measured
about 12 ms per picture on the RTX 5070, with identical orientation decisions
to CPU in the 56-case check. RAW loading and preview preparation are additional.
Set `GREIGN_CPU_UPRIGHT=1` before launching to force CPU inference. Other
platforms continue using CPU inference without the optional runtime.

**Invert orientation** next to Crop in Single frame changes only that frame. It mirrors the positive
left-to-right after rotation, correcting scans taken from the opposite side
of the film. Click again to turn it off. Under **Adjust → Composition**, use
**Apply this orientation to whole roll** to copy the active frame's rotation
and mirror setting to all frames. Crop and dust locations stay attached to
the same source pixels. Original-negative inspection and crop/base selection
still show the scan as captured. Mirroring is included in exports, saved
versions, Undo/Redo, and the composition copy/paste group.

Conversion walks through inspecting the original for clipping, measuring a
clean film-base patch, setting the analysis-area inset, and analyzing. The
original view shows the uncropped, unrotated scan so a previous creative crop
cannot hide the film border. Analysis area is independent of export crop.

The original RAW preview applies the camera's as-shot white balance for display.
Film-base measurements and inversion still use the untouched linear camera
channels. This fixes the green original-negative preview on the R7 scans.

In Single frame, **Crop · C**, **Auto crop**, and **Reset crop** are above the
image. Press **C** or click Crop, then drag a rectangle on the full, unrotated
original; release to apply, or press Escape to cancel. The current crop is
outlined, so it can be expanded as well as tightened. Crop and rotation are
saved with the frame and support Undo/Redo.

**Auto crop** finds the inner picture on all four sides of the active scan,
removes the film-base border, and straightens tilt up to 5°. It fits many edge
samples and adds a small inward margin past the soft transition. Preview and
all export formats use the same crop and angle. **Reset crop** also resets this
straightening; quarter-turn rotation and mirroring remain independent. If the
four boundaries are uncertain or incomplete, Auto crop leaves the existing crop
intact and suggests a manual crop. Original inspection shows the whole scan;
the Crop tool outlines the tilted rectangle. Existing libraries load with zero
straightening, and manual crop starts a new, axis-aligned selection.

Tightly framed captures also work when there is no broad film-base strip on
every side. Detection follows the inner transition from the dark surround,
checks for picture detail, and excludes clipped perforation tips. Blank film
and empty openings remain unchanged. See the
[28-scan regression check](quality/autocrop-261104-roll-001.md).

**Autocrop all frames**, beside the mirror checkbox, applies the picture-frame
detector to every frame in the roll, including rejected frames. It runs in the
background and reuses loaded previews. The contact sheet stays open. Uncertain
boundaries, unreadable scans, and crops edited during detection remain unchanged
and are named in the completion message. The whole batch is one Undo step.

Autocrop targets the exposed picture boundary. Analysis version
5 uses that same tilted picture region for density estimates while estimating
film base from the uncropped capture. The old holder-crop detector has been
removed. The analysis inset can further restrict the picture region; effectively
opaque samples are excluded when no picture boundary can be found.
Narrow borders with unequal brightness use local color-consistent transitions
and the tilt supported by the other edges. All four picture sides must still be
supported; a photograph extending past the scan edge requires a manual crop.
Existing accepted conversions stay unchanged: choose **Build new preview**, compare the result, and apply it to apply the improved
analysis. Large differences in estimated film-base brightness now produce a
capture-consistency diagnostic; a shared calibration assumes consistent scan
exposure and lighting. See the [Portra scan investigation](quality/portra-scans-2026-11-01.md).

Applying the first negative conversion creates an Initial conversion version.
Existing converted libraries receive an Opening look on load; positive rolls
receive an initial version after loading. Subsequent analysis is a proposal:
preview it, accept it, or discard it. Accept saves the previous look as a
version. Adding scans never clears an accepted calibration.

Use **Lock applied conversion** to prevent replacement or calibration-input changes.
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

The per-roll export dialog offers Whole roll, Selected frames, and Active frame.
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

Full-resolution export means the source pixel dimensions after your crop; it
does not upscale the remaining picture. Width and height fractions multiply:
a crop retaining 66% of the width and 67% of the height retains about 44% of the
pixels. A 32.3 MP capture therefore yields about 14.3 MP at that crop.
More of the film frame must fill the camera sensor to retain more pixels.

Apple Shared Albums created with version 26 or earlier reduce ordinary photos
to 2048 pixels on the long edge. Version 27 supports full-resolution albums;
older albums need upgrading, and newly added originals then use full resolution
and count toward the owner's iCloud storage. The original Greign export remains
unchanged. See [Apple's limits](https://support.apple.com/en-us/148868) and
[album upgrade instructions](https://support.apple.com/en-us/108916).

**Date photographed** in roll metadata is now embedded as EXIF DateTimeOriginal
in every newly exported JPEG, TIFF and DNG. Enter `YYYY-MM-DD` or
`YYYY-MM-DD HH:MM:SS`; date-only uses noon with no invented time zone. All frames
share this roll timestamp. Invalid nonempty dates block export; blank dates
omit capture-date tags. Existing exports are not rewritten.

Needs my review is a manual flag, separate from rejection. Scan-warning badges
report possible clipping; roll-analysis warnings are in Conversion. These are
diagnostics, not judgments of whether a scene's colors are correct.

Export remembers format, destination, folder suffix, longest edge (0 means full
resolution), and filename prefix. Library defaults can be applied to every
roll, and named presets can still be saved per roll. Full TIFF and Share JPEG
are quick choices; **More export options** exposes DNG, JPEG quality, the size
limit, and filename prefix. Output is never enlarged by the size setting.
Only letters, numbers, dash and underscore are used from the filename prefix.
Each export gets a named folder and recipe under `exports/film-rolls/` unless
another master destination is selected.

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
