//! Bilinear and pattern-pixel-grouping (PPG) Bayer demosaic implementations.

use crate::{CfaColor, LinearRgbImage, NormalizedMosaic, PipelineError, active_crop};

use rayon::prelude::*;

const PPG_RADIUS: u32 = 3;
const PPG_TILE_HALO: u32 = PPG_RADIUS + 1;

/// Demosaics a 2×2 Bayer mosaic using local bilinear averages.
///
/// # Errors
///
/// Returns [`PipelineError::UnsupportedCfa`] for non-Bayer patterns.
pub fn bilinear_demosaic(mosaic: &NormalizedMosaic) -> Result<LinearRgbImage, PipelineError> {
    require_bayer(mosaic, "reference")?;
    let mut pixels = Vec::with_capacity(mosaic.samples.len());
    for y in 0..mosaic.height {
        for x in 0..mosaic.width {
            pixels.push([
                interpolate(mosaic, x, y, CfaColor::Red),
                interpolate(mosaic, x, y, CfaColor::Green),
                interpolate(mosaic, x, y, CfaColor::Blue),
            ]);
        }
    }
    Ok(LinearRgbImage {
        width: mosaic.width,
        height: mosaic.height,
        pixels,
    })
}

/// Pattern-pixel-grouping Bayer demosaic.
///
/// This is derived from Darktable's GPLv3 PPG implementation: green at red
/// and blue sites is selected from a clamped directional estimate, then the
/// missing red and blue channels are reconstructed from color differences.
///
/// # Errors
///
/// Returns [`PipelineError::UnsupportedCfa`] for non-Bayer patterns.
pub fn ppg_demosaic(mosaic: &NormalizedMosaic) -> Result<LinearRgbImage, PipelineError> {
    use CfaColor::{Blue, Green, Red};
    require_bayer(mosaic, "PPG")?;
    let width = mosaic.width as usize;
    let height = mosaic.height as usize;
    if width.checked_mul(height) != Some(mosaic.samples.len()) {
        return Err(PipelineError::InvalidDimensions);
    }
    let phases = [
        [mosaic.cfa.color_at(0, 0), mosaic.cfa.color_at(1, 0)],
        [mosaic.cfa.color_at(0, 1), mosaic.cfa.color_at(1, 1)],
    ];
    let mut pixels = vec![[0.0; 3]; mosaic.samples.len()];
    // A standard Bayer pattern only reads measured red/blue and first-pass green
    // from adjacent rows. Other accepted 2x2 layouts retain serial update order.
    let standard = matches!(
        phases,
        [[Red, Green], [Green, Blue]]
            | [[Blue, Green], [Green, Red]]
            | [[Green, Red], [Blue, Green]]
            | [[Green, Blue], [Red, Green]]
    );
    if !pixels.is_empty() {
        let band_height = if standard { height.min(32) } else { height };
        pixels
            .par_chunks_mut(width * band_height)
            .enumerate()
            .for_each_init(Vec::new, |scratch, (band, output)| {
                let start = band * band_height;
                let end = start + output.len() / width;
                // One extra first-pass row on either side supplies second-pass neighbors.
                let top = start.saturating_sub(1);
                let bottom = (end + 1).min(height);
                scratch.resize((bottom - top) * width, [0.; 3]);
                for (row, sy) in scratch
                    .chunks_exact_mut(width)
                    .zip((0..mosaic.height).skip(top))
                {
                    let y = sy as usize;
                    let colors = phases[y & 1];
                    for (pixel, sx) in row.iter_mut().zip(0..mosaic.width) {
                        let x = sx as usize;
                        if is_ppg_border(mosaic, sx, sy) {
                            *pixel = bilinear_pixel(mosaic, sx, sy).map(|v| v.max(0.));
                        } else {
                            let color = colors[x & 1];
                            *pixel = [0.; 3];
                            pixel[color_channel(color)] = mosaic.samples[y * width + x].max(0.);
                            if color != CfaColor::Green {
                                pixel[1] = ppg_green(mosaic, sx, sy).max(0.);
                            }
                        }
                    }
                }
                for y in start.max(1)..end.min(height.saturating_sub(1)) {
                    let colors = phases[y & 1];
                    for x in 1..width.saturating_sub(1) {
                        let i = (y - top) * width + x;
                        scratch[i] =
                            ppg_red_blue(scratch, i, width, colors[x & 1], colors[(x + 1) & 1])
                                .map(|v| v.max(0.));
                    }
                }
                let offset = (start - top) * width;
                output.copy_from_slice(&scratch[offset..offset + output.len()]);
            });
    }
    Ok(LinearRgbImage {
        width: mosaic.width,
        height: mosaic.height,
        pixels,
    })
}

fn bilinear_pixel(mosaic: &NormalizedMosaic, x: u32, y: u32) -> [f32; 3] {
    [
        interpolate(mosaic, x, y, CfaColor::Red),
        interpolate(mosaic, x, y, CfaColor::Green),
        interpolate(mosaic, x, y, CfaColor::Blue),
    ]
}

fn is_ppg_border(mosaic: &NormalizedMosaic, x: u32, y: u32) -> bool {
    x < PPG_RADIUS
        || y < PPG_RADIUS
        || x.saturating_add(PPG_RADIUS) >= mosaic.width
        || y.saturating_add(PPG_RADIUS) >= mosaic.height
}

/// Demosaics bounded tiles with the exact dependency halo required by
/// [`ppg_demosaic`], then stitches only each tile's interior.
///
/// This correctness implementation establishes seam-free tile semantics for
/// the scheduler. It deliberately favors a small, auditable algorithm over
/// allocation reuse, which belongs to the Gate 5 cache and buffer pool.
///
/// # Errors
///
/// Returns [`PipelineError`] for non-Bayer input, zero-sized tiles, or
/// inconsistent dimensions.
pub fn ppg_demosaic_tiled(
    mosaic: &NormalizedMosaic,
    tile_width: u32,
    tile_height: u32,
) -> Result<LinearRgbImage, PipelineError> {
    require_bayer(mosaic, "tiled PPG")?;
    if tile_width == 0 || tile_height == 0 {
        return Err(PipelineError::InvalidDimensions);
    }
    let expected = usize::try_from(mosaic.width)
        .ok()
        .and_then(|width| {
            usize::try_from(mosaic.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or(PipelineError::InvalidDimensions)?;
    if expected != mosaic.samples.len() {
        return Err(PipelineError::InvalidDimensions);
    }
    let mut output = vec![[0.0_f32; 3]; expected];
    for tile_y in (0..mosaic.height).step_by(tile_height as usize) {
        for tile_x in (0..mosaic.width).step_by(tile_width as usize) {
            let interior_width = tile_width.min(mosaic.width - tile_x);
            let interior_height = tile_height.min(mosaic.height - tile_y);
            let halo_x = tile_x.saturating_sub(PPG_TILE_HALO);
            let halo_y = tile_y.saturating_sub(PPG_TILE_HALO);
            let halo_right = tile_x
                .saturating_add(interior_width)
                .saturating_add(PPG_TILE_HALO)
                .min(mosaic.width);
            let halo_bottom = tile_y
                .saturating_add(interior_height)
                .saturating_add(PPG_TILE_HALO)
                .min(mosaic.height);
            let cropped = active_crop(
                mosaic,
                halo_x,
                halo_y,
                halo_right - halo_x,
                halo_bottom - halo_y,
            )?;
            let demosaiced = ppg_demosaic(&cropped)?;
            let source_x = tile_x - halo_x;
            let source_y = tile_y - halo_y;
            for row in 0..interior_height {
                let source_start = usize::try_from((source_y + row) * demosaiced.width + source_x)
                    .map_err(|_| PipelineError::InvalidDimensions)?;
                let destination_start = usize::try_from((tile_y + row) * mosaic.width + tile_x)
                    .map_err(|_| PipelineError::InvalidDimensions)?;
                let length = usize::try_from(interior_width)
                    .map_err(|_| PipelineError::InvalidDimensions)?;
                output[destination_start..destination_start + length]
                    .copy_from_slice(&demosaiced.pixels[source_start..source_start + length]);
            }
        }
    }
    Ok(LinearRgbImage {
        width: mosaic.width,
        height: mosaic.height,
        pixels: output,
    })
}

fn require_bayer(mosaic: &NormalizedMosaic, name: &str) -> Result<(), PipelineError> {
    if mosaic.cfa.width() == 2 && mosaic.cfa.height() == 2 {
        Ok(())
    } else {
        Err(PipelineError::UnsupportedCfa(format!(
            "{}x{} {name} demosaic",
            mosaic.cfa.width(),
            mosaic.cfa.height()
        )))
    }
}

fn interpolate(mosaic: &NormalizedMosaic, x: u32, y: u32, color: CfaColor) -> f32 {
    if mosaic.cfa.color_at(x, y) == color {
        return mosaic.samples[mosaic.index(x, y)];
    }
    let mut sum = 0.0_f32;
    let mut count = 0_u8;
    for dy in -1_i32..=1 {
        for dx in -1_i32..=1 {
            let Some(neighbor_x) = x.checked_add_signed(dx) else {
                continue;
            };
            let Some(neighbor_y) = y.checked_add_signed(dy) else {
                continue;
            };
            if neighbor_x >= mosaic.width
                || neighbor_y >= mosaic.height
                || mosaic.cfa.color_at(neighbor_x, neighbor_y) != color
            {
                continue;
            }
            sum += mosaic.samples[mosaic.index(neighbor_x, neighbor_y)];
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        sum / f32::from(count)
    }
}

fn ppg_green(mosaic: &NormalizedMosaic, x: u32, y: u32) -> f32 {
    let left3 = sample_offset(mosaic, x, y, -3, 0);
    let left2 = sample_offset(mosaic, x, y, -2, 0);
    let left = sample_offset(mosaic, x, y, -1, 0);
    let right = sample_offset(mosaic, x, y, 1, 0);
    let right2 = sample_offset(mosaic, x, y, 2, 0);
    let right3 = sample_offset(mosaic, x, y, 3, 0);
    let up3 = sample_offset(mosaic, x, y, 0, -3);
    let up2 = sample_offset(mosaic, x, y, 0, -2);
    let up = sample_offset(mosaic, x, y, 0, -1);
    let down = sample_offset(mosaic, x, y, 0, 1);
    let down2 = sample_offset(mosaic, x, y, 0, 2);
    let down3 = sample_offset(mosaic, x, y, 0, 3);
    let center = mosaic.samples[mosaic.index(x, y)];
    let horizontal_guess = (left + center + right) * 2.0 - right2 - left2;
    let horizontal_difference =
        ((left2 - center).abs() + (right2 - center).abs() + (left - right).abs()) * 3.0
            + ((right3 - right).abs() + (left3 - left).abs()) * 2.0;
    let vertical_guess = (up + center + down) * 2.0 - down2 - up2;
    let vertical_difference = ((up2 - center).abs() + (down2 - center).abs() + (up - down).abs())
        * 3.0
        + ((down3 - down).abs() + (up3 - up).abs()) * 2.0;
    let horizontal = (horizontal_guess * 0.25).clamp(left.min(right), left.max(right));
    let vertical = (vertical_guess * 0.25).clamp(up.min(down), up.max(down));
    // Equivalent directions must not flip on CPU/GPU floating-point roundoff.
    let tolerance = 1.0e-6 * horizontal_difference.max(vertical_difference).max(1.0);
    if (horizontal_difference - vertical_difference).abs() <= tolerance {
        (horizontal + vertical) * 0.5
    } else if horizontal_difference > vertical_difference {
        vertical
    } else {
        horizontal
    }
}

fn sample_offset(mosaic: &NormalizedMosaic, x: u32, y: u32, dx: i32, dy: i32) -> f32 {
    let x = x.checked_add_signed(dx).expect("PPG x is inside halo");
    let y = y.checked_add_signed(dy).expect("PPG y is inside halo");
    mosaic.samples[mosaic.index(x, y)]
}

fn ppg_red_blue(
    pixels: &[[f32; 3]],
    i: usize,
    width: usize,
    center: CfaColor,
    right_color: CfaColor,
) -> [f32; 3] {
    let mut color = pixels[i];
    if center == CfaColor::Green {
        let top = pixels[i - width];
        let bottom = pixels[i + width];
        let left = pixels[i - 1];
        let right = pixels[i + 1];
        if right_color == CfaColor::Red {
            color[2] = (top[2] + bottom[2] + 2.0 * color[1] - top[1] - bottom[1]) * 0.5;
            color[0] = (left[0] + right[0] + 2.0 * color[1] - left[1] - right[1]) * 0.5;
        } else {
            color[0] = (top[0] + bottom[0] + 2.0 * color[1] - top[1] - bottom[1]) * 0.5;
            color[2] = (left[2] + right[2] + 2.0 * color[1] - left[1] - right[1]) * 0.5;
        }
    } else {
        let target = if center == CfaColor::Red { 2 } else { 0 };
        let top_left = pixels[i - width - 1];
        let top_right = pixels[i - width + 1];
        let bottom_left = pixels[i + width - 1];
        let bottom_right = pixels[i + width + 1];
        let difference_one = (top_left[target] - bottom_right[target]).abs()
            + (top_left[1] - color[1]).abs()
            + (bottom_right[1] - color[1]).abs();
        let guess_one = top_left[target] + bottom_right[target] + 2.0 * color[1]
            - top_left[1]
            - bottom_right[1];
        let difference_two = (top_right[target] - bottom_left[target]).abs()
            + (top_right[1] - color[1]).abs()
            + (bottom_left[1] - color[1]).abs();
        let guess_two = top_right[target] + bottom_left[target] + 2.0 * color[1]
            - top_right[1]
            - bottom_left[1];
        let tolerance = 1.0e-6 * difference_one.max(difference_two).max(1.0);
        color[target] = if (difference_one - difference_two).abs() <= tolerance {
            (guess_one + guess_two) * 0.25
        } else if difference_one > difference_two {
            guess_two * 0.5
        } else if difference_one < difference_two {
            guess_one * 0.5
        } else {
            (guess_one + guess_two) * 0.25
        };
    }
    color
}

const fn color_channel(color: CfaColor) -> usize {
    match color {
        CfaColor::Red => 0,
        CfaColor::Green => 1,
        CfaColor::Blue => 2,
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::{bilinear_demosaic, ppg_demosaic, ppg_demosaic_tiled};
    use crate::{CfaColor, CfaPattern, NormalizedMosaic};

    #[test]
    fn constant_bayer_field_remains_neutral() {
        let mosaic = NormalizedMosaic {
            width: 3,
            height: 3,
            cfa: CfaPattern::parse("RGGB").expect("CFA"),
            samples: vec![0.25; 9],
        };
        let rgb = bilinear_demosaic(&mosaic).expect("demosaic");
        assert!(rgb.pixels.iter().all(|pixel| *pixel == [0.25; 3]));
    }

    #[test]
    fn known_samples_are_not_replaced() {
        let mosaic = NormalizedMosaic {
            width: 2,
            height: 2,
            cfa: CfaPattern::parse("RGGB").expect("CFA"),
            samples: vec![1.0, 2.0, 3.0, 4.0],
        };
        let rgb = bilinear_demosaic(&mosaic).expect("demosaic");
        assert_eq!(rgb.pixels[0][0], 1.0);
        assert_eq!(rgb.pixels[1][1], 2.0);
        assert_eq!(rgb.pixels[2][1], 3.0);
        assert_eq!(rgb.pixels[3][2], 4.0);
    }

    #[test]
    fn edge_aware_preserves_constant_neutral() {
        let mosaic = NormalizedMosaic {
            width: 7,
            height: 7,
            cfa: CfaPattern::parse("RGGB").expect("CFA"),
            samples: vec![0.25; 49],
        };
        let rgb = ppg_demosaic(&mosaic).expect("demosaic");
        assert!(
            rgb.pixels
                .iter()
                .all(|pixel| { pixel.iter().all(|channel| (*channel - 0.25).abs() < 1.0e-6) })
        );
    }

    #[test]
    fn edge_aware_improves_sharp_vertical_edge_mse() {
        let (width, height) = (12, 10);
        let cfa = CfaPattern::parse("RGGB").expect("CFA");
        let mut samples = Vec::new();
        let mut truth = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let pixel = if x < width / 2 {
                    [0.05, 0.15, 0.1]
                } else {
                    [0.9, 0.45, 0.2]
                };
                samples.push(match cfa.color_at(x, y) {
                    CfaColor::Red => pixel[0],
                    CfaColor::Green => pixel[1],
                    CfaColor::Blue => pixel[2],
                });
                truth.push(pixel);
            }
        }
        let mosaic = NormalizedMosaic {
            width,
            height,
            cfa,
            samples,
        };
        let bilinear = bilinear_demosaic(&mosaic).expect("bilinear");
        let edge_aware = ppg_demosaic(&mosaic).expect("PPG");
        let mse = |pixels: &[[f32; 3]]| {
            pixels
                .iter()
                .zip(&truth)
                .flat_map(|(actual, expected)| {
                    actual
                        .iter()
                        .zip(expected)
                        .map(|(actual, expected)| (actual - expected).powi(2))
                })
                .sum::<f32>()
        };
        assert!(mse(&edge_aware.pixels) < mse(&bilinear.pixels));
    }

    #[test]
    fn tiled_edge_aware_is_identical_at_seams_for_all_bayer_layouts() {
        let (width, height) = (29, 67);
        for pattern in ["RGGB", "BGGR", "GRBG", "GBRG"] {
            let cfa = CfaPattern::parse(pattern).expect("CFA");
            let samples = (0..height)
                .flat_map(|y| {
                    (0..width).map(move |x| {
                        let diagonal = if x + y > 24 { 0.75 } else { 0.08 };
                        let repeating = f32::from(((x * 7 + y * 11) % 17) as u8) / 64.0;
                        if (x == 14 && y > 4) || (x + y) % 31 == 0 {
                            1.2
                        } else {
                            diagonal + repeating
                        }
                    })
                })
                .collect();
            let mosaic = NormalizedMosaic {
                width,
                height,
                cfa,
                samples,
            };
            let full = ppg_demosaic(&mosaic).expect("full demosaic");
            for (tile_width, tile_height) in [(1, 1), (7, 5), (16, 9), (64, 64)] {
                let tiled =
                    ppg_demosaic_tiled(&mosaic, tile_width, tile_height).expect("tiled demosaic");
                assert_eq!(
                    full.pixels, tiled.pixels,
                    "pattern={pattern} tile={tile_width}x{tile_height}"
                );
            }
        }
    }

    #[test]
    fn tiled_edge_aware_rejects_empty_tiles() {
        let mosaic = NormalizedMosaic {
            width: 2,
            height: 2,
            cfa: CfaPattern::parse("RGGB").expect("CFA"),
            samples: vec![0.0; 4],
        };
        assert!(ppg_demosaic_tiled(&mosaic, 0, 1).is_err());
        assert!(ppg_demosaic_tiled(&mosaic, 1, 0).is_err());
    }

    #[test]
    fn xtrans_is_rejected_until_a_quality_path_is_validated() {
        let pattern = "GBGGRGRGGRGGBGGBGGRGGRGGBGGBGGRGGRGG";
        let mosaic = NormalizedMosaic {
            width: 6,
            height: 6,
            cfa: CfaPattern::parse(pattern).expect("6x6 CFA"),
            samples: vec![0.25; 36],
        };
        assert!(bilinear_demosaic(&mosaic).is_err());
        assert!(ppg_demosaic(&mosaic).is_err());
    }
}
