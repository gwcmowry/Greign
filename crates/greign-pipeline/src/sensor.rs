//! Sensor-domain normalization, repair, crop, and white balance.

use greign_raw::{ActiveArea, Mosaic};
use rayon::prelude::*;

use crate::{CfaColor, CfaPattern, NormalizedMosaic, PipelineError, SensorPoint};

/// Converts integer sensor samples into unclamped linear values.
///
/// # Errors
///
/// Returns [`PipelineError`] for unsupported components, invalid CFA/levels,
/// or inconsistent dimensions.
pub fn normalize_sensor(mosaic: &Mosaic) -> Result<NormalizedMosaic, PipelineError> {
    let cfa = sensor_pattern(mosaic)?;
    normalize_all(mosaic, cfa)
}

fn sensor_pattern(mosaic: &Mosaic) -> Result<CfaPattern, PipelineError> {
    if mosaic.metadata.components_per_pixel != 1 {
        return Err(PipelineError::UnsupportedComponents);
    }
    let expected = usize::try_from(mosaic.metadata.width)
        .ok()
        .and_then(|width| {
            usize::try_from(mosaic.metadata.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or(PipelineError::InvalidDimensions)?;
    if expected != mosaic.samples.len() {
        return Err(PipelineError::InvalidDimensions);
    }
    let pattern = mosaic
        .metadata
        .cfa_pattern
        .as_deref()
        .ok_or_else(|| PipelineError::UnsupportedCfa("missing".into()))?;
    let cfa = CfaPattern::parse(pattern)?;
    validate_levels(&mosaic.metadata.black_levels, &cfa)?;
    validate_levels(&mosaic.metadata.white_levels, &cfa)?;

    Ok(cfa)
}

fn normalize_all(mosaic: &Mosaic, cfa: CfaPattern) -> Result<NormalizedMosaic, PipelineError> {
    let mut samples = Vec::with_capacity(mosaic.samples.len());
    for y in 0..mosaic.metadata.height {
        for x in 0..mosaic.metadata.width {
            let index = usize::try_from(y * mosaic.metadata.width + x)
                .map_err(|_| PipelineError::InvalidDimensions)?;
            let black = level_at(&mosaic.metadata.black_levels, &cfa, x, y);
            let white = level_at(&mosaic.metadata.white_levels, &cfa, x, y);
            if !black.is_finite() || !white.is_finite() || white <= black {
                return Err(PipelineError::InvalidLevels);
            }
            samples.push((f32::from(mosaic.samples[index]) - black) / (white - black));
        }
    }
    Ok(NormalizedMosaic {
        width: mosaic.metadata.width,
        height: mosaic.metadata.height,
        cfa,
        samples,
    })
}

/// Normalizes a film scan directly into its cropped full-size or reduced sensor plane.
/// Preview averages retain the original sample order and per-site black and white levels.
///
/// # Errors
///
/// Returns the same sensor, crop and reduction errors as the individual operations.
pub fn normalize_scan(
    mosaic: &Mosaic,
    area: Option<ActiveArea>,
    max_dimension: Option<u32>,
) -> Result<NormalizedMosaic, PipelineError> {
    let cfa = sensor_pattern(mosaic)?;
    let (sensor_width, sensor_height) = (mosaic.metadata.width, mosaic.metadata.height);
    // Preserve the general normalization path for non-Bayer and degenerate sensors.
    if cfa.width() != 2 || sensor_width < 2 || sensor_height < 2 {
        let mut normalized = normalize_all(mosaic, cfa)?;
        if let Some(a) = area {
            normalized = active_crop(&normalized, a.x, a.y, a.width, a.height)?;
        }
        return match max_dimension {
            Some(size) => downsample_bayer(&normalized, size),
            None => Ok(normalized),
        };
    }
    let mut levels = [[(0.0, 0.0); 2]; 2];
    for (y, row) in (0..2).zip(&mut levels) {
        for (x, level) in (0..2).zip(row) {
            let black = level_at(&mosaic.metadata.black_levels, &cfa, x, y);
            let white = level_at(&mosaic.metadata.white_levels, &cfa, x, y);
            if !black.is_finite() || !white.is_finite() || white <= black {
                return Err(PipelineError::InvalidLevels);
            }
            *level = (black, white - black);
        }
    }
    let a = area.unwrap_or(ActiveArea {
        x: 0,
        y: 0,
        width: sensor_width,
        height: sensor_height,
    });
    if a.width == 0
        || a.height == 0
        || a.x
            .checked_add(a.width)
            .is_none_or(|end| end > sensor_width)
        || a.y
            .checked_add(a.height)
            .is_none_or(|end| end > sensor_height)
    {
        return Err(PipelineError::InvalidCrop);
    }
    let cfa = cfa.shifted(a.x, a.y);
    let sample = |x: u32, y: u32| {
        let (x, y) = (a.x + x, a.y + y);
        let (black, range) = levels[(y & 1) as usize][(x & 1) as usize];
        (f32::from(mosaic.samples[y as usize * sensor_width as usize + x as usize]) - black) / range
    };
    if let Some(size) = max_dimension {
        if size == 0 {
            return Err(PipelineError::InvalidDimensions);
        }
        if a.width.max(a.height) > size {
            return reduce_bayer(a.width, a.height, cfa, size, sample);
        }
    }
    let mut samples = vec![0.; a.width as usize * a.height as usize];
    samples
        .par_chunks_mut(a.width as usize)
        .zip(0..a.height)
        .for_each(|(row, y)| {
            for (v, x) in row.iter_mut().zip(0..a.width) {
                *v = sample(x, y);
            }
        });
    Ok(NormalizedMosaic {
        width: a.width,
        height: a.height,
        cfa,
        samples,
    })
}

/// Repairs explicitly identified bad pixels using same-color neighbors.
///
/// Pixels without any same-color neighbor in a five-by-five window are left
/// unchanged. Invalid coordinates are ignored.
pub fn repair_bad_pixels(mosaic: &mut NormalizedMosaic, bad_pixels: &[SensorPoint]) {
    for point in bad_pixels {
        if point.x >= mosaic.width || point.y >= mosaic.height {
            continue;
        }
        let target_color = mosaic.cfa.color_at(point.x, point.y);
        let mut sum = 0.0_f32;
        let mut count = 0_u8;
        for dy in -2_i32..=2 {
            for dx in -2_i32..=2 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let Some(x) = point.x.checked_add_signed(dx) else {
                    continue;
                };
                let Some(y) = point.y.checked_add_signed(dy) else {
                    continue;
                };
                if x >= mosaic.width
                    || y >= mosaic.height
                    || mosaic.cfa.color_at(x, y) != target_color
                {
                    continue;
                }
                sum += mosaic.samples[mosaic.index(x, y)];
                count += 1;
            }
        }
        if count > 0 {
            let index = mosaic.index(point.x, point.y);
            mosaic.samples[index] = sum / f32::from(count);
        }
    }
}

/// Crops to a sensor rectangle and shifts the CFA origin accordingly.
///
/// # Errors
///
/// Returns [`PipelineError::InvalidCrop`] for an empty or out-of-bounds area.
pub fn active_crop(
    mosaic: &NormalizedMosaic,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<NormalizedMosaic, PipelineError> {
    let valid = width > 0
        && height > 0
        && x.checked_add(width)
            .is_some_and(|right| right <= mosaic.width)
        && y.checked_add(height)
            .is_some_and(|bottom| bottom <= mosaic.height);
    if !valid {
        return Err(PipelineError::InvalidCrop);
    }
    let capacity = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or(PipelineError::InvalidDimensions)?;
    let mut samples = Vec::with_capacity(capacity);
    for row in y..y + height {
        let start = mosaic.index(x, row);
        let end = start + usize::try_from(width).map_err(|_| PipelineError::InvalidDimensions)?;
        samples.extend_from_slice(&mosaic.samples[start..end]);
    }
    Ok(NormalizedMosaic {
        width,
        height,
        cfa: mosaic.cfa.shifted(x, y),
        samples,
    })
}

/// Applies RGB as-shot white balance directly to CFA samples.
///
/// # Errors
///
/// Returns [`PipelineError::NonFiniteParameter`] for invalid coefficients.
pub fn apply_white_balance(
    mosaic: &mut NormalizedMosaic,
    coefficients: [f32; 3],
) -> Result<(), PipelineError> {
    if coefficients
        .iter()
        .any(|coefficient| !coefficient.is_finite() || *coefficient <= 0.0)
    {
        return Err(PipelineError::NonFiniteParameter);
    }
    for y in 0..mosaic.height {
        for x in 0..mosaic.width {
            let coefficient = match mosaic.cfa.color_at(x, y) {
                CfaColor::Red => coefficients[0],
                CfaColor::Green => coefficients[1],
                CfaColor::Blue => coefficients[2],
            };
            let index = mosaic.index(x, y);
            mosaic.samples[index] *= coefficient;
        }
    }
    Ok(())
}

/// Reduces a normalized Bayer mosaic to a bounded preview dimension.
///
/// Each destination location averages only source samples matching its
/// destination CFA color. Reduction factors greater than one are rounded up
/// to an even number so every source block contains all Bayer colors and the
/// output retains the original 2×2 layout.
///
/// # Errors
///
/// Returns [`PipelineError`] for non-Bayer input, a zero maximum dimension,
/// or inconsistent dimensions.
pub fn downsample_bayer(
    mosaic: &NormalizedMosaic,
    max_dimension: u32,
) -> Result<NormalizedMosaic, PipelineError> {
    if max_dimension == 0 {
        return Err(PipelineError::InvalidDimensions);
    }
    if mosaic.cfa.width() != 2 || mosaic.cfa.height() != 2 {
        return Err(PipelineError::UnsupportedCfa(format!(
            "{}x{} preview reduction",
            mosaic.cfa.width(),
            mosaic.cfa.height()
        )));
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
    if mosaic.width.max(mosaic.height) <= max_dimension {
        return Ok(mosaic.clone());
    }
    reduce_bayer(
        mosaic.width,
        mosaic.height,
        mosaic.cfa.clone(),
        max_dimension,
        |x, y| mosaic.samples[mosaic.index(x, y)],
    )
}

fn reduce_bayer(
    source_width: u32,
    source_height: u32,
    cfa: CfaPattern,
    max_dimension: u32,
    sample: impl Fn(u32, u32) -> f32 + Sync,
) -> Result<NormalizedMosaic, PipelineError> {
    let mut factor = source_width.max(source_height).div_ceil(max_dimension);
    if factor % 2 != 0 {
        factor = factor.saturating_add(1);
    }
    let width = source_width.div_ceil(factor);
    let height = source_height.div_ceil(factor);
    let phases = [
        [cfa.color_at(0, 0), cfa.color_at(1, 0)],
        [cfa.color_at(0, 1), cfa.color_at(1, 1)],
    ];
    let mut samples = vec![0.; width as usize * height as usize];
    samples
        .par_chunks_mut((width as usize).max(1))
        .zip(0..height)
        .try_for_each(|(row, output_y)| {
            for (value, output_x) in row.iter_mut().zip(0..width) {
                let target = phases[(output_y & 1) as usize][(output_x & 1) as usize];
                // Shift partial blocks inward to retain both CFA phases at the edge.
                let source_x = (output_x * factor).min(source_width.saturating_sub(2));
                let source_y = (output_y * factor).min(source_height.saturating_sub(2));
                let right = source_x.saturating_add(factor).min(source_width);
                let bottom = source_y.saturating_add(factor).min(source_height);
                let mut sum = 0.0_f32;
                let mut count = 0.0_f32;
                for y in source_y..bottom {
                    let colors = phases[(y & 1) as usize];
                    for x in source_x..right {
                        if colors[(x & 1) as usize] == target {
                            sum += sample(x, y);
                            count += 1.0;
                        }
                    }
                }
                if count < 0.5 {
                    return Err(PipelineError::InvalidDimensions);
                }
                *value = sum / count;
            }
            Ok(())
        })?;
    Ok(NormalizedMosaic {
        width,
        height,
        cfa,
        samples,
    })
}

/// Reconstructs clipped white-balanced Bayer samples from the opposing planes.
///
/// The local estimate is the cube of the mean cube-roots of the other two
/// color-plane means. The boundary of each connected clipped region supplies
/// its chrominance residual, including pixels deep inside a blown highlight.
/// A clipped value is only raised, never replaced by an arbitrary lower
/// neighbor. This is derived from Darktable's GPLv3 "inpaint opposed"
/// highlight reconstruction by its 2022–2023 contributors; Greign uses a
/// region context shared by the CPU and GPU paths.
///
/// # Errors
///
/// Returns [`PipelineError::NonFiniteParameter`] for a non-finite or
/// non-positive threshold.
pub fn reconstruct_clipped_samples(
    mosaic: &mut NormalizedMosaic,
    thresholds: [f32; 3],
) -> Result<(), PipelineError> {
    if thresholds
        .iter()
        .any(|threshold| !threshold.is_finite() || *threshold <= 0.0)
    {
        return Err(PipelineError::NonFiniteParameter);
    }
    let context = crate::HighlightContext::from_normalized(mosaic, thresholds)?;
    let source = mosaic.samples.clone();
    for y in 0..mosaic.height {
        for x in 0..mosaic.width {
            let index = mosaic.index(x, y);
            let color = match mosaic.cfa.color_at(x, y) {
                CfaColor::Red => 0,
                CfaColor::Green => 1,
                CfaColor::Blue => 2,
            };
            if source[index] < thresholds[color] {
                continue;
            }
            let reference = opposed_reference(mosaic, &source, x, y, color);
            let correction = context.corrections[((y / crate::HIGHLIGHT_TILE) * context.width
                + x / crate::HIGHLIGHT_TILE)
                as usize];
            let chrominance = if correction[3] > 0.0 {
                correction[color]
            } else {
                local_chrominance(mosaic, &source, thresholds[color], x, y, color)
            };
            mosaic.samples[index] = source[index].max(reference + chrominance);
        }
    }
    Ok(())
}

fn local_chrominance(
    mosaic: &NormalizedMosaic,
    source: &[f32],
    threshold: f32,
    x: u32,
    y: u32,
    color: usize,
) -> f32 {
    let mut sum = 0.0;
    let mut count = 0_u8;
    for (dx, dy) in [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ] {
        for distance in 1..=4 {
            let Some(nx) = x.checked_add_signed(dx * distance * 2) else {
                break;
            };
            let Some(ny) = y.checked_add_signed(dy * distance * 2) else {
                break;
            };
            if nx >= mosaic.width || ny >= mosaic.height {
                break;
            }
            let value = source[mosaic.index(nx, ny)];
            if value > threshold * 0.2 && value < threshold {
                sum += value - opposed_reference(mosaic, source, nx, ny, color);
                count += 1;
                break;
            }
        }
    }
    if count >= 3 {
        sum / f32::from(count)
    } else {
        0.0
    }
}

fn opposed_reference(
    mosaic: &NormalizedMosaic,
    source: &[f32],
    x: u32,
    y: u32,
    target_color: usize,
) -> f32 {
    let mut sums = [0.0_f32; 3];
    let mut counts = [0_u8; 3];
    for dy in -1_i32..=1 {
        for dx in -1_i32..=1 {
            let Some(neighbor_x) = x.checked_add_signed(dx) else {
                continue;
            };
            let Some(neighbor_y) = y.checked_add_signed(dy) else {
                continue;
            };
            if neighbor_x >= mosaic.width || neighbor_y >= mosaic.height {
                continue;
            }
            let neighbor_color = match mosaic.cfa.color_at(neighbor_x, neighbor_y) {
                CfaColor::Red => 0,
                CfaColor::Green => 1,
                CfaColor::Blue => 2,
            };
            sums[neighbor_color] += source[mosaic.index(neighbor_x, neighbor_y)].max(0.0);
            counts[neighbor_color] += 1;
        }
    }
    let mut roots = [0.0_f32; 3];
    for channel in 0..3 {
        if counts[channel] > 0 {
            roots[channel] = (sums[channel] / f32::from(counts[channel])).cbrt();
        }
    }
    let opposing = match target_color {
        0 => (roots[1] + roots[2]) * 0.5,
        1 => (roots[0] + roots[2]) * 0.5,
        _ => (roots[0] + roots[1]) * 0.5,
    };
    opposing.powi(3)
}

fn validate_levels(levels: &[f32], cfa: &CfaPattern) -> Result<(), PipelineError> {
    if levels.len() == 1 || levels.len() == cfa.len() {
        Ok(())
    } else {
        Err(PipelineError::InvalidLevels)
    }
}

fn level_at(levels: &[f32], cfa: &CfaPattern, x: u32, y: u32) -> f32 {
    if levels.len() == 1 {
        levels[0]
    } else {
        let index = (y % cfa.height()) * cfa.width() + (x % cfa.width());
        levels[usize::try_from(index).expect("small CFA index")]
    }
}

#[cfg(test)]
mod tests {
    use greign_raw::{Mosaic, Orientation, RawMetadata};

    use super::{
        active_crop, apply_white_balance, downsample_bayer, normalize_sensor,
        reconstruct_clipped_samples, repair_bad_pixels,
    };
    use crate::{CfaColor, CfaPattern, NormalizedMosaic, SensorPoint};

    fn metadata(width: u32, height: u32) -> RawMetadata {
        RawMetadata {
            backend: "test",
            make: String::new(),
            model: String::new(),
            mode: String::new(),
            width,
            height,
            components_per_pixel: 1,
            bits_per_component: 12,
            orientation: Orientation::Normal,
            active_area: None,
            crop_area: None,
            black_levels: vec![64.0; 4],
            white_levels: vec![1_064.0],
            white_balance: [2.0, 1.0, 3.0],
            cfa_pattern: Some("RGGB".into()),
            xyz_to_camera_d65: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    #[test]
    fn fused_scan_matches_separate_steps_with_shifted_levels_and_partial_blocks() {
        let mut mosaic = Mosaic {
            metadata: metadata(33, 29),
            samples: (0_u16..33 * 29)
                .map(|i| i.wrapping_mul(97) % 4096)
                .collect(),
        };
        mosaic.metadata.black_levels = vec![64., 53., 71., 68.];
        mosaic.metadata.white_levels = vec![1064., 1032., 999., 1101.];
        for pattern in ["RGGB", "BGGR", "GRBG", "GBRG"] {
            mosaic.metadata.cfa_pattern = Some(pattern.into());
            let area = greign_raw::ActiveArea {
                x: 1,
                y: 3,
                width: 31,
                height: 25,
            };
            let original = normalize_sensor(&mosaic).unwrap();
            let cropped = active_crop(&original, area.x, area.y, area.width, area.height).unwrap();
            for size in [None, Some(64), Some(7), Some(4), Some(1)] {
                let expected = size.map_or_else(
                    || cropped.clone(),
                    |size| downsample_bayer(&cropped, size).unwrap(),
                );
                let actual = super::normalize_scan(&mosaic, Some(area), size).unwrap();
                assert_eq!(
                    (actual.width, actual.height, actual.cfa),
                    (expected.width, expected.height, expected.cfa)
                );
                assert_eq!(actual.samples, expected.samples);
            }
        }
    }

    #[test]
    fn normalization_is_hand_computable_and_unclamped() {
        let mosaic = Mosaic {
            metadata: metadata(2, 2),
            samples: vec![0, 64, 564, 1_064],
        };
        let normalized = normalize_sensor(&mosaic).expect("normalize");
        assert_eq!(normalized.samples, vec![-0.064, 0.0, 0.5, 1.0]);
    }

    #[test]
    fn odd_crop_shifts_cfa_origin() {
        let mosaic = Mosaic {
            metadata: metadata(4, 4),
            samples: vec![64; 16],
        };
        let normalized = normalize_sensor(&mosaic).expect("normalize");
        let cropped = active_crop(&normalized, 1, 1, 3, 3).expect("crop");
        assert_eq!(cropped.cfa.color_at(0, 0), crate::CfaColor::Blue);
        assert_eq!(cropped.samples.len(), 9);
    }

    #[test]
    fn white_balance_uses_cfa_colors() {
        let mosaic = Mosaic {
            metadata: metadata(2, 2),
            samples: vec![564; 4],
        };
        let mut normalized = normalize_sensor(&mosaic).expect("normalize");
        apply_white_balance(&mut normalized, [2.0, 1.0, 3.0]).expect("white balance");
        assert_eq!(normalized.samples, vec![1.0, 0.5, 0.5, 1.5]);
    }

    #[test]
    fn bad_pixel_uses_same_color_neighbors() {
        let mosaic = Mosaic {
            metadata: metadata(5, 5),
            samples: vec![564; 25],
        };
        let mut normalized = normalize_sensor(&mosaic).expect("normalize");
        normalized.samples[12] = 10.0;
        repair_bad_pixels(&mut normalized, &[SensorPoint { x: 2, y: 2 }]);
        assert!((normalized.samples[12] - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn opposed_reconstruction_raises_clipped_plane_without_lowering_signal() {
        let cfa = CfaPattern::parse("RGGB").expect("CFA");
        let mut samples = Vec::new();
        for y in 0..5 {
            for x in 0..5 {
                samples.push(match cfa.color_at(x, y) {
                    CfaColor::Green => 1.0,
                    CfaColor::Red | CfaColor::Blue => 2.0,
                });
            }
        }
        let mut normalized = NormalizedMosaic {
            width: 5,
            height: 5,
            cfa,
            samples,
        };
        reconstruct_clipped_samples(&mut normalized, [1.9, 0.99, 1.9]).expect("reconstruct");
        assert!((normalized.samples[normalized.index(2, 3)] - 2.0).abs() < 1.0e-5);
        assert!((normalized.samples[normalized.index(2, 2)] - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn opposed_reconstruction_preserves_nearby_highlight_chrominance() {
        let cfa = CfaPattern::parse("RGGB").expect("CFA");
        let mut samples = Vec::new();
        for y in 0..13 {
            for x in 0..13 {
                samples.push(match cfa.color_at(x, y) {
                    CfaColor::Green => 0.8,
                    CfaColor::Red | CfaColor::Blue => 2.0,
                });
            }
        }
        let mut normalized = NormalizedMosaic {
            width: 13,
            height: 13,
            cfa,
            samples,
        };
        let clipped_green = normalized.index(7, 6);
        normalized.samples[clipped_green] = 1.0;
        reconstruct_clipped_samples(&mut normalized, [1.9, 0.99, 1.9]).expect("reconstruct");
        assert!(normalized.samples[clipped_green] < 1.05);
        assert!(normalized.samples[clipped_green] >= 1.0);
    }

    #[test]
    fn bayer_preview_reduction_is_bounded_and_preserves_constant_channels() {
        let cfa = CfaPattern::parse("RGGB").expect("CFA");
        let mut samples = Vec::new();
        for y in 0..13 {
            for x in 0..19 {
                samples.push(match cfa.color_at(x, y) {
                    CfaColor::Red => 0.2,
                    CfaColor::Green => 0.5,
                    CfaColor::Blue => 0.8,
                });
            }
        }
        let reduced = downsample_bayer(
            &NormalizedMosaic {
                width: 19,
                height: 13,
                cfa,
                samples,
            },
            6,
        )
        .expect("reduction");
        assert_eq!((reduced.width, reduced.height), (5, 4));
        for y in 0..reduced.height {
            for x in 0..reduced.width {
                let expected = match reduced.cfa.color_at(x, y) {
                    CfaColor::Red => 0.2,
                    CfaColor::Green => 0.5,
                    CfaColor::Blue => 0.8,
                };
                assert!((reduced.samples[reduced.index(x, y)] - expected).abs() < 1.0e-6);
            }
        }
    }
}
