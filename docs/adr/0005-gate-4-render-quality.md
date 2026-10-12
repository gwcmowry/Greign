# ADR 0005: Gate 4 quality render contract

- Status: Accepted
- Date: 2026-08-29

## Context

The bilinear CPU oracle is useful for numerical verification but visibly
softens diagonal and high-frequency detail. R7 C-RAW testing also exposed
magenta clipped skies and a colored sensor-edge strip when only Rawler's
`active_area` was retained.

## Decision

Use directional green interpolation followed by local color-difference
interpolation as the initial quality demosaic. Fall back to bilinear within
three pixels of a physical image edge. A four-pixel tile halo makes the tiled
implementation numerically identical to the full-frame implementation.

Reconstruct isolated clipped mosaic samples conservatively. After the camera
matrix, suppress impossible highlight chroma only when at least two Rec. 2020
channels exceed the sensor-white region; preserve single-channel saturated
colors. Use Rawler's `crop_area` for presentation when available and retain
`active_area` separately as sensor metadata.

The first render-supported set is Canon EOS R7 CR3 full RAW and C-RAW. It uses
Rawler's D65 camera matrix and Greign's documented fixed tone curve. External
DCP profiles and look tables remain outside the claimed compatibility set.

## Consequences

Synthetic sharp-edge error improves over bilinear, all Bayer layouts have
CPU/GPU bilinear parity, and tiled edge-aware output is exactly equal at seams
and borders. Private R7 landscape and portrait renders no longer show magenta
clipped skies or sensor-edge stripes. The CPU quality path still holds large
full-frame intermediates; Gate 5 owns bounded scheduling and cache reuse.
