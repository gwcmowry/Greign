# /// script
# requires-python = ">=3.12"
# dependencies = ["pillow==11.3.0"]
# ///
"""Compare artifact-oriented image statistics with an external render oracle."""

from __future__ import annotations

import argparse
import json
import math
import statistics
from pathlib import Path

from PIL import Image


def roi(value: str) -> tuple[int, int, int, int]:
    values = tuple(int(component) for component in value.split(","))
    if len(values) != 4 or min(values) < 0 or values[2] == 0 or values[3] == 0:
        raise argparse.ArgumentTypeError("ROI must be x,y,width,height with positive size")
    x, y, width, height = values
    return x, y, x + width, y + height


def percentile(values: list[float], percent: int) -> float:
    if not values:
        return 0.0
    values.sort()
    return values[math.ceil((len(values) - 1) * percent / 100)]


def image_metrics(path: Path, crop: tuple[int, int, int, int] | None) -> dict[str, object]:
    with Image.open(path) as opened:
        image = opened.convert("RGB")
    if crop is not None:
        if crop[2] > image.width or crop[3] > image.height:
            raise ValueError(f"ROI {crop} exceeds {image.size} for {path}")
        image = image.crop(crop)

    width, height = image.size
    pixels = list(image.getdata())
    luma: list[float] = []
    red_green: list[float] = []
    blue_green: list[float] = []
    chroma: list[float] = []
    magenta = 0
    cyan = 0
    bright_chroma = 0
    for red_byte, green_byte, blue_byte in pixels:
        red = red_byte / 255.0
        green = green_byte / 255.0
        blue = blue_byte / 255.0
        luminance = 0.2126 * red + 0.7152 * green + 0.0722 * blue
        rg = red - green
        bg = blue - green
        magnitude = math.hypot(rg, bg)
        luma.append(luminance)
        red_green.append(rg)
        blue_green.append(bg)
        chroma.append(magnitude)
        if luminance > 0.45 and (red + blue) * 0.5 - green > 0.15:
            magenta += 1
        if luminance > 0.45 and (green + blue) * 0.5 - red > 0.15:
            cyan += 1
        if luminance > 0.65 and max(red, green, blue) - min(red, green, blue) > 0.2:
            bright_chroma += 1

    edge_chroma: list[float] = []
    for y in range(1, height - 1):
        row = y * width
        for x in range(1, width - 1):
            index = row + x
            gradient = max(
                abs(luma[index] - luma[index - 1]),
                abs(luma[index] - luma[index + 1]),
                abs(luma[index] - luma[index - width]),
                abs(luma[index] - luma[index + width]),
            )
            if gradient > 0.08:
                edge_chroma.append(chroma[index])

    count = len(pixels)
    return {
        "path": str(path),
        "width": width,
        "height": height,
        "pixels": count,
        "luma_stddev": statistics.pstdev(luma),
        "red_green_stddev": statistics.pstdev(red_green),
        "blue_green_stddev": statistics.pstdev(blue_green),
        "chroma_p95": percentile(chroma, 95),
        "edge_chroma_p95": percentile(edge_chroma, 95),
        "edge_pixels": len(edge_chroma),
        "magenta_outlier_fraction": magenta / count,
        "cyan_outlier_fraction": cyan / count,
        "bright_chroma_fraction": bright_chroma / count,
    }


def ratio(candidate: float, oracle: float) -> float | None:
    return candidate / oracle if oracle != 0.0 else None


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("candidate", type=Path)
    parser.add_argument("oracle", type=Path)
    parser.add_argument("--candidate-roi", type=roi)
    parser.add_argument("--oracle-roi", type=roi)
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()

    candidate = image_metrics(arguments.candidate, arguments.candidate_roi)
    oracle = image_metrics(arguments.oracle, arguments.oracle_roi)
    comparisons = {
        key + "_ratio": ratio(float(candidate[key]), float(oracle[key]))
        for key in (
            "luma_stddev",
            "red_green_stddev",
            "blue_green_stddev",
            "chroma_p95",
            "edge_chroma_p95",
            "magenta_outlier_fraction",
            "cyan_outlier_fraction",
            "bright_chroma_fraction",
        )
    }
    report = {
        "schema_version": 1,
        "candidate": candidate,
        "oracle": oracle,
        "comparison": comparisons,
    }
    serialized = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if arguments.output is not None:
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        arguments.output.write_text(serialized)
    print(serialized, end="")


if __name__ == "__main__":
    main()
