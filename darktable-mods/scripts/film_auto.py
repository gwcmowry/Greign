# /// script
# requires-python = ">=3.12"
# dependencies = [
#   "numpy>=2.1,<3",
#   "pillow>=11,<13",
#   "rawpy>=0.25,<1",
# ]
# ///

"""Generate an explainable automatic negative conversion contact sheet.

This is the executable reference for darktable's native film_analysis module.
It intentionally keeps analysis separate from creative paper/render controls.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import time
from dataclasses import asdict, dataclass, replace
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont, ImageOps
import rawpy

try:
    profile
except NameError:
    def profile(function):
        return function


PAPER_BLACK = 0.0755
PAPER_EXPOSURE = 0.9245
COLOR_PAPER_GRADE = 4.0
MONOCHROME_PAPER_GRADE = 2.5
PAPER_SOFT_CLIP = 0.75


@dataclass(frozen=True)
class AnalysisConfig:
    border_fraction: float = 0.08
    base_percentile: float = 0.95
    density_floor_percentile: float = 0.005
    density_ceiling_percentile: float = 0.995
    maximum_density: float = 6.0
    proxy_sample_limit: int = 1_000_000


@dataclass(frozen=True)
class FilmAnalysis:
    stock: str
    base_source: str
    frame_rect: list[int]
    analysis_rect: list[int]
    geometry_confidence: float
    film_base: list[float]
    density_floor: list[float]
    density_median: list[float]
    density_ceiling: list[float]
    negadoctor_dmax: float
    negadoctor_offset: float
    negadoctor_wb_high: list[float]
    confidence: float
    warnings: list[str]
    sampled_pixels: int
    sampled_border_pixels: int


@dataclass(frozen=True)
class FilmBasePrior:
    film_base: list[float]
    confidence: float
    source: str
    stock: str | None = None


@dataclass(frozen=True)
class RollAnalysis:
    base_mode: str
    film_base: list[float]
    density_floor: list[float]
    density_median: list[float]
    density_ceiling: list[float]
    negadoctor_dmax: float
    negadoctor_wb_high: list[float]
    base_confidence: float
    roll_confidence: float
    frame_count: int
    frames_used: int
    frames_rejected: int
    warnings: list[str]


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


@profile
def _decode_linear(path: Path) -> np.ndarray:
    with rawpy.imread(str(path)) as raw:
        rgb16 = raw.postprocess(
            demosaic_algorithm=rawpy.DemosaicAlgorithm.AHD,
            use_camera_wb=True,
            use_auto_wb=False,
            no_auto_bright=True,
            output_color=rawpy.ColorSpace.ProPhoto,
            output_bps=16,
            gamma=(1.0, 1.0),
        )
    return np.asarray(rgb16, dtype=np.float32) / 65535.0


def _proxy(image: np.ndarray, sample_limit: int) -> np.ndarray:
    height, width, _ = image.shape
    step = max(1, math.ceil(math.sqrt((height * width) / sample_limit)))
    return image[::step, ::step]


def _detect_frame(image: np.ndarray, sample_limit: int) -> tuple[list[int], float]:
    proxy = np.nan_to_num(_proxy(image, sample_limit), nan=0.0, posinf=0.0, neginf=0.0)
    channel_scale = np.quantile(proxy, 0.995, axis=(0, 1))
    display_proxy = np.sqrt(np.clip(proxy / np.maximum(channel_scale, 1.0e-8), 0.0, 1.0))
    gray = np.mean(display_proxy, axis=2)

    def axis_bounds(projection: np.ndarray) -> tuple[int, int, float]:
        kernel_width = max(9, round(len(projection) * 0.018))
        if kernel_width % 2 == 0:
            kernel_width += 1
        smoothed = np.convolve(projection, np.ones(kernel_width) / kernel_width, mode="same")
        gradient = np.abs(np.diff(smoothed))
        margin = max(2, round(len(projection) * 0.01))
        search = max(margin + 2, round(len(projection) * 0.22))
        low = margin + int(np.argmax(gradient[margin:search]))
        high_start = len(projection) - search
        high = high_start + int(np.argmax(gradient[high_start : len(projection) - margin]))
        background = float(np.median(gradient)) + 1.0e-8
        confidence = min(float(gradient[low]), float(gradient[high])) / background
        return low, high, confidence

    left, right, x_confidence = axis_bounds(np.median(gray, axis=0))
    top, bottom, y_confidence = axis_bounds(np.median(gray, axis=1))
    scale_x = image.shape[1] / proxy.shape[1]
    scale_y = image.shape[0] / proxy.shape[0]
    x0, x1 = round(left * scale_x), round(right * scale_x)
    y0, y1 = round(top * scale_y), round(bottom * scale_y)
    if x1 - x0 < image.shape[1] * 0.5 or y1 - y0 < image.shape[0] * 0.5:
        return [0, 0, image.shape[1], image.shape[0]], 0.0
    confidence = float(np.clip(min(x_confidence, y_confidence) / 20.0, 0.0, 1.0))
    return [x0, y0, x1 - x0, y1 - y0], confidence


@profile
def analyze(image: np.ndarray, stock: str, config: AnalysisConfig) -> FilmAnalysis:
    proxy = _proxy(image, config.proxy_sample_limit)
    height, width, _ = proxy.shape
    frame_rect, geometry_confidence = _detect_frame(image, config.proxy_sample_limit)
    frame_x, frame_y, frame_width, frame_height = frame_rect
    inset_x = max(2, round(frame_width * 0.04))
    inset_y = max(2, round(frame_height * 0.04))
    analysis_rect = [
        frame_x + inset_x,
        frame_y + inset_y,
        max(1, frame_width - 2 * inset_x),
        max(1, frame_height - 2 * inset_y),
    ]
    border_x = max(1, round(width * config.border_fraction))
    border_y = max(1, round(height * config.border_fraction))

    border_mask = np.ones((height, width), dtype=bool)
    border_mask[border_y : height - border_y, border_x : width - border_x] = False
    valid = np.all(np.isfinite(proxy) & (proxy > 1.0e-8), axis=2)
    border_values = proxy[border_mask & valid]
    scale_x = width / image.shape[1]
    scale_y = height / image.shape[0]
    analysis_x, analysis_y, analysis_width, analysis_height = analysis_rect
    x0 = max(0, round(analysis_x * scale_x))
    y0 = max(0, round(analysis_y * scale_y))
    x1 = min(width, round((analysis_x + analysis_width) * scale_x))
    y1 = min(height, round((analysis_y + analysis_height) * scale_y))
    frame_valid = valid[y0:y1, x0:x1]
    interior_values = proxy[y0:y1, x0:x1][frame_valid]
    if len(border_values) < 32 or len(interior_values) < 32:
        raise ValueError("not enough valid film samples")

    border_base = np.quantile(border_values, config.base_percentile, axis=0)
    global_base = np.quantile(proxy[valid], 0.98, axis=0)
    border_to_global = float(np.mean(border_base / np.maximum(global_base, 1.0e-8)))
    base_source = "global-highlights" if border_to_global < 0.55 else "outer-rebate"
    base = global_base if base_source == "global-highlights" else border_base
    density = np.log10(base / np.maximum(interior_values, 1.0e-8))
    density = np.clip(density, 0.0, config.maximum_density)
    floor = np.quantile(density, config.density_floor_percentile, axis=0)
    median = np.quantile(density, 0.5, axis=0)
    ceiling = np.quantile(density, config.density_ceiling_percentile, axis=0)
    ranges = np.maximum(ceiling - floor, 0.1)
    dmax = float(np.mean(ranges))
    offset = float(np.mean(floor) / dmax)
    wb_high = dmax / ranges

    warnings: list[str] = []
    if base_source == "global-highlights":
        warnings.append("holder-dominates-border-used-global-highlights")
    if float(np.min(ranges)) < 0.15:
        warnings.append("low-density-range")
    # A clean rebate is expected to be the brightest part of a negative, so
    # comparing it to the frame maximum would flag nearly every good sample.
    # The decoder emits normalized linear RGB; warn only when a base channel
    # itself approaches that representation's ceiling.
    if np.any(base >= 0.995):
        warnings.append("film-base-near-scan-clipping")
    if float(np.mean(np.std(border_values / np.maximum(base, 1.0e-8), axis=0))) > 0.25:
        warnings.append("nonuniform-or-missing-film-rebate")

    border_coverage = min(len(border_values) / 5000.0, 1.0)
    range_quality = float(np.clip(np.min(ranges), 0.0, 1.0))
    confidence = 0.35 * border_coverage + 0.65 * range_quality

    return FilmAnalysis(
        stock=stock,
        base_source=base_source,
        frame_rect=frame_rect,
        analysis_rect=analysis_rect,
        geometry_confidence=geometry_confidence,
        film_base=[float(value) for value in base],
        density_floor=[float(value) for value in floor],
        density_median=[float(value) for value in median],
        density_ceiling=[float(value) for value in ceiling],
        negadoctor_dmax=dmax,
        negadoctor_offset=offset,
        negadoctor_wb_high=[float(value) for value in wb_high],
        confidence=confidence,
        warnings=warnings,
        sampled_pixels=len(interior_values),
        sampled_border_pixels=len(border_values),
    )


def measure_film_base_reference(
    image: np.ndarray,
    sample_limit: int,
    rect: tuple[int, int, int, int] | None = None,
) -> FilmBasePrior:
    if rect is not None:
        x, y, width, height = rect
        if x < 0 or y < 0 or width <= 0 or height <= 0:
            raise ValueError("film-base rectangle must have non-negative origin and positive size")
        if x + width > image.shape[1] or y + height > image.shape[0]:
            raise ValueError("film-base rectangle lies outside the decoded image")
        sample_image = image[y : y + height, x : x + width]
    else:
        sample_image = image
    proxy = np.nan_to_num(_proxy(sample_image, sample_limit), nan=0.0, posinf=0.0, neginf=0.0)
    height, width, _ = proxy.shape
    central = proxy if rect is not None else proxy[
        height // 5 : height * 4 // 5,
        width // 5 : width * 4 // 5,
    ]
    valid = np.all(np.isfinite(central) & (central > 1.0e-8), axis=2)
    samples = central[valid]
    if len(samples) < 32:
        raise ValueError("film-base reference has too few valid pixels")
    film_base = np.median(samples, axis=0)
    relative_spread = float(np.mean(np.std(samples / np.maximum(film_base, 1.0e-8), axis=0)))
    confidence = float(np.clip(1.0 - 2.5 * relative_spread, 0.2, 1.0))
    return FilmBasePrior(
        film_base=[float(value) for value in film_base],
        confidence=confidence,
        source="explicit-film-base-reference",
    )


def analyze_roll(
    frames: list[FilmAnalysis],
    explicit_base: FilmBasePrior | None = None,
    borrowed_prior: FilmBasePrior | None = None,
) -> RollAnalysis:
    if not frames:
        raise ValueError("roll analysis requires at least one frame")
    stocks = {frame.stock for frame in frames}
    if len(stocks) != 1:
        raise ValueError("roll analysis cannot mix color and monochrome film")
    accepted = [
        frame
        for frame in frames
        if frame.confidence >= 0.2 and "film-base-near-scan-clipping" not in frame.warnings
    ]
    if not accepted:
        raise ValueError("roll analysis rejected every frame")
    measured = [
        frame
        for frame in accepted
        if frame.base_source == "outer-rebate"
        and "nonuniform-or-missing-film-rebate" not in frame.warnings
    ]
    warnings: list[str] = []

    if explicit_base is not None:
        base_mode = "explicit-reference"
        film_base = np.asarray(explicit_base.film_base, dtype=np.float64)
        base_confidence = float(np.clip(explicit_base.confidence, 0.0, 1.0))
        if base_confidence < 0.6:
            warnings.append("film-base-reference-nonuniform")
    elif measured:
        base_mode = "measured-rebate"
        film_base = np.median([frame.film_base for frame in measured], axis=0)
        base_confidence = min(0.95, 0.70 + 0.05 * len(measured))
    elif len(accepted) >= 3:
        base_mode = "inferred-roll-envelope"
        film_base = np.quantile([frame.film_base for frame in accepted], 0.75, axis=0)
        base_confidence = min(0.85, 0.45 + 0.05 * len(accepted))
        warnings.append("film-base-inferred-no-blank-frame")
    elif borrowed_prior is not None:
        if borrowed_prior.stock is not None and borrowed_prior.stock not in stocks:
            raise ValueError("film-base prior stock does not match the roll")
        base_mode = "borrowed-prior"
        film_base = np.asarray(borrowed_prior.film_base, dtype=np.float64)
        base_confidence = min(max(borrowed_prior.confidence, 0.0), 0.65)
        warnings.append("film-base-borrowed-verify-scan-setup")
    else:
        base_mode = "per-frame-fallback"
        film_base = np.asarray(accepted[0].film_base, dtype=np.float64)
        base_confidence = min(accepted[0].confidence, 0.4)
        warnings.append("film-base-per-frame-low-confidence")

    floors = np.median([frame.density_floor for frame in accepted], axis=0)
    medians = np.median([frame.density_median for frame in accepted], axis=0)
    ceilings = np.median([frame.density_ceiling for frame in accepted], axis=0)
    dmax = float(np.median([frame.negadoctor_dmax for frame in accepted]))
    wb_high = np.median([frame.negadoctor_wb_high for frame in accepted], axis=0)
    rejected = len(frames) - len(accepted)
    if rejected:
        warnings.append(f"rejected-{rejected}-frames")
    coverage = len(accepted) / len(frames)
    return RollAnalysis(
        base_mode=base_mode,
        film_base=[float(value) for value in film_base],
        density_floor=[float(value) for value in floors],
        density_median=[float(value) for value in medians],
        density_ceiling=[float(value) for value in ceilings],
        negadoctor_dmax=dmax,
        negadoctor_wb_high=[float(value) for value in wb_high],
        base_confidence=base_confidence,
        roll_confidence=base_confidence * (0.6 + 0.4 * coverage),
        frame_count=len(frames),
        frames_used=len(accepted),
        frames_rejected=rejected,
        warnings=warnings,
    )


def apply_roll_baseline(frame: FilmAnalysis, roll: RollAnalysis) -> FilmAnalysis:
    frame_base = np.asarray(frame.film_base, dtype=np.float64)
    shared_base = np.asarray(roll.film_base, dtype=np.float64)
    base_shift = np.log10(shared_base / np.maximum(frame_base, 1.0e-8))
    shifted_floor = np.asarray(frame.density_floor) + base_shift
    shifted_median = np.asarray(frame.density_median) + base_shift

    roll_floor = np.asarray(roll.density_floor)
    roll_median = np.asarray(roll.density_median)
    roll_ceiling = np.asarray(roll.density_ceiling)
    roll_ranges = np.maximum(roll_ceiling - roll_floor, 0.1)
    color_floor_offsets = roll_floor - np.mean(roll_floor)
    color_median_offsets = roll_median - np.mean(roll_median)
    frame_floor = np.mean(shifted_floor) + color_floor_offsets
    frame_median = np.mean(shifted_median) + color_median_offsets
    frame_ceiling = frame_floor + roll_ranges
    dmax = float(np.mean(roll_ranges))
    wb_high = dmax / roll_ranges
    return replace(
        frame,
        base_source=f"roll:{roll.base_mode}",
        film_base=[float(value) for value in shared_base],
        density_floor=[float(value) for value in frame_floor],
        density_median=[float(value) for value in frame_median],
        density_ceiling=[float(value) for value in frame_ceiling],
        negadoctor_dmax=dmax,
        negadoctor_offset=float(np.mean(frame_floor) / max(dmax, 0.1)),
        negadoctor_wb_high=[float(value) for value in wb_high],
        confidence=min(frame.confidence, roll.roll_confidence),
        warnings=[*frame.warnings, *roll.warnings],
    )


@profile
def convert(image: np.ndarray, analysis: FilmAnalysis, tile_rows: int = 512) -> np.ndarray:
    paper_grade = MONOCHROME_PAPER_GRADE if analysis.stock == "monochrome" else COLOR_PAPER_GRADE
    base = np.asarray(analysis.film_base, dtype=np.float32)
    floor = np.asarray(analysis.density_floor, dtype=np.float32)
    median = np.asarray(analysis.density_median, dtype=np.float32)
    ceiling = np.asarray(analysis.density_ceiling, dtype=np.float32)
    ranges = np.maximum(ceiling - floor, 0.1)

    median_normalized = np.clip((median - floor) / ranges, 0.0, 1.0)
    median_positive = (1.0 - np.power(10.0, -median_normalized) + PAPER_BLACK) * PAPER_EXPOSURE
    median_positive = np.power(np.maximum(median_positive, 0.0), paper_grade)
    target = float(np.mean(median_positive))
    gains = np.clip(target / np.maximum(median_positive, 1.0e-5), 0.90, 1.10)
    luminance_weights = np.asarray([0.2126, 0.7152, 0.0722], dtype=np.float32)

    height, width, _ = image.shape
    output = np.empty((height, width, 3), dtype=np.uint8)
    for start in range(0, height, tile_rows):
        stop = min(start + tile_rows, height)
        source = image[start:stop]
        density = np.maximum(source, 1.0e-8)
        density = np.log10(base / density)
        density -= floor
        density /= ranges
        np.clip(density, 0.0, 1.0, out=density)

        np.negative(density, out=density)
        positive = np.power(10.0, density)
        positive *= -1.0
        positive += 1.0 + PAPER_BLACK
        positive *= PAPER_EXPOSURE
        np.maximum(positive, 0.0, out=positive)
        np.power(positive, paper_grade, out=positive)
        soft_mask = positive > PAPER_SOFT_CLIP
        soft_values = positive[soft_mask]
        soft_values = PAPER_SOFT_CLIP + (
            1.0 - np.exp(-(soft_values - PAPER_SOFT_CLIP) / (1.0 - PAPER_SOFT_CLIP))
        ) * (1.0 - PAPER_SOFT_CLIP)
        positive[soft_mask] = soft_values

        if analysis.stock == "monochrome":
            monochrome = np.sum(positive * luminance_weights, axis=2)
            positive = np.broadcast_to(monochrome[:, :, None], positive.shape).copy()
        else:
            positive *= gains
            np.clip(positive, 0.0, 1.0, out=positive)

        # The virtual paper above is linear-light; encode it for display.
        linear_mask = positive <= 0.0031308
        linear_values = positive[linear_mask] * 12.92
        np.power(positive, 1.0 / 2.4, out=positive)
        positive *= 1.055
        positive -= 0.055
        positive[linear_mask] = linear_values
        np.clip(positive, 0.0, 1.0, out=positive)
        output[start:stop] = np.round(positive * 255.0).astype(np.uint8)
    return output


def _display_negative(image: np.ndarray) -> np.ndarray:
    step = max(1, math.ceil(max(image.shape[:2]) / 1800))
    image = image[::step, ::step]
    high = np.quantile(image, 0.999, axis=(0, 1))
    normalized = np.clip(image / np.maximum(high, 1.0e-8), 0.0, 1.0)
    return np.power(normalized, 1.0 / 2.2)


def _to_image(array: np.ndarray) -> Image.Image:
    if array.dtype == np.uint8:
        return Image.fromarray(array, "RGB")
    return Image.fromarray(np.round(np.clip(array, 0.0, 1.0) * 255.0).astype(np.uint8), "RGB")


def _preview(image: Image.Image, maximum: int = 1800) -> Image.Image:
    output = image.copy()
    output.thumbnail((maximum, maximum), Image.Resampling.LANCZOS)
    return output


def _analysis_overlay(
    image: Image.Image,
    source_size: tuple[int, int],
    frame_rect: list[int],
    analysis_rect: list[int],
) -> Image.Image:
    output = image.copy()
    draw = ImageDraw.Draw(output)
    width, height = output.size
    source_width, source_height = source_size
    scale_x, scale_y = width / source_width, height / source_height
    line_width = max(2, round(max(width, height) / 500))

    def scaled(rect: list[int]) -> tuple[int, int, int, int]:
        x, y, rect_width, rect_height = rect
        return (
            round(x * scale_x),
            round(y * scale_y),
            round((x + rect_width) * scale_x),
            round((y + rect_height) * scale_y),
        )

    frame = scaled(frame_rect)
    analysis = scaled(analysis_rect)
    draw.rectangle(frame, outline=(255, 205, 40), width=line_width)
    draw.rectangle(analysis, outline=(0, 255, 80), width=line_width)
    draw.text((analysis[0] + 8, analysis[1] + 8), "green: density analysis", fill=(0, 255, 80))
    draw.text((frame[0] + 8, frame[1] + 8), "yellow: detected frame", fill=(255, 205, 40))
    return output


def process_one(
    path: Path,
    output_dir: Path,
    config: AnalysisConfig,
    analysis_override: FilmAnalysis | None = None,
) -> dict[str, object]:
    started = time.perf_counter()
    stock = "monochrome" if path.name.lower().startswith("bw-") else "color"
    decode_started = time.perf_counter()
    linear = _decode_linear(path)
    decode_seconds = time.perf_counter() - decode_started

    analysis_started = time.perf_counter()
    result = analysis_override if analysis_override is not None else analyze(linear, stock, config)
    analysis_seconds = time.perf_counter() - analysis_started

    render_started = time.perf_counter()
    positive = convert(linear, result)
    render_seconds = time.perf_counter() - render_started

    stem = path.stem.lower()
    negative_image = _preview(_to_image(_display_negative(linear)))
    positive_image = _preview(_to_image(positive))
    overlay_image = _analysis_overlay(
        negative_image,
        (linear.shape[1], linear.shape[0]),
        result.frame_rect,
        result.analysis_rect,
    )
    negative_path = output_dir / f"{stem}-negative.jpg"
    positive_path = output_dir / f"{stem}-auto-positive.jpg"
    cropped_positive_path = output_dir / f"{stem}-auto-positive-cropped.jpg"
    overlay_path = output_dir / f"{stem}-analysis-mask.jpg"
    negative_image.save(negative_path, quality=94, subsampling=0)
    positive_image.save(positive_path, quality=95, subsampling=0)
    frame_x, frame_y, frame_width, frame_height = result.frame_rect
    full_positive = _to_image(positive)
    cropped_positive = full_positive.crop(
        (frame_x, frame_y, frame_x + frame_width, frame_y + frame_height)
    )
    _preview(cropped_positive).save(cropped_positive_path, quality=95, subsampling=0)
    overlay_image.save(overlay_path, quality=94, subsampling=0)

    report: dict[str, object] = {
        "source": str(path),
        "source_sha256": _sha256(path),
        "dimensions": {"width": int(linear.shape[1]), "height": int(linear.shape[0])},
        "config": asdict(config),
        "analysis": asdict(result),
        "render_intent": {
            "name": "negadoctor-neutral-paper-v1",
            "paper_black": PAPER_BLACK,
            "paper_exposure": PAPER_EXPOSURE,
            "paper_grade": (
                MONOCHROME_PAPER_GRADE if result.stock == "monochrome" else COLOR_PAPER_GRADE
            ),
            "paper_soft_clip": PAPER_SOFT_CLIP,
        },
        "outputs": {
            "negative": str(negative_path),
            "positive": str(positive_path),
            "cropped_positive": str(cropped_positive_path),
            "analysis_mask": str(overlay_path),
        },
        "timing_seconds": {
            "decode": decode_seconds,
            "analysis": analysis_seconds,
            "render": render_seconds,
            "total": time.perf_counter() - started,
        },
    }
    report_path = output_dir / f"{stem}-analysis.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    return report


def _contact_sheet(reports: list[dict[str, object]], output_dir: Path) -> None:
    cards: list[Image.Image] = []
    for report in reports:
        outputs = report["outputs"]
        analysis = report["analysis"]
        source = Path(str(report["source"])).name
        positive = Image.open(str(outputs["cropped_positive"])).convert("RGB")
        positive = ImageOps.contain(positive, (900, 620), Image.Resampling.LANCZOS)
        card = Image.new("RGB", (940, 720), (28, 28, 28))
        card.paste(positive, ((940 - positive.width) // 2, 12))
        draw = ImageDraw.Draw(card)
        warning_text = ", ".join(analysis["warnings"]) or "none"
        label = (
            f"{source}\n"
            f"{analysis['stock']} | confidence {analysis['confidence']:.2f} | "
            f"Dmax {analysis['negadoctor_dmax']:.3f}\n"
            f"warnings: {warning_text}"
        )
        draw.multiline_text((18, 642), label, fill=(235, 235, 235), spacing=4)
        cards.append(card)

    columns = 2
    rows = math.ceil(len(cards) / columns)
    sheet = Image.new("RGB", (columns * 940, rows * 720), (12, 12, 12))
    for index, card in enumerate(cards):
        sheet.paste(card, ((index % columns) * 940, (index // columns) * 720))
    sheet.save(output_dir / "contact-sheet.jpg", quality=95, subsampling=0)


def _comparison_sheet(
    reports: list[dict[str, object]], reference_dir: Path, output_dir: Path
) -> None:
    rows: list[Image.Image] = []
    for report in reports:
        stem = Path(str(report["source"])).stem.lower()
        reference_path = reference_dir / f"{stem}-reference.jpg"
        if not reference_path.is_file():
            continue
        auto = Image.open(str(report["outputs"]["cropped_positive"])).convert("RGB")
        reference = Image.open(reference_path).convert("RGB")
        auto = ImageOps.contain(auto, (880, 620), Image.Resampling.LANCZOS)
        reference = ImageOps.contain(reference, (880, 620), Image.Resampling.LANCZOS)
        row = Image.new("RGB", (1800, 700), (20, 20, 20))
        row.paste(auto, ((900 - auto.width) // 2, 8))
        row.paste(reference, (900 + (900 - reference.width) // 2, 8))
        draw = ImageDraw.Draw(row)
        draw.text((18, 650), f"AUTO: {stem}", fill=(235, 235, 235))
        draw.text((918, 650), "COMMUNITY XMP REFERENCE (creative, not ground truth)", fill=(235, 235, 235))
        rows.append(row)
    if not rows:
        return
    sheet = Image.new("RGB", (1800, 700 * len(rows)), (12, 12, 12))
    for index, row in enumerate(rows):
        sheet.paste(row, (0, index * 700))
    sheet.save(output_dir / "correctness-comparison.jpg", quality=95, subsampling=0)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inputs", nargs="+", type=Path)
    parser.add_argument("--output-dir", type=Path, default=Path("artifacts/film-conversions"))
    parser.add_argument("--reference-dir", type=Path)
    parser.add_argument(
        "--roll-analysis",
        action="store_true",
        help="derive shared roll color/range statistics before rendering",
    )
    parser.add_argument(
        "--film-base-reference",
        type=Path,
        help="optional RAW containing a clean unexposed film-base area",
    )
    parser.add_argument(
        "--film-base-rect",
        nargs=4,
        type=int,
        metavar=("X", "Y", "WIDTH", "HEIGHT"),
        help="decoded-image rectangle selecting a small clean patch in the reference RAW",
    )
    parser.add_argument(
        "--base-prior",
        type=Path,
        help="optional JSON prior with film_base, confidence, and source",
    )
    args = parser.parse_args()
    if args.film_base_rect and not args.film_base_reference:
        parser.error("--film-base-rect requires --film-base-reference")
    args.output_dir.mkdir(parents=True, exist_ok=True)

    config = AnalysisConfig()
    roll: RollAnalysis | None = None
    roll_analysis_enabled = bool(
        args.roll_analysis or args.film_base_reference or args.base_prior
    )
    if roll_analysis_enabled:
        initial_analyses: list[FilmAnalysis] = []
        for path in args.inputs:
            linear = _decode_linear(path)
            stock = "monochrome" if path.name.lower().startswith("bw-") else "color"
            initial_analyses.append(analyze(linear, stock, config))

        explicit_base = None
        if args.film_base_reference:
            explicit_base = measure_film_base_reference(
                _decode_linear(args.film_base_reference),
                config.proxy_sample_limit,
                tuple(args.film_base_rect) if args.film_base_rect else None,
            )
        borrowed_prior = None
        if args.base_prior:
            prior_data = json.loads(args.base_prior.read_text())
            borrowed_prior = FilmBasePrior(
                film_base=[float(value) for value in prior_data["film_base"]],
                confidence=float(prior_data["confidence"]),
                source=str(prior_data.get("source", args.base_prior)),
                stock=str(prior_data["stock"]) if "stock" in prior_data else None,
            )
        roll = analyze_roll(initial_analyses, explicit_base, borrowed_prior)
        adjusted = [apply_roll_baseline(frame, roll) for frame in initial_analyses]
        reports = [
            process_one(path, args.output_dir, config, analysis_override=frame)
            for path, frame in zip(args.inputs, adjusted, strict=True)
        ]
        (args.output_dir / "roll-analysis.json").write_text(
            json.dumps(asdict(roll), indent=2) + "\n"
        )
        prior_path = args.output_dir / "film-base-prior.json"
        if (
            roll.base_mode in {"explicit-reference", "measured-rebate"}
            and roll.base_confidence >= 0.6
        ):
            reusable_prior = {
                "schema_version": 1,
                "film_base": roll.film_base,
                "confidence": roll.base_confidence,
                "source": f"greign-{roll.base_mode}",
                "stock": initial_analyses[0].stock,
                "constraint": "reuse only with the same film stock and unchanged capture setup",
            }
            prior_path.write_text(json.dumps(reusable_prior, indent=2) + "\n")
        else:
            # Do not leave a reusable-looking prior from an older run in an
            # output directory whose current calibration did not qualify.
            prior_path.unlink(missing_ok=True)
    else:
        reports = [process_one(path, args.output_dir, config) for path in args.inputs]
    _contact_sheet(reports, args.output_dir)
    if args.reference_dir:
        _comparison_sheet(reports, args.reference_dir, args.output_dir)
    summary = {
        "schema_version": 1,
        "generated_at_unix": time.time(),
        "roll_analysis": asdict(roll) if roll else None,
        "images": reports,
    }
    (args.output_dir / "run-report.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps({
        "processed": len(reports),
        "output_dir": str(args.output_dir),
        "contact_sheet": str(args.output_dir / "contact-sheet.jpg"),
    }, indent=2))


if __name__ == "__main__":
    main()
