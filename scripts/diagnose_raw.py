# /// script
# requires-python = ">=3.12"
# dependencies = ["numpy>=2.1,<3", "rawpy>=0.25,<1"]
# ///
"""Compare a raw_diagnostic dump with LibRaw, before photographic processing."""

import argparse
import json
import re
import struct
from pathlib import Path

import numpy as np
import rawpy


def canon_colordata48(path):
    """Inspect the version-48 TIFF ColorData block in this Canon fixture.

    This bounded diagnostic scan is not a general CR3 container decoder.
    It also reaches the embedded CTMD TIFF, where this fixture stores ColorData.
    """
    data = path.read_bytes()
    for match in re.finditer(re.escape(b"II*\x00"), data):
        origin = match.start()
        try:
            offset = struct.unpack_from("<I", data, origin + 4)[0]
            if offset > 100_000:
                continue
            count = struct.unpack_from("<H", data, origin + offset)[0]
            if count > 1000:
                continue
            for i in range(count):
                tag, kind, size, value = struct.unpack_from(
                    "<HHII", data, origin + offset + 2 + i * 12)
                if tag != 0x4001 or kind != 3 or size != 3778:
                    continue
                words = struct.unpack_from(f"<{size}H", data, origin + value)
                if words[0] == 48:
                    return {"tiff_file_offset": origin, "version": 48,
                            "words": {hex(j): words[j] for j in range(0x27c, 0x284)}}
        except struct.error:
            continue
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("dump_directory", type=Path)
    args = parser.parse_args()
    meta = json.loads((args.dump_directory / "metadata.json").read_text())
    candidate = np.memmap(args.dump_directory / "mosaic.u16le", dtype="<u2",
                          mode="r", shape=(meta["height"], meta["width"]))
    with rawpy.imread(str(args.input)) as raw:
        if candidate.shape != raw.raw_image.shape:
            raise ValueError(f"Different full sensor shapes: {candidate.shape}, {raw.raw_image.shape}")
        crop = meta["crop_area"] or meta["active_area"]
        x, y, w, h = (crop[k] for k in ("x", "y", "width", "height"))
        roi = np.s_[y:y+h, x:x+w]
        difference = candidate[roi].astype(np.int32) - raw.raw_image[roi].astype(np.int32)
        levels = np.asarray(meta["black_levels"])
        if levels.size == 1:
            levels = np.repeat(levels, 4)
        channels = raw.raw_colors[roi]
        pattern = "".join(chr(raw.color_desc[i]) for i in raw.raw_colors[:2, :2].flat)
        libraw_wb = np.asarray(raw.camera_whitebalance[:3]) / raw.camera_whitebalance[1]
        raw_values = candidate[roi]
        clipping_threshold = 0.995
        greign_white = meta["white_levels"][0]
        phase_values = []
        for dy in range(2):
            for dx in range(2):
                values = raw_values[dy::2, dx::2]
                black = levels[((y+dy) % 2) * 2 + ((x+dx) % 2)]
                phase_values.append(int(np.count_nonzero(
                    values >= black + clipping_threshold * (greign_white - black))))
        saturation = raw.camera_white_level_per_channel
        libraw_clipped = None
        if saturation:
            black = np.asarray(raw.black_level_per_channel)[channels]
            white = np.asarray(saturation)[channels]
            libraw_clipped = int(np.count_nonzero(
                raw.raw_image[roi] >= black + clipping_threshold * (white - black)))
        report = {
            "input": str(args.input), "rawpy": rawpy.__version__,
            "libraw": list(rawpy.libraw_version), "comparison_crop": crop,
            "samples_compared": int(difference.size),
            "different_samples": int(np.count_nonzero(difference)),
            "maximum_absolute_sample_difference": int(np.abs(difference).max()),
            "mean_absolute_sample_difference": float(np.abs(difference).mean()),
            "greign_cfa_at_full_sensor_origin": meta["cfa_pattern"],
            "libraw_cfa_at_full_sensor_origin": pattern,
            "greign_white_balance": meta["white_balance"],
            "libraw_white_balance_green_normalized": libraw_wb.tolist(),
            "greign_black_levels_spatial_2x2": levels.tolist(),
            "libraw_black_levels_spatial_2x2":
                np.asarray(raw.black_level_per_channel)[raw.raw_colors[:2, :2]].tolist(),
            "greign_white_levels": meta["white_levels"],
            "libraw_camera_white_levels": saturation,
            "libraw_nominal_white_level": raw.white_level,
            "samples_flagged_at_greign_0_995_threshold": sum(phase_values),
            "samples_flagged_using_libraw_camera_levels_at_0_995": libraw_clipped,
            "note": "White-level disagreement requires camera-metadata validation; neither is assumed correct.",
        }
    if meta["model"] == "Canon EOS R7":
        report["canon_colordata48"] = canon_colordata48(args.input)
    (args.dump_directory / "decoder-comparison.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
