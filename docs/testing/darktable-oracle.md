# Darktable render oracle

The external oracle catches defects shared by Greign's CPU and GPU paths. It
does not define Greign's look and is not a bit-exact golden renderer.

The currently pinned version is darktable 4.6.1. Generate an isolated
full-resolution 16-bit TIFF with:

```sh
scripts/darktable-oracle.sh photo.CR3 artifacts/oracle/photo-darktable.tif
```

The script refuses to overwrite an output. Override the expected version only
when deliberately reviewing an oracle upgrade:

```sh
GREIGN_DARKTABLE_VERSION=5.4.1 scripts/darktable-oracle.sh \
  photo.CR3 artifacts/oracle/photo-darktable.tif
```

Compare matching semantic crops. ROIs are `x,y,width,height`; separate values
account for decoder active-area differences:

```sh
UV_CACHE_DIR=/tmp/uv-cache uv run scripts/oracle_metrics.py \
  candidate.jpg oracle.tif \
  --candidate-roi 4550,1080,1500,690 \
  --oracle-roi 4562,1090,1500,690 \
  --output artifacts/oracle/edge-metrics.json
```

Darktable defaults, output exposure, and tone are not identical to Greign.
Interpret absolute cross-render pixel differences cautiously. Useful regression
signals are changes in smooth-region chroma variance, bright magenta/cyan
outlier fractions, and edge-chroma percentiles. Always retain synthetic
operator invariants and inspect difficult real crops alongside these metrics.
