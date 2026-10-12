# ADR 0009: Independent RAW decode and render oracles

- Status: Accepted
- Date: 2026-08-30

## Context

CPU/GPU parity proves that Greign executes one algorithm consistently, but it
cannot detect a shared algorithmic defect. Canon R7 backlit highlights exposed
magenta speckling, smooth-sky chroma variation, and colored high-contrast
boundaries in both Greign paths. The camera JPEG and darktable do not exhibit
the same failures.

RAW container decoding and photographic development are distinct. RawSpeed
and Rawler produce mosaics; highlight reconstruction, chromatic-aberration
correction, denoise, demosaic, color, and tone create a viewable image.
Replacing a correct decoder cannot repair a later processing defect.

## Decision

Keep Rawler as the primary decoder while its mosaic contract passes. Add two
independent verification layers:

1. compare decoded metadata, active area, CFA, levels, and normalized mosaic
   samples against LibRaw/RawSpeed on the curated corpus; and
2. compare final render properties against a version-pinned `darktable-cli`
   16-bit TIFF export and, for private camera samples, the embedded Canon JPEG.

Invoke darktable with an isolated config/cache, in-memory library, disabled
custom presets, high-quality full-resolution output, sRGB ICC, and OpenCL
disabled. Record the exact darktable version and its embedded XMP history.

Do not require pixel identity. Darktable has different color and tone defaults.
Verify explicit failure properties instead: highlight magenta/cyan outliers,
smooth-region chroma variance, high-contrast edge chroma, dimensions,
orientation, and color-patch behavior. Version and ROI changes require review.

`scripts/darktable-oracle.sh` generates a controlled reference.
`scripts/oracle_metrics.py` reports candidate/oracle statistics for matching
semantic regions, including independently offset ROIs when active crops differ.

## Consequences

Greign no longer promotes an image-processing change merely because its GPU
output matches its CPU implementation. Darktable remains a development/test
tool rather than a runtime dependency. Its GPLv3 license is compatible with
Greign; any algorithm or code adaptation still requires local attribution and
license review.

RawSpeed remains an optional decoder experiment, not an assumed improvement.
It intentionally omits metadata beyond white balance, demosaic, and color
development, and is not currently packaged as a standalone stable library.
