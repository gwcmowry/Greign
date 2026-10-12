# Film-negative test corpus

The initial local corpus contains two color-negative camera scans and one
black-and-white camera scan. The RAW files are ignored by Git. Their manifest,
hashes, provenance, and attribution are committed in
`fixtures/manifests/film-negatives.json`.

## Included manifest entries

| ID | Material and purpose | Capture RAW | Creator/source | License |
|---|---|---|---|---|
| `color-kodak-gold-snow-nikon-d780` | Kodak Gold 200; snow and sparse colors make automatic color and density estimation difficult; includes the film boundary | Nikon D780 NEF | clennep, [PIXLS.US Play Raw thread](https://discuss.pixls.us/t/negadoctor-snowy-day/48581) | [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/) |
| `color-aged-negative-canon` | Roughly 25-year-old color negative; known oversaturated-green/input-profile challenge; includes sprockets and film base | Canon EOS 80D CR2 | Hans Petter (hpbirkeland), [PIXLS.US Play Raw thread](https://discuss.pixls.us/t/negatives-and-color-profiles/20293) | [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/) |
| `bw-negative-fujifilm-x-t2` | B&W negative photographed with a phone backlight; difficult exposure; useful for monochrome neutrality and X-Trans coverage | Fujifilm X-T2 RAF | Bastian Bechtold (bastibe), [PIXLS.US Play Raw thread](https://discuss.pixls.us/t/how-to-process-photo-scanned-b-w-negatives/19458) | [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/) |

Attribution applies to the source photographs. Greign has not modified the
downloaded RAW bytes. If derivatives are published, credit the named creator,
link the source and license, identify modifications, and follow ShareAlike.

An additional local-only Tri-X 400 CR2 was downloaded from a
[CC BY-NC-SA 4.0 Play Raw thread](https://discuss.pixls.us/t/dont-need-no-stinkin-ai/60239).
It is intentionally excluded from the public manifest because the
NonCommercial restriction makes it unsuitable as a general project fixture.

## Verification

With the downloads in `fixtures/downloads/`, run:

```sh
cargo run --manifest-path ../Cargo.toml --release -p greign-cli -- corpus verify \
  fixtures/manifests/film-negatives.json --root fixtures/downloads
```

The three files currently total about 81 MB. Metadata and file hashes were
verified with Greign's Rawler backend. CPU preview rendering also exposed a
current Greign limitation: the Bayer NEF and CR2 render, but reduced-resolution
quality rendering rejects the X-T2's 6x6 X-Trans CFA. darktable renders that RAF
successfully. This is a concrete compatibility benefit of adopting darktable's
RAW pipeline.

## Corpus gaps

This is sufficient for an initial conversion spike but not roll analysis. The
next acquisition should be complete, consistently digitized rolls with explicit
reuse permission and, ideally, the following supporting frames:

- at least 12 frames from one color roll spanning daylight, shade, tungsten,
  dominant-color scenes, portraits, thin negatives, and dense negatives;
- one complete B&W roll with varied densities;
- a no-film flat-field capture and an unexposed-film-base capture;
- matching lab scans or photographer-approved reference renders;
- one scanner RGB+IR input set and one bracketed camera scan;
- recorded film stock, chemistry, camera, macro lens, light spectral data or
  model, exposure settings, holder, and orientation.

Do not use publicly posted files without an explicit license or direct written
permission. A public download link alone is not a redistribution grant.
