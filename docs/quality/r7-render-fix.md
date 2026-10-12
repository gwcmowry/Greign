# R7 rendering correction and validation

The eight private R7 photographs now have updated full-resolution JPEG exports
in [exports/r7-render-fix](../../exports/r7-render-fix/). Open the
[comparison browser](../../exports/r7-render-fix/index.html) to switch between
the original Greign exports, updated renders, and saved darktable references.
The sun photograph is `152A9949`; the sky/ridge photograph is `152A9971`.

## Rendering changes

- Vendored Rawler 0.7.0 with the version-48 Canon ColorData saturation lookup
  corrected from `0x282` (linearity margin) to `0x281` (specular white).
  All eight files agree with LibRaw: the appropriate saturation is 13,036 or
  13,660, depending on the file. No hard-coded per-camera white value is used.
- Connected clipped regions are identified on a 16-pixel sensor grid. Usable
  color near each region's boundary supplies its opposed-plane chrominance
  correction, including for the interior of a large clipped sun. Regions
  without reliable complete-color support retain the local single-channel
  fallback. Unclipped sensor samples are never replaced.
- Default chroma reduction is 0.5 on a zero-to-one scale. A 5×5 filter weights
  neighboring chroma by luminance and chroma similarity, preserving luminance
  and protecting distinct color details. There is no luminance denoising.
- Sharpening now adds only luminance detail; it no longer independently
  amplifies the three RGB channels. Default strength remains 0.2.
- The quality-render shoulder uses extended Reinhard with scene white at two,
  retaining channel ratios before output limits and allowing reconstructed
  highlights to reach display white. The simple bilinear reference path
  retains its earlier tone curve.
- Full-image parity exposed two PPG inconsistencies: almost equal directional
  gradients could choose different interpolation branches, and the GPU's
  second pass used signed red/blue samples where the CPU used clamped known
  samples. Both are corrected. The green estimator still uses signed RAW.

CPU and GPU use the same region context. GPU rendering computes that context
on the host without allocating a full normalized CPU image, then uploads the
small correction grid. GPU chroma reduction and luminance sharpening share an
output pass; no extra full-resolution texture is required.

## Quality evidence

All eight cropped mosaic digests match LibRaw exactly: 258,355,200 sensor
samples in total. Saturation fields also match on every file.

Full-resolution CPU/GPU comparison before JPEG encoding passes on both
`152A9949` and `152A9971`: maximum error is two 8-bit channel levels, no channel
exceeds that tolerance, and RMSE is approximately 0.174 levels. All four Bayer
layouts also pass clipped-region parity with an odd crop and chroma strengths
0, 0.5, and 1. A separate parity fixture exercises below-black sensor values.

JPEG ROI measurements (display-referred, no exposure matching):

| Region / measure | Fresh before | Updated | Darktable reference |
|---|---:|---:|---:|
| Sun/halo mean RGB | .847, .815, .759 | .972, .957, .906 | .988, .976, .941 |
| Hair bright-magenta fraction | 0.0000965 | 0 | 0.00000439 |
| Sky R−G adjacent-difference RMS | .008416 | .004213 | .005420 |
| Sky B−G adjacent-difference RMS | .007652 | .004143 | .004665 |

The sun is brighter with a warm transition; the sampled backlit hair has no
bright-magenta outliers. Relative to the fresh pre-change render, sky chroma
roughness falls about 50% in R−G and 46% in B−G. Holding the other current
settings fixed, denoising alone reduces those measures about 9% and 8%.
This is deliberately mild. The larger before/after improvement includes the
metadata, reconstruction, sharpening, and display changes.

Darktable uses different exposure and tone defaults; its statistics are not
an absolute target and do not prove Greign preserves more detail. Synthetic
tests separately verify unchanged luminance, retention of a small saturated
color feature, distant warm-boundary support, separate region colors, and
unchanged unclipped samples. Full clipping destroys the original color;
reconstructed color is an estimate from surviving surroundings.

The 16-pixel context grid can merge very close clipped objects. It is not a
full-resolution semantic segmentation model. This change also does not add
lens chromatic-aberration correction or a camera-calibrated noise model.

## Performance

Repeated stage wall-clock timings replace privileged sampling. Reports keep
every run, and GPU kernels use device timestamps. No system profiling
permissions or kernel settings were changed.

For the full `152A9949` CPU render, caching luminance/chroma computations and
using Rayon across independent rows reduced chroma-filter time from a median
2,474 ms to 199 ms and luminance sharpening from 460 ms to 38 ms.
The remaining main CPU stages are demosaic (607 ms) and display conversion /
orientation (616 ms). Highlight analysis plus reconstruction is about 199 ms.

The GPU corpus run initializes the device once. Decode + render/readback + JPEG
encoding is approximately 0.69–0.79 seconds per image, excluding one-time GPU
initialization and validation-only CPU/digest work. The full render portion is
about 0.23–0.26 seconds. Host highlight analysis takes 145–158 ms and is the
largest render-stage cost; JPEG encoding has a median of about 313 ms.
For `152A9949`, GPU kernels take approximately 0.51 ms sensor, 5.18 ms demosaic,
1.79 ms color, and 3.18 ms denoise/sharpen/tone/output.

Raw evidence lives under `artifacts/render-fix/`: `stages-first.json`,
`stages-final.json`, `r7-corpus.json`, `validation.json`, and check logs.

## Use and reproduction

```sh
cargo run --release -p greign-cli -- raw render raw/152A9949.CR3 \
  exports/r7-render-fix/my-render.jpg --quality --gpu

# Disable only chroma denoising, keeping the other corrections:
cargo run --release -p greign-cli -- raw render raw/152A9971.CR3 \
  exports/r7-render-fix/my-no-denoise.jpg --quality --chroma-denoise 0

# Repeat CPU stage measurements without privileged profiling:
cargo run --release -p greign-cli --example render_stages -- \
  raw/152A9949.CR3 exports/r7-render-fix/profile-render.jpg \
  artifacts/render-fix/profile.json 3

# GPU corpus exports and full-image parity (requires GPU access):
cargo run --release -p greign-cli --example validate_r7 -- \
  raw exports/r7-render-fix artifacts/render-fix/r7-corpus.json
UV_CACHE_DIR=/tmp/greign-uv-cache uv run --offline --python 3.12 \
  scripts/check_r7_exports.py
```

Workspace tests, Clippy, formatting, API documentation, seven explicit GPU
tests, and full-image parity pass. The public full-RAW fixture tests the
saturation correction through both metadata-only and full-decode paths; the
private corpus exercises C-RAW. Recipe pipeline version is now 2 so old recipe
behavior cannot silently change; recipe and catalog schemas remain version 1.
Private RAWs and exported photographs are Git-ignored, and original files are
preserved.
