# Automatic film-conversion spike

This is the first executable vertical slice of the darktable-based film
workflow. It is a correctness harness and parameter-derivation prototype, not
yet a finished color model or darktable user interface.

## What is native in the darktable fork

The pinned `darktable/` source tree has a new pure C subsystem:

- `src/common/film_analysis.h`
- `src/common/film_analysis.c`
- `src/common/film_roll_analysis.h`
- `src/common/film_roll_analysis.c`
- `src/tests/unittests/common/test_film_analysis.c`
- `src/tests/unittests/common/test_film_roll_analysis.c`

It takes non-negative scene-linear RGB, measures film-base transmission and
per-channel density distributions with bounded histograms, and derives the
technical values needed by negadoctor: Dmin, Dmax, scan offset, and highlight
white-balance coefficients. Analysis is deterministic, bounded-memory, and
keeps no input pixels after the call.

Twelve native tests cover known film-base recovery, color/B&W parameter
derivation, conversion monotonicity, invalid input, explicit and measured
calibration, roll-envelope inference, and prior/per-frame fallbacks. The
implementation is also compiled directly with strict warnings while the
complete fork build is being connected.

The roll subsystem separates shared base/color/range statistics from per-frame
luminance placement. This is the first behavior in the fork that goes beyond
negadoctor's ordinary one-image-at-a-time parameter workflow.

Negadoctor now also consumes the native analysis in darktable's preview
pixelpipe and commits its technical controls as one ordinary history item. See
the [native Negadoctor integration](darktable-negadoctor-integration.md).

## Executable reference

`scripts/film_auto.py` currently provides the end-to-end harness:

1. Decode the camera RAW to scene-linear wide-gamut RGB.
2. Detect the film-frame rectangle from smoothed edge projections.
3. Create a separate inset analysis rectangle so sprockets, borders, and direct
   light do not change density endpoints.
4. Estimate film base from the outer rebate, falling back to robust global
   highlights when a dark holder dominates the border.
5. Measure per-channel density floor, median, and ceiling.
6. Apply the measured inversion and a named, separately recorded virtual-paper
   intent based on negadoctor's equation.
7. Force true monochrome output for B&W film.
8. Write all measurements, warnings, confidence values, geometry, source hash,
   render intent, and timings to JSON.

With `--roll-analysis`, the harness now aggregates multiple frames from one
roll. A whole blank frame is optional: a selected small rebate patch has first
priority, followed by automatically visible rebate, inference from at least
three ordinary frames, a compatible saved prior, and finally a labeled
per-frame fallback. See [film-base calibration](film-base-calibration.md) for
commands and limitations.

The Python implementation is intentionally transparent. Once the parameter
model settles, the same orchestration will call the C analysis within
darktable's background job and commit ordinary negadoctor history parameters.

## Run it

The wrapper requires the three files in the film-negative corpus and packages
already present in `/tmp/greign-uv-cache`:

```sh
./scripts/check-film-auto.sh
```

To allow UV to populate an empty cache once:

```sh
UV_CACHE_DIR=/tmp/greign-uv-cache uv run --python 3.12 \
  scripts/test_film_auto.py
```

## Inspectable output

Everything is under `artifacts/film-conversions/` and ignored by Git:

- `contact-sheet.jpg`: current cropped automatic conversions and confidence;
- `correctness-comparison.jpg`: automatic conversions beside community XMP
  renders, explicitly labeled as creative references rather than ground truth;
- `*-auto-positive-cropped.jpg`: the main current result for visual review;
- `*-auto-positive.jpg`: uncropped result, useful for geometry diagnosis;
- `*-negative.jpg`: display rendering of the decoded negative;
- `*-analysis-mask.jpg`: analysis-area diagnostic;
- `*-analysis.json`: complete per-frame measurements and parameters;
- `run-report.json`: run-level index.
- `roll-analysis.json`: selected calibration mode and shared roll parameters;
- `film-base-prior.json`: reusable same-session prior, written only from a
  measured or explicit base.

The community XMP sidecars and local renders remain under
`artifacts/film-conversions/community-reference/`. They retain the source
photographers' CC BY-SA attribution. They are valuable expert edits, but they
also contain crops and creative modules, so they must not be treated as numeric
ground truth.

## Correctness properties currently enforced

The fifteen executable Python tests require:

- recovery of a known synthetic color-film base;
- rejection of a dark holder as film base and a recorded fallback warning;
- frame geometry recovery within five pixels on a synthetic holder;
- monotonically brighter positive output for increasing negative density;
- bit-identical R, G, and B channels for monochrome output;
- finite analysis when isolated NaN and infinity pixels occur.
- clipping warnings to mean the decoder ceiling rather than merely the
  brightest sample in a frame;
- roll calibration from a small patch, visible rebate, or a no-blank-frame
  multi-image envelope;
- explicit labeling and confidence caps for borrowed and per-frame fallbacks;
- shared roll color/range parameters without flattening per-frame luminance;
- rejection of mixed color and monochrome batches.
- rejection of saved priors for the wrong film type and warnings for uneven
  explicitly selected patches.

Visual inspection of the real corpus additionally confirmed:

- all three RAWs decode and invert with the expected orientation retained;
- the coastal color result is now broadly comparable to its negadoctor XMP
  render after using the actual virtual-paper equation;
- the B&W result is neutral and uses a separately declared gentler paper grade;
- the difficult snow frame remains low-confidence and reports its questionable
  rebate instead of presenting the conversion as reliable.

## What is not yet proven

- Roll-envelope inference is scene-dependent. A complete-roll corpus with
  measured base evidence is required before claiming consistent color.
- There is no color target or known scene reference, so pleasing color is not
  evidence of colorimetric accuracy.
- Frame detection does not yet estimate rotation or perspective.
- Film-base sampling uses robust statistics, not a spatial uniformity model.
- Community edits are subjective and cannot define the single correct result.
- The executable reference currently uses RawPy for convenient headless RAW
  decode. Production will use the fork's darktable pixelpipe.
- The derived parameters are not yet committed into a darktable history stack
  or exposed in GTK.

The next correctness milestone is a licensed full roll with varied scenes, a
small measured base reference, and photographer-approved reference renders.
That enables outlier detection, crop-invariance tests, and direct comparison of
measured versus no-blank-frame inference without guessing scene intent.

## Performance evidence retained for later

Raw profiler files are saved under `artifacts/profiles/`. Before the priority
shift to correctness, cProfile localized the prototype to RAW demosaic and
full-resolution conversion. A bounded-row conversion removed redundant image
passes, reducing the measured three-image wall time from 11.09 seconds to 7.05
seconds. No further optimization is planned until the conversion model is
accepted.
