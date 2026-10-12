# /// script
# requires-python = ">=3.12"
# dependencies = [
#   "numpy>=2.1,<3",
#   "pillow>=11,<13",
#   "rawpy>=0.25,<1",
# ]
# ///

from __future__ import annotations

import sys
import unittest
from dataclasses import replace
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
from film_auto import (
    AnalysisConfig,
    FilmBasePrior,
    _detect_frame,
    analyze,
    analyze_roll,
    apply_roll_baseline,
    convert,
    measure_film_base_reference,
)


def synthetic_negative(
    width: int = 180,
    height: int = 120,
    base: tuple[float, float, float] = (0.92, 0.61, 0.33),
) -> np.ndarray:
    image = np.empty((height, width, 3), dtype=np.float32)
    image[:] = base
    x_density = np.linspace(0.08, 1.8, width - 24, dtype=np.float32)
    density = np.broadcast_to(x_density, (height - 24, width - 24))
    image[12:-12, 12:-12] = np.asarray(base) * np.power(10.0, -density[:, :, None])
    return image


class FilmAutoCorrectnessTests(unittest.TestCase):
    def test_analysis_recovers_known_film_base_and_density_range(self) -> None:
        base = np.asarray((0.92, 0.61, 0.33))
        result = analyze(synthetic_negative(base=tuple(base)), "color", AnalysisConfig())
        np.testing.assert_allclose(result.film_base, base, atol=0.003)
        self.assertEqual(result.base_source, "outer-rebate")
        self.assertNotIn("film-base-near-scan-clipping", result.warnings)
        self.assertGreater(result.negadoctor_dmax, 1.3)
        self.assertLess(result.negadoctor_dmax, 1.9)

    def test_film_base_at_decoder_ceiling_is_reported_as_clipped(self) -> None:
        result = analyze(synthetic_negative(base=(1.0, 0.61, 0.33)), "color", AnalysisConfig())
        self.assertIn("film-base-near-scan-clipping", result.warnings)

    def test_dark_holder_falls_back_without_becoming_film_base(self) -> None:
        source = synthetic_negative()
        padded = np.full((180, 240, 3), 0.002, dtype=np.float32)
        padded[30:150, 30:210] = source
        result = analyze(padded, "color", AnalysisConfig())
        self.assertEqual(result.base_source, "global-highlights")
        np.testing.assert_allclose(result.film_base, (0.92, 0.61, 0.33), atol=0.01)
        self.assertIn("holder-dominates-border-used-global-highlights", result.warnings)

    def test_frame_detection_finds_image_inside_holder(self) -> None:
        source = synthetic_negative()
        padded = np.full((180, 240, 3), 0.002, dtype=np.float32)
        padded[30:150, 30:210] = source
        rect, confidence = _detect_frame(padded, AnalysisConfig().proxy_sample_limit)
        np.testing.assert_allclose(rect, (30, 30, 180, 120), atol=5)
        self.assertGreater(confidence, 0.25)

    def test_conversion_is_monotonic_in_negative_density(self) -> None:
        result = analyze(synthetic_negative(), "color", AnalysisConfig())
        base = np.asarray(result.film_base, dtype=np.float32)
        samples = np.asarray([[base, base * 0.01]], dtype=np.float32)
        positive = convert(samples, result)
        self.assertTrue(np.all(positive[0, 0] < positive[0, 1]))

    def test_monochrome_output_channels_are_identical(self) -> None:
        source = synthetic_negative(base=(0.88, 0.71, 0.56))
        result = analyze(source, "monochrome", AnalysisConfig())
        positive = convert(source, result)
        np.testing.assert_array_equal(positive[:, :, 0], positive[:, :, 1])
        np.testing.assert_array_equal(positive[:, :, 1], positive[:, :, 2])
        # The neutral virtual paper intentionally leaves headroom rather than
        # mapping its brightest reproducible tone to display white.
        self.assertGreater(int(positive.max()) - int(positive.min()), 190)

    def test_invalid_pixels_do_not_poison_analysis(self) -> None:
        source = synthetic_negative()
        source[20:30, 20:30] = np.nan
        source[40:50, 40:50] = np.inf
        result = analyze(source, "color", AnalysisConfig())
        self.assertTrue(np.isfinite(result.negadoctor_dmax))
        self.assertTrue(all(np.isfinite(result.film_base)))

    def test_roll_uses_small_measured_rebate_without_blank_frame(self) -> None:
        frame = analyze(synthetic_negative(), "color", AnalysisConfig())
        frames = [replace(frame, film_base=[0.90, 0.60, 0.32]), frame]
        roll = analyze_roll(frames)
        self.assertEqual(roll.base_mode, "measured-rebate")
        self.assertAlmostEqual(roll.base_confidence, 0.8)

    def test_explicit_base_can_use_a_small_patch_inside_an_ordinary_scan(self) -> None:
        source = synthetic_negative()
        prior = measure_film_base_reference(
            source,
            AnalysisConfig().proxy_sample_limit,
            rect=(0, 0, 18, 10),
        )
        np.testing.assert_allclose(prior.film_base, (0.92, 0.61, 0.33), atol=0.003)
        self.assertEqual(prior.source, "explicit-film-base-reference")

    def test_nonuniform_explicit_patch_is_not_silently_trusted(self) -> None:
        source = synthetic_negative()
        source[0:10, 9:18] *= 0.25
        prior = measure_film_base_reference(
            source,
            AnalysisConfig().proxy_sample_limit,
            rect=(0, 0, 18, 10),
        )
        frame = analyze(synthetic_negative(), "color", AnalysisConfig())
        roll = analyze_roll([frame], explicit_base=prior)
        self.assertLess(roll.base_confidence, 0.6)
        self.assertIn("film-base-reference-nonuniform", roll.warnings)

    def test_roll_infers_base_when_every_frame_lacks_rebate(self) -> None:
        frame = analyze(synthetic_negative(), "color", AnalysisConfig())
        bases = (0.82, 0.87, 0.91, 0.94)
        frames = [
            replace(
                frame,
                base_source="global-highlights",
                film_base=[base, base * 0.65, base * 0.35],
                warnings=["nonuniform-or-missing-film-rebate"],
            )
            for base in bases
        ]
        roll = analyze_roll(frames)
        self.assertEqual(roll.base_mode, "inferred-roll-envelope")
        self.assertGreater(roll.film_base[0], 0.89)
        self.assertLess(roll.film_base[0], 0.94)
        self.assertIn("film-base-inferred-no-blank-frame", roll.warnings)

    def test_borrowed_prior_is_labeled_and_confidence_capped(self) -> None:
        frame = analyze(synthetic_negative(), "color", AnalysisConfig())
        frame = replace(
            frame,
            base_source="global-highlights",
            warnings=["nonuniform-or-missing-film-rebate"],
        )
        prior = FilmBasePrior([0.91, 0.59, 0.32], 0.9, "same-stock-same-session")
        roll = analyze_roll([frame], borrowed_prior=prior)
        self.assertEqual(roll.base_mode, "borrowed-prior")
        self.assertLessEqual(roll.base_confidence, 0.65)

    def test_borrowed_prior_rejects_a_different_film_type(self) -> None:
        frame = analyze(synthetic_negative(), "color", AnalysisConfig())
        frame = replace(
            frame,
            base_source="global-highlights",
            warnings=["nonuniform-or-missing-film-rebate"],
        )
        prior = FilmBasePrior([0.91, 0.59, 0.32], 0.9, "wrong-stock", "monochrome")
        with self.assertRaisesRegex(ValueError, "stock does not match"):
            analyze_roll([frame], borrowed_prior=prior)

    def test_roll_baseline_shares_color_range_but_keeps_frame_luma(self) -> None:
        frame = analyze(synthetic_negative(), "color", AnalysisConfig())
        darker = replace(
            frame,
            density_floor=[value + 0.2 for value in frame.density_floor],
            density_median=[value + 0.2 for value in frame.density_median],
            density_ceiling=[value + 0.2 for value in frame.density_ceiling],
        )
        roll = analyze_roll([frame, darker])
        adjusted_light = apply_roll_baseline(frame, roll)
        adjusted_dark = apply_roll_baseline(darker, roll)
        light_ranges = np.subtract(adjusted_light.density_ceiling, adjusted_light.density_floor)
        dark_ranges = np.subtract(adjusted_dark.density_ceiling, adjusted_dark.density_floor)
        np.testing.assert_allclose(light_ranges, dark_ranges)
        self.assertNotAlmostEqual(
            float(np.mean(adjusted_light.density_floor)),
            float(np.mean(adjusted_dark.density_floor)),
        )

    def test_roll_rejects_mixed_color_and_monochrome(self) -> None:
        color = analyze(synthetic_negative(), "color", AnalysisConfig())
        monochrome = replace(color, stock="monochrome")
        with self.assertRaisesRegex(ValueError, "cannot mix"):
            analyze_roll([color, monochrome])


if __name__ == "__main__":
    unittest.main()
