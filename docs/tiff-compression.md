# Lossless TIFF compression

New TIFF exports use ZIP/Deflate level 1, horizontal prediction, and 64-row
strips. RGB16 samples and the sRGB ICC profile are retained. Previous exports
were uncompressed; they have not been modified. JPEG and DNG are unchanged.

## Measurements

CPU-only release measurements on the existing full-resolution
`01-152A0123-after-positive.tif` (2026-09-07):

| Encoding | File bytes | Encode time |
| --- | ---: | ---: |
| Original uncompressed | 193,787,226 | Not measured |
| ZIP fast, selected default | 147,780,668 | 1,013 ms |
| ZIP balanced | 141,499,235 | 5,576 ms |

Fast ZIP saves about 24% on this image. Balanced saves only another 6.3 MB
while taking 5.5 times as long. Encoding is the largest measured stage:
input decode took 237 ms and compressed readback took 695 ms in the fast run.
These are single-image stage timings, not general throughput guarantees.
Detailed/noisy scans may compress less well than smooth images.

Every decoded 16-bit sample matched the input exactly. The automated export
test additionally checks TIFF compression/predictor tags and ICC preservation.
Raw measurements are in `artifacts/tiff-compression-profile.csv` and
`artifacts/tiff-compression-balanced.csv`. Reproduce with:

```sh
cargo run --offline --release -p greign-film --example tiff_compression_check -- path/to/rgb16.tif
```

The check writes a viewable copy to
`exports/tiff-compression-check/compressed-positive.tif` without changing the
input. Darktable 4.6.1 independently imported that copy and exported
`exports/tiff-compression-check/darktable-readback.png` with OpenCL disabled;
the log is `artifacts/tiff-compatibility.log`.

Adobe documents TIFF ZIP compression in its
[Lightroom reference](https://helpx.adobe.com/pdf/lightroom_reference.pdf)
and [Photoshop format options](https://helpx.adobe.com/photoshop/using/saving-files-graphics-formats.html).
Lightroom interoperability has not been tested locally.
