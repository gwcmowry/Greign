# /// script
# requires-python = ">=3.12"
# dependencies = ["numpy>=2.1,<3", "rawpy>=0.25,<1", "pillow>=11,<13"]
# ///
"""Verify R7 decoder digests and report explicit display-referred comparison ROIs."""

import hashlib
import json
from pathlib import Path

import numpy as np
import rawpy
from PIL import Image


def metrics(path, box):
    with Image.open(path) as image:
        a = np.asarray(image.crop(box).convert("RGB"), dtype=np.float64) / 255
    rg, bg = a[..., 0] - a[..., 1], a[..., 2] - a[..., 1]
    y = a @ [0.2126, 0.7152, 0.0722]
    roughness = lambda c: float(np.sqrt(np.mean(np.diff(c, axis=1) ** 2)))
    return {"path": str(path), "roi_xyxy": box, "mean_rgb": a.mean(axis=(0, 1)).tolist(),
            "rg_difference_rms": roughness(rg), "bg_difference_rms": roughness(bg),
            "luma_difference_rms": roughness(y),
            "bright_magenta_fraction": float(np.mean((y > .45) & ((a[..., 0]+a[..., 2])*.5-a[..., 1] > .15)))}


def main():
    root = Path(__file__).resolve().parent.parent
    reports = json.loads((root / "artifacts/render-fix/r7-corpus.json").read_text())
    expected = {p.resolve() for p in (root / "raw").glob("*.CR3")}
    assert { (root / r["input"]).resolve() for r in reports } == expected, "Incomplete corpus report"
    checks = []
    for report in reports:
        meta = report["metadata"]
        crop = meta["crop_area"]
        x, y, w, h = (crop[k] for k in ("x", "y", "width", "height"))
        with rawpy.imread(str(root / report["input"])) as raw:
            digest = hashlib.sha256(raw.raw_image[y:y+h, x:x+w].astype("<u2").tobytes()).hexdigest()
            saturation = raw.camera_white_level_per_channel
            check = {"input": report["input"], "samples": w*h,
                     "exact_mosaic_match": digest == report["crop_sha256_le_u16"],
                     "greign_white": meta["white_levels"], "libraw_saturation": saturation,
                     "white_level_match": all(v == meta["white_levels"][0] for v in saturation)}
            checks.append(check)
            assert check["exact_mosaic_match"] and check["white_level_match"], check
    folder = root / "exports/r7-render-fix"
    regions = {"sun_halo": ("152A9949", (1050, 350, 2550, 1500)),
               "hair": ("152A9949", (1080, 1770, 1840, 2670)),
               "sky": ("152A9971", (2200, 400, 3200, 900))}
    comparisons = {}
    for name, (stem, box) in regions.items():
        comparisons[name] = [metrics(folder / f"{stem}-{variant}.jpg", box)
                             for variant in ("before", "after", "darktable", "no-chroma-denoise")
                             if (folder / f"{stem}-{variant}.jpg").exists()]
    result = {"decoder_checks": checks, "regions": comparisons,
              "note": "JPEG/display metrics; tone and exposure differ from darktable. Use after vs no-chroma-denoise to isolate denoising."}
    (root / "artifacts/render-fix/validation.json").write_text(json.dumps(result, indent=2) + "\n")
    print(f"Verified {len(checks)} exact mosaics and saturation fields; saved ROI metrics.")


if __name__ == "__main__":
    main()
