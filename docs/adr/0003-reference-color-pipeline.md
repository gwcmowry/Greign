# ADR 0003: Gate 2 reference color pipeline

- Status: accepted
- Date: 2026-08-29
- Gate: 2

## Context

Greign needs a deterministic, independently testable CPU path before adding
GPU acceleration. Sensor values below black and scene values outside `[0, 1]`
must survive intermediate operations. Canon R7 color matrices follow the
dcraw/Rawler XYZ-to-camera convention.

## Decision

Use linear `f32` through normalization, white balance, bilinear demosaic,
camera conversion, exposure, and tone mapping. The documented working space is
linear Rec. 2020 at D65.

Compose the XYZ-to-camera matrix with Rec. 2020-to-XYZ, normalize each resulting
row, and invert that working-to-camera matrix. Row normalization is required to
preserve a white-balanced neutral; directly inverting the published camera
matrix produced a gross magenta cast in the first R7 proof render.

Use a signed Reinhard curve as the deliberately simple initial
scene-to-display operator. Clamp only while converting working RGB to bounded
gamma-encoded sRGB. Embed the CC0 colord sRGB ICC profile in JPEG and 16-bit
TIFF proof exports.

## Consequences

The CPU path is deterministic and all numerical operators can be tested
without codecs or UI types. The reference R7 render is recognizable and has
plausible orientation and color, but its simple bilinear demosaic and tone
curve are not a finished photographic look.

The untiled full-resolution reference path peaks around 588 MiB and takes about
3.2 seconds on the development system. Gate 5 must replace full-frame preview
intermediates with tiled scheduling; this reference remains the correctness
oracle.

## Alternatives considered

Clamping normalized sensor values was rejected because it destroys data below
the declared black level and makes CPU/GPU edge behavior ambiguous. Using the
embedded JPEG as the render was rejected because it bypasses Greign's sensor
and color pipeline. A higher-quality demosaic is deferred until Gate 4 corpus
comparisons.

