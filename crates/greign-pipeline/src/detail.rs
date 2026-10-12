//! Initial capture-detail operations.

use crate::{LinearRgbImage, PipelineError};
use rayon::prelude::*;

/// Applies a three-by-three luminance unsharp mask in linear Rec.2020.
///
/// Values are intentionally not clamped; the display transform owns gamut and
/// output bounds. The operation requires a one-pixel tile halo.
///
/// # Errors
///
/// Returns [`PipelineError::InvalidSharpening`] outside the conservative
/// `[0, 2]` amount range.
pub fn capture_sharpen(image: &mut LinearRgbImage, amount: f32) -> Result<(), PipelineError> {
    if !amount.is_finite() || !(0.0..=2.0).contains(&amount) {
        return Err(PipelineError::InvalidSharpening);
    }
    if amount == 0.0 {
        return Ok(());
    }
    let width = image.width as usize;
    let height = image.height as usize;
    if width == 0 || height == 0 || image.pixels.len() != width * height {
        return Err(PipelineError::InvalidDimensions);
    }
    let source: Vec<_> = image.pixels.par_iter().map(|p| luminance(*p)).collect();
    image
        .pixels
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, pixel) in row.iter_mut().enumerate() {
                let mut blur = 0.0;
                let mut count = 0_u8;
                for ny in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                    for nx in x.saturating_sub(1)..=(x + 1).min(width - 1) {
                        blur += source[ny * width + nx];
                        count += 1;
                    }
                }
                let detail = source[y * width + x] - blur / f32::from(count);
                *pixel = pixel.map(|v| v + amount * detail);
            }
        });
    Ok(())
}

/// Rec.2020 linear luminance.
#[must_use]
pub fn luminance(pixel: [f32; 3]) -> f32 {
    0.262_700_2 * pixel[0] + 0.677_998_07 * pixel[1] + 0.059_301_715 * pixel[2]
}

/// Mild edge-aware chroma filtering, preserving linear luminance and signed RGB.
///
/// # Errors
/// Returns an error for strength outside zero through one, or invalid storage.
pub fn chroma_denoise(image: &mut LinearRgbImage, strength: f32) -> Result<(), PipelineError> {
    if !strength.is_finite() || !(0.0..=1.0).contains(&strength) {
        return Err(PipelineError::NonFiniteParameter);
    }
    if image.width == 0
        || image.height == 0
        || image.pixels.len() != image.width as usize * image.height as usize
    {
        return Err(PipelineError::InvalidDimensions);
    }
    if strength == 0.0 {
        return Ok(());
    }
    let width = image.width as usize;
    let height = image.height as usize;
    // Reuse luminance/chroma across all 25 neighbor visits.
    let source: Vec<_> = image
        .pixels
        .par_iter()
        .map(|p| {
            let y = luminance(*p);
            [y, p[0] - y, p[2] - y]
        })
        .collect();
    image
        .pixels
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, pixel) in row.iter_mut().enumerate() {
                let [luma, red, blue] = source[y * width + x];
                let variance = 0.000_025 + 0.000_4 * luma.max(0.0);
                let mut sum = [0.0; 2];
                let mut total = 0.0;
                for ny in y.saturating_sub(2)..=(y + 2).min(height - 1) {
                    for nx in x.saturating_sub(2)..=(x + 2).min(width - 1) {
                        let [nl, nr, nb] = source[ny * width + nx];
                        let dy = nl - luma;
                        let dc = (nr - red).powi(2) + (nb - blue).powi(2);
                        let weight = 1.0 / (1.0 + dy * dy / variance + dc / (4.0 * variance));
                        sum[0] += weight * nr;
                        sum[1] += weight * nb;
                        total += weight;
                    }
                }
                let dr = strength * (sum[0] / total - red);
                let db = strength * (sum[1] / total - blue);
                pixel[0] += dr;
                pixel[2] += db;
                pixel[1] -= (0.262_700_2 * dr + 0.059_301_715 * db) / 0.677_998_07;
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{capture_sharpen, chroma_denoise, luminance};
    use crate::{LinearRgbImage, PipelineError};

    #[test]
    fn constant_field_is_unchanged() {
        let mut image = LinearRgbImage {
            width: 3,
            height: 3,
            pixels: vec![[0.25; 3]; 9],
        };
        capture_sharpen(&mut image, 0.5).expect("sharpen");
        assert!(
            image
                .pixels
                .iter()
                .flatten()
                .all(|value| (*value - 0.25).abs() < 1.0e-6)
        );
    }

    #[test]
    fn impulse_gains_local_contrast_without_clamp() {
        let mut image = LinearRgbImage {
            width: 3,
            height: 3,
            pixels: vec![[0.0; 3]; 9],
        };
        image.pixels[4] = [1.0; 3];
        capture_sharpen(&mut image, 0.5).expect("sharpen");
        assert!(image.pixels[4][0] > 1.0);
        assert!(image.pixels[0][0] < 0.0);
    }

    #[test]
    fn invalid_amount_is_rejected() {
        let mut image = LinearRgbImage {
            width: 1,
            height: 1,
            pixels: vec![[0.0; 3]],
        };
        assert_eq!(
            capture_sharpen(&mut image, -0.1),
            Err(PipelineError::InvalidSharpening)
        );
    }

    #[test]
    fn chroma_filter_reduces_noise_without_smoothing_luminance() {
        let mut image = LinearRgbImage {
            width: 31,
            height: 29,
            pixels: (0..899)
                .map(|i| {
                    let n = f32::from(u8::try_from((i * 37) % 19).unwrap()) * 0.001 - 0.009;
                    [0.2 + n, 0.2 - n * 0.262_700_2 / 0.677_998_07, 0.2]
                })
                .collect(),
        };
        let original = image.clone();
        chroma_denoise(&mut image, 0.0).unwrap();
        assert_eq!(image, original);
        chroma_denoise(&mut image, 0.5).unwrap();
        let energy = |image: &LinearRgbImage| {
            image
                .pixels
                .iter()
                .map(|p| (p[0] - p[1]).powi(2))
                .sum::<f32>()
        };
        assert!(energy(&image) < energy(&original) * 0.6);
        for (a, b) in image.pixels.iter().zip(&original.pixels) {
            assert!((luminance(*a) - luminance(*b)).abs() < 1.0e-6);
        }
        let chroma: Vec<_> = image
            .pixels
            .iter()
            .map(|p| [p[0] - p[1], p[2] - p[1]])
            .collect();
        capture_sharpen(&mut image, 0.2).unwrap();
        for (p, c) in image.pixels.iter().zip(chroma) {
            assert!((p[0] - p[1] - c[0]).abs() < 1.0e-6);
            assert!((p[2] - p[1] - c[1]).abs() < 1.0e-6);
        }
    }

    #[test]
    fn small_saturated_color_detail_survives_chroma_filter() {
        let mut image = LinearRgbImage {
            width: 9,
            height: 9,
            pixels: vec![[0.1; 3]; 81],
        };
        image.pixels[40] = [0.7, 0.02, 0.01];
        let before = image.pixels[40];
        chroma_denoise(&mut image, 0.5).unwrap();
        assert!((image.pixels[40][0] - before[0]).abs() < 0.02);
        assert!(chroma_denoise(&mut image, f32::NAN).is_err());
    }
}
