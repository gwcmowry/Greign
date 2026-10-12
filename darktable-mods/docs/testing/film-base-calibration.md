# Film-base calibration without a blank frame

A whole blank frame is useful but not required. Greign treats film-base
calibration as an evidence hierarchy, and records the chosen mode in
`roll-analysis.json` instead of silently pretending every estimate is equally
reliable.

## Calibration hierarchy

1. **Explicit reference patch.** Select any small, clean, unexposed area of
   the same strip: rebate, leader, or an inter-frame gap. The rest of that scan
   may contain a photograph. This is the strongest option.
2. **Automatically measured rebate.** If one or more ordinary scans include
   clean film outside the image, their samples are combined robustly. No blank
   frame is needed.
3. **Inferred roll envelope.** With at least three ordinary frames and no
   visible rebate, Greign combines their high-transmission envelopes. This is
   useful but lower-confidence because a dense or unusually uniform roll may
   never expose the true base level.
4. **Borrowed prior.** A saved measurement may be reused only for the same film
   stock with unchanged light source, camera exposure, aperture, and RAW
   development. Confidence is capped because aged film base and fog can vary
   between rolls.
5. **Per-frame fallback.** A single frame can still be converted. It is marked
   low-confidence and is not presented as a calibrated roll result.

The Essential Film Holder can crop very consistently while still leaving no
rebate. In that case, scan the roll normally and use roll-envelope inference.
If practical, shifting the strip once to capture only a few millimeters of
clean rebate gives a materially stronger calibration; it does not need to be
a full empty frame.

## Run without any film-base reference

Pass at least three scans from one roll and one unchanged capture setup:

```sh
UV_CACHE_DIR=/tmp/greign-uv-cache uv run --python 3.12 \
  scripts/film_auto.py /path/to/one-roll/*.NEF \
  --roll-analysis \
  --output-dir artifacts/my-roll
```

The result uses `inferred-roll-envelope` when no usable rebate is visible.
Variation among the photographs helps: a mix of light and dark scenes is more
informative than several nearly identical dense frames.

## Select a small patch in an ordinary scan

The rectangle is `X Y WIDTH HEIGHT` in decoded-image pixels. It must contain
only clean, unexposed film:

```sh
UV_CACHE_DIR=/tmp/greign-uv-cache uv run --python 3.12 \
  scripts/film_auto.py /path/to/one-roll/*.NEF \
  --film-base-reference /path/to/one-roll/frame-03.NEF \
  --film-base-rect 120 80 300 150 \
  --output-dir artifacts/my-roll
```

Supplying a reference or prior automatically enables roll analysis. A reliable
explicit or automatically measured result writes
`artifacts/my-roll/film-base-prior.json`. That file can be supplied to another
batch with `--base-prior`, subject to the same-stock and unchanged-capture
constraint recorded in the file. A visibly selected but statistically uneven
patch is marked `film-base-reference-nonuniform` and is not exported as a
reusable prior.

## What this cannot recover

No software can reconstruct scanner channel data that clipped during capture.
Greign rejects clipped film-base measurements rather than using their color as
a roll baseline. A fully cropped roll with only dense photographs may also
underestimate the true base; the inference mode reports this uncertainty.

Old color film deserves particular caution because age, storage, processing,
and emulsion lot can change base color and fog. A prior from a different roll
is a fallback, not a substitute for evidence from the strip itself. B&W avoids
the orange-mask problem, but base fog can still vary.
