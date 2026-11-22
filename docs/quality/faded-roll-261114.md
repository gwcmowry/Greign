# Faded color-negative roll: 261114 roll 002

Sources: 25 Canon R7 CR3 captures in
`/media/grant/T7 Shield/261114_roll_002/2026_11_15` and the user's accepted
`exports/film-rolls/2026_11_15 - converted/roll-recipe.json`.
The originals, existing exports, and live library were not modified. Photographs
and diagnostic caches remain in ignored local directories.

## Findings and changes

The previous detector accepted only `IMG_0008`, or 1/25 frames. Clear-film strips
on opposite sides differ substantially in brightness: in `IMG_0003`, the right
border transmits approximately 1.4 stops less light than the left while retaining
similar channel ratios. This is evidence of a spatial capture/base difference,
not proof of its physical cause. A single film-base value cannot correct it.

The detector now falls back to local film-to-picture transitions when broad
scanline percentiles describe the wrong strip. It compares normalized base
colors, excludes clipped backlight, and uses the tilt supported by other edges
to find a weak boundary among stronger scene transitions. A smooth lighting
gradient no longer qualifies as picture texture. Established detection paths
retain precedence. The fallback is restricted to borders near the capture edges
and still requires four supported sides.

The final detector accepts **24/25** frames. `IMG_0011` extends beyond the right
scan edge; its existing manual crop is retained. All 24 overlays were inspected;
96,000 perimeter samples contained no clipped perforation pixels. This check
does not assert that every scene pixel is unclipped. The 26 picture crops and
two blank abstentions in the earlier 261104 roll, plus all seven Portra crops,
are exactly unchanged.

Analysis version 5 adds two opt-in controls:

- **Correct uneven film-base lighting** samples uniform, unclipped patches
  outside the detected picture. A robust quadratic fit to log transmission
  supplies a common-channel brightness field, evaluated in original source
  coordinates before inversion. All four borders must contribute, channel
  slopes must agree, and residual/extrapolation limits must pass. A failed fit
  leaves the field at zero and reports why. Shared-roll analysis additionally
  checks agreement between frame fields.
- **Faded-film color balance** blends from a common density range (0%) toward
  separately balanced channel ranges (100%), with bounded multipliers. This can
  address unequal channel response and color crossover, but depends on scene
  content; it is not a measured dye or film-stock characterization. Dominant
  colors and missing neutral references can make full strength inappropriate.

The film-base and density operations are separate from creative white balance,
contrast, and saturation. For related density-domain calibration principles, see
the [official darktable negadoctor documentation](https://darktable-org.github.io/dtdocs/en/module-reference/processing-modules/negadoctor/).
The new border-field fitting is Greign's implementation, not a darktable algorithm.

## Using and comparing the results

The native app exposes both options in Conversion. **Preview faded-film
restoration** sets individual optimization, full color balance, and illumination
correction, then automatically displays a neutral preview. Compare **Current
conversion** and **New preview**, then **Apply conversion**. Accepted recipes,
locks, prior versions, manual
crops, rotations, mirroring, and healing retain their established behavior.
Old analyses deserialize with a zero field and unity channel multipliers.
The CPU preparation stage feeds both the GPU preview and exports, so these
corrections do not require a separate shader implementation.

Two full-resolution JPEG sets and their recipes are provided locally:

- `exports/film-rolls/2026_11_15 - balanced/`: color-density balancing with
  no spatial lighting field.
- `exports/film-rolls/2026_11_15 - restored/`: color-density balancing plus
  illumination correction wherever the border fit passes.

Each set includes contact sheets, preview variants, an analysis report, and a
recipe. Existing orientation, mirroring, and repairs are retained. New detected
crops are used; for the incomplete scan, the existing manual crop also supplies
an explicit picture-only analysis area recorded in its report. Creative edits
are neutral for the new conversions. Exact original scene colors cannot be
verified without a reference, and a measured flat-field capture would give more
reliable spatial calibration than border extrapolation.

Illumination fitting passes on **21/25** captures. `IMG_0005`, `IMG_0007`,
`IMG_0011`, and `IMG_0023` retain a zero field and carry an explanatory warning.
The labeled side-by-side comparison is
`exports/faded-roll-review/comparison.jpg`. All 50 numbered full-resolution
JPEGs were verified for valid decoding, expected dimensions, and embedded sRGB
profiles. The updated live app must be restarted to load the rebuilt binary;
the library's accepted looks have not been replaced automatically.

## Validation and reproduction

Synthetic tests cover unequal border illumination with both tilt directions,
neutral ramps with unequal channel density slopes, preservation of real scene
shading, dust/perforation outliers, missing/colored borders, public-analysis
integration, and legacy recipe defaults. Snapshot/restore also retains the new
calibration settings. The repository check script passes 180 tests (9 opt-in
tests ignored), formatting, Clippy, documentation, and schema agreement.

```sh
cargo build --release -p greign-film --example faded_roll_check --example crop_check
./target/release/examples/faded_roll_check \
  'exports/film-rolls/2026_11_15 - converted/roll-recipe.json' \
  exports/faded-roll-new-comparison full
```

The comparison tool refuses an existing destination. Omit `full` for previews.
RAW conversion and fitting run on the CPU. Cached detector benchmarks and
stage timings are kept under `artifacts/faded-roll-*`. Cargo-flamegraph was
attempted in and outside the sandbox; Linux `perf_event_paranoid=4` blocked
sampling in both cases. The failure logs are preserved; no stack profile or
flamegraph result is claimed.

Twenty repetitions per cached frame measured a median **12.0 ms** detector
time, with a maximum per-frame average of **13.2 ms**. On `IMG_0003`, CPU stage
timings measured 442 ms for preview loading and 944 ms for full RAW loading.
Analysis took 15.0 ms with the optional corrections off and 16.9 ms with them
on. Three full-resolution preparation repetitions had medians of 284 ms off
and 280 ms on; this difference is timing noise, not an optimization claim.
Finishing/cropping took about 313 ms, display conversion 339 ms, and RGB8 packing
161 ms. Among these measured stages, RAW loading dominates; display conversion
is the largest post-decode stage. JPEG encoding is excluded. Raw measurements
are saved in `artifacts/faded-roll-stage-timings.csv` and
`artifacts/faded-roll-crop-timings.jsonl`.
