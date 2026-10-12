# Linear DNG handoff

In Single frame, choose **Export frame…**, or open the usual export dialog
and choose **Selected frames** / **Active frame**. Choose **DNG · linear
32-bit float**. Rejected frames are excluded in every scope. The default
destination remains `exports/film-rolls/`, with a new directory per export.
The recipe and `export-selection.json` record the settings and exported IDs.
Failed-only retries retain the chosen scope and frozen recipe.

## What this file contains

The edited **positive**, not a copy of the original negative camera RAW.
Greign's conversion, lighting/color edits, tone model, manual healing, crop,
and rotation are baked into its pixels. Lightroom/darktable can make further
edits, but cannot undo those operations or reconstruct Greign's sliders.
Keep the original scans and Greign library for that purpose.

Pixels are taken from the shared renderer before final sRGB conversion and
clipping. The encoding is linear-light Rec.2020 D65, with a matching DNG
color matrix and neutral white balance. Float values above one are stored
unchanged; negative working-RGB channels are floored at zero. This does not
restore detail already lost during scanning or Greign's conversion. It is
not an untouched, scene-linear sensor capture.

The writer uses DNG 1.4, `LinearRaw` photometric interpretation, 32-bit IEEE
float samples, uncompressed strips, zero black, nominal white of one,
orientation 1, full baked crop, and an identity profile tone curve. It does
not invent a Bayer pattern or reuse the original camera matrix. This uses
the float and linear-image provisions in Adobe's
[DNG 1.4 specification](https://www.kronometric.org/phot/processing/DNG/dng_spec_1.4.0.0.pdf).

Files are large: about 12 bytes per pixel plus metadata (roughly 390 MB for
32.5 megapixels before cropping). No embedded thumbnail, original RAW, camera
MakerNotes, or portable edit instructions are included in this first version.
The accompanying Greign recipe is not a Lightroom/darktable settings file.
Re-importing linear DNG into Greign is not yet a supported round-trip path.
Use 16-bit TIFF when you prefer a smaller, display-rendered handoff.

## Darktable

CPU-only import/render was verified with the installed **darktable 4.6.1**,
using an isolated configuration and OpenCL disabled. No desktop automation
or GPU access was involved.

For a neutral starting point, avoid automatically adding another filmic/base
curve/sigmoid treatment, and **disable highlight reconstruction**: this is
already a demosaiced positive. In our six-patch test, darktable's automatic
reconstruction tinted white; disabling it restored [1, 1, 1]. This is an
import-processing issue, not a tint in the stored DNG samples.

The test fixture `fixtures/synthetic/dng-neutral.xmp` disables reconstruction
for the command-line comparison. It is a validation fixture, not a sidecar
automatically imposed on people's exports. With workflow defaults set to
none and reconstruction off, a 16-bit sRGB export differed from Greign's
reference by mean absolute 0.00000182 and maximum 0.00006104 on the 0–1 scale.
This validates the sample's handoff, not every possible adjustment or RAW.

## Lightroom

**Not yet tested in Lightroom/Camera Raw.** The file follows the DNG 1.4
linear-float representation, but successful darktable import is not proof
of Adobe compatibility or matching default appearance. Try importing the
sample below and check color, exposure, crop, and further adjustment behavior
before relying on this for a production Lightroom workflow. Its automatic
profile/tone processing may produce a different initial look.

## Samples, tests and profiling

Visible files are in `exports/dng-handoff/`:

- `simulated-positive.dng`: 1920×1280 simulated-negative conversion.
- `greign-reference.tif`: Greign's matching proof.
- `darktable-neutral-16bit.tif`: neutral darktable handoff check.
- `darktable-default.tif`: darktable's automatic initial rendering.
- `color-check.dng` and the `color-check-*.png` outputs: diagnostic patches.

These use an existing positive photograph to synthesize a negative, not a
real film scan. Generate them with:

```sh
cargo run --offline --release -p greign-film --example dng_handoff
```

26 tests passed across app, film and export, including DNG tag/sample
inspection, selected-only export, source preservation, and unchanged existing
JPEG/TIFF behavior. Generic TIFF decoders need not support `LinearRaw`; the
unit test inspects the DNG directory and strip bytes directly, and darktable
provides the independent actual decoder check. Clippy and the native release
build pass. Native dialog interaction and Lightroom remain manual checks.

Logs: `artifacts/dng-tests.log`, `dng-clippy.log`, `dng-native-build.log`,
`dng-darktable.log`, `dng-no-reconstruction-values.log`, and
`dng-roundtrip-16bit.log` (all under `artifacts/`).

The raw CPU stage timing dump is `artifacts/dng-profile.log`. For the 2.46 MP
sample, loading/resizing/demo synthesis plus analysis took 295 ms, rendering
34 ms, and DNG encoding/writing 17 ms. Loading dominated this one-sample
measurement; it is not a full-resolution RAW benchmark or a throughput
guarantee. Writes benefit from the filesystem cache.
