# Camera and recording-mode compatibility

Compatibility is recorded per tested camera and recording mode. A recognized
extension or a successful metadata inspection does not imply render support.

## Render-supported

| Camera | Container | Mode | Evidence | Current level |
| --- | --- | --- | --- | --- |
| Canon EOS R7 | CR3 | full RAW | CC0 public corpus file | Metadata, embedded preview, mosaic digest, quality JPEG and 16-bit TIFF |
| Canon EOS R7 | CR3 | C-RAW | Eight private camera files | Metadata, embedded preview, mosaic decode, landscape/portrait quality rendering |

The R7 path uses the camera-recommended 6960×4640 crop, EXIF orientation,
Rawler's D65 camera matrix, edge-aware RGGB demosaic, highlight chroma
reconstruction, a fixed scene-to-display tone curve, and an embedded sRGB ICC
profile. Both modes use the same recorded Canon matrix. External `.dcp` files
and DCP look tables are not yet supported or required by this tested set.

## Decode-tested, render experimental

| Camera | Container/mode | CFA | Evidence |
| --- | --- | --- | --- |
| Canon EOS 350D DIGITAL | CR2 | RGGB | CC0 mosaic digest and visual smoke render |
| Nikon D2H | NEF, 12-bit | GBRG | CC0 mosaic digest and visual smoke render |
| Sony DSLR-A100 | ARW, compressed | RGGB | CC0 mosaic digest and visual smoke render |
| Panasonic DMC-LX7 | RW2, 4:3 | GRBG | CC0 mosaic digest and visual smoke render |
| Olympus E-1 | ORF, 16-bit | GRBG | CC0 mosaic digest and visual smoke render |
| Pentax K10D | DNG, 12-bit | RGGB | CC0 mosaic digest and visual smoke render |

These files exercise the decoder and every 2×2 Bayer rotation, but one file per
camera is not enough to claim production rendering compatibility.

## Explicit decode-only case

The CC0 Fujifilm X-T1 RAF fixture decodes to its expected 6×6 X-Trans mosaic
digest. Greign deliberately rejects it in both demosaic paths because no
X-Trans quality algorithm has been validated. RAF/X-Trans rendering is not
currently advertised.
