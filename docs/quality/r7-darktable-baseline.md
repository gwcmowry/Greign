# Canon R7 Darktable oracle baseline

Date: 2026-08-30

Oracle: Ubuntu darktable 4.6.1, CPU-only `darktable-cli`, isolated configuration,
custom presets disabled, in-memory library, full-resolution high-quality TIFF,
sRGB output. This distribution build reports `[libraw_open]` for these CR3
files. Each TIFF embeds the Darktable XMP history used for its export.

All eight private Canon R7 files exported successfully. Darktable took roughly
1.98–2.81 seconds per image on the Ryzen 9 7900X. Its uncropped output is
6984×4660 (or oriented equivalent), while Greign uses the camera-recommended
6960×4640 crop, so paired ROIs require a 12×10-pixel origin adjustment before
orientation.

The initial artifact metrics establish failures, not acceptance thresholds:

- In the 152A9949 backlit-hair region, Greign's bright-magenta outlier fraction
  is 0.0001225 versus Darktable's 0.00000178, approximately 69× higher.
- In a smooth 152A9971 sky region, Greign's luma standard deviation is 0.01814
  versus Darktable's 0.01183, approximately 1.53× higher.
- The Greign ridge boundary visibly contains magenta zippering and a stronger
  cyan transition. Darktable's default render does not show the magenta edge.

The first accepted oracle-driven correction ports the core of Darktable's
opposed-plane highlight estimator into Greign. On the 152A9949 hair ROI it
reduces Greign's bright-magenta outlier fraction from 0.0001225 to zero. CPU
render time remains approximately 4.13 seconds; GPU cold export remains
0.74–0.82 seconds on the two artifact-heavy frames, and the fused sensor kernel
remains below 0.6 ms. Smooth-sky variance and the cyan ridge transition remain
open demosaic/RAW-CA work rather than being concealed by a broad chroma blur.

Darktable is brighter, more saturated, and uses different tone/color defaults.
Whole-crop chroma ratios can therefore reverse even when a local artifact is
obviously worse in Greign. Regression gates must use stable semantic ROIs and
artifact-specific measures, backed by visual inspection and synthetic
invariants; full-image pixel identity is not a goal.

Ignored raw results live under `artifacts/darktable-oracle/`. Reproduce them
with `scripts/darktable-oracle.sh` and `scripts/oracle_metrics.py` as described
in `docs/testing/darktable-oracle.md`.
