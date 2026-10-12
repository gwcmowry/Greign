# Greign color-noise investigation — 2026-09-05

The evidence points to problems in photographic processing, plus a confirmed
Canon R7 saturation-metadata lookup defect. There is no evidence of a scrambled
Bayer mosaic in the available public R7 fixture. This does not establish the
dominant cause in every private photograph: their source drive is unavailable.
No production rendering behavior was changed during this investigation.

## Independent decoder comparison

Compared Greign's Rawler 0.7.0 output with rawpy 0.27.1 / LibRaw 0.22.1 on
`fixtures/downloads/canon-eos-r7-raw.cr3`, before demosaic or color processing.

- All **32,294,400** integer sensor samples in the camera crop match exactly:
  zero differing samples and maximum absolute difference zero.
- Both report RGGB at the full sensor origin and matching as-shot white balance
  (approximately 1.7041016, 1, 1.6591797 after green normalization).
- Greign's crop is x=156, y=84, width=6960, height=4640. Its CPU `active_crop`
  shifts the CFA origin, and the GPU parameter preparation does the same.
- Black levels differ by at most one code value: Rawler's spatial tile is
  `[512,511,511,512]`; LibRaw exposes a common value of 511. This is far too
  small to explain gross Bayer-like corruption in ordinary exposed pixels.

The comparison deliberately uses matching sensor coordinates rather than
comparing differently cropped developed images. LibRaw's visible area and
Greign's active area have slightly different extents; their recommended crop
agrees. This test covers the public full-RAW fixture, not the unavailable
private C-RAW files.

## Confirmed R7 white-level field error

Rawler 0.7.0's `src/decoders/cr2/colordata.rs`, in the version-48 R7/R10 arm,
reads normal/specular white levels at word offsets `0x281` and `0x282`.
Its CR3 decoder uses that supposed specular white level as the sensor white.

The actual ColorData block in this fixture contains:

| Word offset | Field | Value |
|---|---|---:|
| `0x280` | NormalWhiteLevel | 12,535 |
| `0x281` | SpecularWhiteLevel | 13,036 |
| `0x282` | LinearityUpperMargin | 12,735 |

The field names/offsets are independently documented in
[ExifTool's Canon ColorData11 implementation](https://raw.githubusercontent.com/exiftool/exiftool/master/lib/Image/ExifTool/Canon.pm).
The local LibRaw source at
`darktable-mods/darktable/src/external/LibRaw/src/metadata/canon.cpp`
also starts the normal/specular pair at `0x69 + 0x217 = 0x280` for version 48.
Reading the fixture's embedded TIFF directly confirms the values above.

Thus Greign consumes the linearity-margin field as if it were the saturation
white level. With black around 512, this increases normalized values by about
2.46% and causes the `0.995` highlight threshold to be crossed early.
`develop_quality` applies this threshold before demosaic, and
`reconstruct_clipped_samples` can then replace samples using a local estimate.
This is a plausible contributor to bright-region speckling and false color,
particularly where adjacent noisy samples cross that threshold inconsistently.

It is **not proven to explain the whole complaint**. On this mostly unclipped
flower fixture, only 14 samples cross the current threshold versus 13 using
LibRaw's camera saturation level. The private backlit photos are needed to
measure the effect where the reported artifacts actually occur. LibRaw's
nominal 16,383 white level is a separate value and should not be substituted
blindly for the camera saturation field either.

## The quality pipeline preserves and amplifies color noise

The current `crates/greign-pipeline/src/develop.rs` quality path is:
normalization → crop → white balance → highlight reconstruction → PPG demosaic
→ camera color transform → exposure → RGB sharpening → tone/output.

It has no sensor or chroma denoising stage, no green-plane equilibration, and
no RAW chromatic-aberration correction. Its bad-pixel repair is called with an
empty list. Their absence does not prove that each would be needed on every
photograph, but this is still an incomplete photographic quality pipeline.

`capture_sharpen`, in `crates/greign-pipeline/src/detail.rs`, sharpens all three
RGB channels independently with a default amount of 0.2. It therefore also
sharpens differences between channels. A CPU stage-isolation measurement on
a fixed 512×512 bokeh ROI in the public R7 fixture confirms this:

| Demosaic / sharpening | R−G difference RMS | B−G difference RMS |
|---|---:|---:|
| Bilinear / 0 | 0.0032000 | 0.0043627 |
| Current PPG / 0 | 0.0021044 | 0.0029057 |
| Current PPG / 0.2 | 0.0023880 | 0.0032975 |

These are horizontal adjacent-pixel chroma differences in linear Rec.2020,
excluding an eight-pixel border. They reduce sensitivity to the smooth scene
gradient, but are roughness metrics rather than calibrated noise estimates.
Sharpening raises both by approximately **13.5%**. Highlight reconstruction
changes zero samples in this ROI. No JPEG encoder participates in this test.
PPG performs better than bilinear here; the data does not support calling the
current demosaicer universally broken.

PPG is also a relatively basic choice. The
[darktable 4.6 demosaic documentation](https://docs.darktable.org/usermanual/4.6/en/module-reference/processing-modules/demosaic/)
describes its limitations and uses RCD as its default. A demosaic comparison
must hold normalization, tone, exposure, and sharpening constant before
attributing differences to the interpolation algorithm.

## The saved JPEGs represent different pipeline versions

The original `fixtures/jpg/*-greign-q95.jpg` batch is dated August 30. Later
opposed-highlight and PPG variants are under `fixtures/jpg/oracle-comparison/`,
dated August 31. The current CPU code and WGSL shader both implement PPG;
some older ADRs still describe the earlier edge-aware implementation.

The [earlier independent oracle investigation](r7-darktable-baseline.md)
already recorded magenta highlight speckling shared by CPU and GPU. Its
highlight correction reduced the selected hair ROI's magenta-outlier fraction
from 0.0001225 to zero, while sky/edge issues remained unresolved. Inspection
of saved 1:1 ridge crops confirms localized colored speckles and edge fringes,
not a general failure to turn the Bayer mosaic into a recognizable photograph.
Different tone/exposure settings make whole-image color variance an unreliable
way to attribute these artifacts.

## Reproduction and next investigation

From the Greign root, using only the CPU:

```sh
cargo run --offline --release -p greign-cli --example raw_diagnostic -- \
  fixtures/downloads/canon-eos-r7-raw.cr3 artifacts/demosaic-diagnosis/public-r7
UV_CACHE_DIR=/tmp/greign-uv-cache uv run --offline --python 3.12 \
  scripts/diagnose_raw.py fixtures/downloads/canon-eos-r7-raw.cr3 \
  artifacts/demosaic-diagnosis/public-r7
```

The example saves the raw mosaic, metadata, camera-RGB ROI dumps, and stage
measurements. The Python script saves `decoder-comparison.json`, including
the directly inspected ColorData words. The fixed ROI is selected for the
public R7 flower fixture; it is not a general scene detector. Dependencies
must already be cached for the offline Python command.

When the private drive returns, first compare the mosaics and saturation
metadata of `152A9949.CR3` and `152A9971.CR3`. Then isolate corrected saturation
metadata, highlight reconstruction, demosaic, and sharpening on the same
highlight/sky/edge regions with matched exposure. The saturation lookup is
the first concrete defect to address; residual broad chroma noise and colored
edges need separate measurements. CPU/GPU parity alone cannot validate an
algorithm they share.

Validation completed: 37 pipeline tests and 11 RAW unit tests pass; the three
private-RAW tests skip internally because the private corpus is unavailable.
The diagnostic example passes Clippy with warnings denied. The project
separation also passed 12 native film tests, 15 Python film tests, the three
film conversions, and hash/metadata verification of all three moved fixtures.
