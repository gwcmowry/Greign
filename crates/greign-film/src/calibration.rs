//! Border-only illumination estimation. No scene shading is used as a flat field.
use crate::{Rect, Scan, channels, quantile};
use serde::{Deserialize, Serialize};

/// Optional corrections, applied only by a new analysis proposal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CalibrationOptions {
    /// Fit a smooth illumination field to clean, unexposed film around the picture.
    pub illumination: bool,
    /// Blend toward equal channel density ranges (0–1) for faded color negatives.
    /// This assumes the picture contains a useful range of colors and neutrals.
    pub color_balance: f32,
}

pub(crate) fn field(coefficients: [f32; 5], x: f32, y: f32) -> f32 {
    let (x, y) = (x - 0.5, y - 0.5);
    coefficients[0] * x
        + coefficients[1] * y
        + coefficients[2] * x * x
        + coefficients[3] * y * y
        + coefficients[4] * x * y
}

#[derive(Clone, Copy)]
struct Sample {
    point: [f32; 2],
    value: [f32; 3],
    side: usize,
}

fn basis(point: [f32; 2]) -> [f64; 6] {
    let [x, y] = point.map(|v| f64::from(v - 0.5));
    [x, y, x * x, y * y, x * y, 1.]
}

// Small, pivoted least-squares solve; work is bounded by 164 border patches.
fn fit(samples: &[Sample], keep: &[bool]) -> Option<[[f32; 3]; 6]> {
    let mut system = [[0_f64; 9]; 6];
    for (sample, &keep) in samples.iter().zip(keep) {
        if !keep {
            continue;
        }
        let features = basis(sample.point);
        for row in 0..6 {
            for column in 0..6 {
                system[row][column] += features[row] * features[column];
            }
            for c in 0..3 {
                system[row][6 + c] += features[row] * f64::from(sample.value[c].log10());
            }
        }
    }
    for column in 0..6 {
        let pivot = (column..6)
            .max_by(|&a, &b| system[a][column].abs().total_cmp(&system[b][column].abs()))?;
        system.swap(column, pivot);
        let divisor = system[column][column];
        if divisor.abs() < 1e-7 {
            return None;
        }
        for value in &mut system[column] {
            *value /= divisor;
        }
        for row in 0..6 {
            if row == column {
                continue;
            }
            let factor = system[row][column];
            let pivot = system[column];
            for (value, pivot) in system[row].iter_mut().zip(pivot).skip(column) {
                *value -= factor * pivot;
            }
        }
    }
    Some(std::array::from_fn(|row| {
        std::array::from_fn(|c| system[row][6 + c] as f32)
    }))
}

#[allow(clippy::many_single_char_names)]
fn border_samples(scan: &Scan, crop: Rect) -> Vec<Sample> {
    let image = &scan.image;
    let (w, h) = (image.width as f32, image.height as f32);
    // Keep the sampling footprint fixed in preview-relative coordinates, also
    // when the caller analyzes a full-resolution scan.
    let scale = (w.max(h) / 960.).max(1.);
    let (sin, cos) = crop.angle.to_radians().sin_cos();
    let mut lines: Vec<Vec<Sample>> = Vec::new();
    for side in 0..4 {
        for i in 0..41 {
            let t = 0.1 + i as f32 * 0.02;
            let q = [[0., t], [1., t], [t, 0.], [t, 1.]][side];
            let start = crop.source_point(q, w / h);
            let direction = [[-cos, -sin], [cos, sin], [sin, -cos], [-sin, cos]][side];
            let mut line = Vec::new();
            for distance in 2..(w.min(h) * 0.05 / scale) as usize {
                let x = start[0] * w + direction[0] * distance as f32 * scale;
                let y = start[1] * h + direction[1] * distance as f32 * scale;
                if x < scale || y < scale || x >= w - scale || y >= h - scale {
                    continue;
                }
                let patch: Vec<_> = [-1., 0., 1.]
                    .into_iter()
                    .flat_map(|dy| {
                        [-1., 0., 1.].into_iter().map(move |dx| {
                            image.pixels[((y + dy * scale) as u32 * image.width
                                + (x + dx * scale) as u32)
                                as usize]
                        })
                    })
                    .collect();
                if patch
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite() || *v <= 0.000_01 || *v >= 0.98)
                {
                    continue;
                }
                let value = channels(&patch, 0.5);
                let low = channels(&patch, 0.0);
                let high = channels(&patch, 1.0);
                if (0..3).any(|c| (high[c] - low[c]) / value[c] > 0.22) {
                    continue;
                }
                line.push(Sample {
                    point: [x / w, y / h],
                    value,
                    side,
                });
            }
            if !line.is_empty() {
                lines.push(line);
            }
        }
    }
    let brightest = |line: &[Sample]| {
        line.iter()
            .max_by(|a, b| {
                a.value
                    .iter()
                    .sum::<f32>()
                    .total_cmp(&b.value.iter().sum::<f32>())
            })
            .copied()
    };
    let colors: Vec<_> = lines
        .iter()
        .filter_map(|line| brightest(line))
        .map(|s| s.value.map(|v| v / s.value.iter().sum::<f32>()))
        .collect();
    let reference = channels(&colors, 0.5);
    lines
        .into_iter()
        .filter_map(|mut line| {
            line.retain(|s| {
                (0..3).all(|c| {
                    (s.value[c] / s.value.iter().sum::<f32>() / reference[c] - 1.).abs() < 0.1
                })
            });
            brightest(&line)
        })
        .collect()
}

/// Return center base and a common-channel log10 field, only with broad clean
/// border support. Constant exposure/color stays in `base`, not in the field.
pub(crate) fn illumination(scan: &Scan, crop: Rect) -> Option<([f32; 3], [f32; 5])> {
    let samples = border_samples(scan, crop);
    if samples.len() < 64 {
        return None;
    }
    let mut keep = vec![true; samples.len()];
    let mut coefficients = fit(&samples, &keep)?;
    for _ in 0..4 {
        let errors: Vec<_> = samples
            .iter()
            .map(|s| {
                let features = basis(s.point);
                (0..3)
                    .map(|c| {
                        (features
                            .iter()
                            .zip(coefficients)
                            .map(|(f, v)| *f as f32 * v[c])
                            .sum::<f32>()
                            - s.value[c].log10())
                        .abs()
                    })
                    .fold(0_f32, f32::max)
            })
            .collect();
        let threshold = (3. * quantile(errors.clone(), 0.5)).clamp(0.025, 0.09);
        keep = errors.iter().map(|e| *e < threshold).collect();
        if (0..4).any(|side| {
            samples
                .iter()
                .zip(&keep)
                .filter(|(s, k)| s.side == side && **k)
                .count()
                < 8
        }) {
            return None;
        }
        coefficients = fit(&samples, &keep)?;
    }
    let residual = quantile(
        samples
            .iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .map(|(s, _)| {
                let features = basis(s.point);
                (0..3)
                    .map(|c| {
                        (features
                            .iter()
                            .zip(coefficients)
                            .map(|(f, v)| *f as f32 * v[c])
                            .sum::<f32>()
                            - s.value[c].log10())
                        .abs()
                    })
                    .fold(0_f32, f32::max)
            })
            .collect(),
        0.75,
    );
    if residual > 0.055 {
        return None;
    }
    let common = std::array::from_fn(|i| coefficients[i].iter().sum::<f32>() / 3.);
    let mut levels = Vec::new();
    for y in 0..9 {
        for x in 0..9 {
            let point = [x as f32 / 8., y as f32 / 8.];
            let value = field(common, point[0], point[1]);
            // A brightness correction must not conceal a color-varying border
            // or extrapolate a large, unsupported correction into the picture.
            if (0..3).any(|c| {
                (field(
                    std::array::from_fn(|i| coefficients[i][c]),
                    point[0],
                    point[1],
                ) - value)
                    .abs()
                    > 0.06
            }) {
                return None;
            }
            levels.push(value);
        }
    }
    if quantile(levels.clone(), 1.) - quantile(levels, 0.) > 0.75 {
        return None;
    }
    Some((coefficients[5].map(|v| 10_f32.powf(v)), common))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Adjustments, Material, analyze_with_options, prepare};

    fn negative(coefficients: [f32; 5]) -> (Scan, Rect) {
        let picture = Rect {
            x: 0.10,
            y: 0.10,
            w: 0.80,
            h: 0.80,
            angle: 0.,
        };
        let (width, height) = (480, 320);
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let (xx, yy) = (x as f32 / width as f32, y as f32 / height as f32);
                let gain = 10_f32.powf(field(coefficients, xx, yy));
                let density = if picture.contains(xx, yy) {
                    // True scene shading must survive the correction.
                    0.15 + 0.7 * xx + 0.2 * (yy * 30.).sin().abs()
                } else {
                    0.
                };
                pixels.push([0.45, 0.30, 0.15].map(|b| b * gain * 10_f32.powf(-density)));
            }
        }
        (
            Scan {
                image: greign_pipeline::LinearRgbImage {
                    width,
                    height,
                    pixels,
                },
                camera_matrix: None,
                white_balance: [1.; 3],
                capture: "synthetic".into(),
            },
            picture,
        )
    }

    #[test]
    fn border_field_recovers_illumination_and_preserves_real_scene_shading() {
        let expected = [-0.28, 0.05, -0.30, -0.15, 0.04];
        let (mut scan, picture) = negative(expected);
        // Outlying dust and clipped perforations are not part of the field.
        for y in 15..24 {
            for x in 100..120 {
                scan.image.pixels[y * 480 + x] = [1.2; 3];
            }
        }
        let (base, actual) = illumination(&scan, picture).expect("clean border around picture");
        let integrated = analyze_with_options(
            &[&scan],
            None,
            Rect::default(),
            CalibrationOptions {
                illumination: true,
                color_balance: 0.,
            },
        )
        .unwrap();
        assert!(
            integrated.illumination.iter().any(|v| v.abs() > 0.01),
            "The requested field must reach the accepted calibration"
        );
        for y in 0..7 {
            for x in 0..7 {
                let (x, y) = (x as f32 / 6., y as f32 / 6.);
                assert!(
                    (field(actual, x, y) - field(expected, x, y)).abs() < 0.018,
                    "{actual:?}"
                );
                assert!(
                    (field(integrated.illumination, x, y) - field(expected, x, y)).abs() < 0.025
                );
            }
        }
        let density = |x: usize| {
            let p = scan.image.pixels[160 * 480 + x];
            (base[0] / p[0]).log10() + field(actual, x as f32 / 480., 0.5)
        };
        assert!((density(384) - density(96) - 0.42).abs() < 0.01);
    }

    #[test]
    fn obscured_or_color_varying_borders_are_not_an_illumination_measurement() {
        let (mut scan, picture) = negative([0.; 5]);
        for y in 0..320 {
            for x in 0..48 {
                scan.image.pixels[y * 480 + x] = [0.000_001; 3];
            }
        }
        assert!(illumination(&scan, picture).is_none());
        let (mut scan, picture) = negative([0.; 5]);
        for y in 0..320 {
            for x in 0..480 {
                scan.image.pixels[y * 480 + x][2] *= 1. - 0.75 * x as f32 / 480.;
            }
        }
        assert!(illumination(&scan, picture).is_none());
    }

    #[test]
    fn measured_base_anchors_color_instead_of_being_canceled_by_auto_black_points() {
        let (scan, _) = negative([0.; 5]);
        let right = analyze_with_options(
            &[&scan],
            Some([0.45, 0.30, 0.15]),
            Rect::default(),
            CalibrationOptions::default(),
        )
        .unwrap();
        let wrong = analyze_with_options(
            &[&scan],
            Some([0.75, 0.30, 0.15]),
            Rect::default(),
            CalibrationOptions::default(),
        )
        .unwrap();
        let correct = prepare(&scan, Material::ColorNegative, Some(&right), false, &|| {
            false
        })
        .unwrap();
        let cast = prepare(&scan, Material::ColorNegative, Some(&wrong), false, &|| {
            false
        })
        .unwrap();
        let index = 160 * 480 + 240;
        let p = correct.image.pixels[index];
        assert!(
            (p[0] - p[1]).abs() < 0.0001 && (p[1] - p[2]).abs() < 0.0001,
            "Known neutral negative must remain neutral: {p:?}"
        );
        assert!(
            (cast.image.pixels[index][0] - cast.image.pixels[index][1]).abs() > 0.05,
            "A different base must affect color, not disappear in independent floor subtraction"
        );
        for analysis in [&right, &wrong] {
            let offsets =
                std::array::from_fn::<_, 3, _>(|c| analysis.floor[c] * analysis.channel_scale[c]);
            assert!(
                (offsets[0] - offsets[1]).abs() < 1e-6 && (offsets[1] - offsets[2]).abs() < 1e-6
            );
        }
        assert!(
            cast.image
                .pixels
                .iter()
                .flatten()
                .all(|v| v.is_finite() && *v < 1.),
            "Measured-base tone range must contain the recovered highlights"
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn density_balance_recovers_a_neutral_ramp_with_unequal_dye_slopes() {
        let (mut scan, _) = negative([0.; 5]);
        for (i, p) in scan.image.pixels.iter_mut().enumerate() {
            let d = 0.1 + 1.2 * (i % 480) as f32 / 479.;
            *p = std::array::from_fn(|c| {
                [0.45, 0.30, 0.15][c] * 10_f32.powf(-d * [0.65, 1., 0.8][c])
            });
        }
        let a = analyze_with_options(
            &[&scan],
            Some([0.45, 0.30, 0.15]),
            Rect::default(),
            CalibrationOptions {
                illumination: false,
                color_balance: 1.,
            },
        )
        .unwrap();
        let p = prepare(&scan, Material::ColorNegative, Some(&a), false, &|| false).unwrap();
        for pixel in p.image.pixels.iter().step_by(53) {
            assert!(
                (pixel[0] - pixel[1]).abs() < 0.0001 && (pixel[1] - pixel[2]).abs() < 0.0001,
                "{pixel:?}"
            );
        }
        let mut legacy = serde_json::to_value(&a).unwrap();
        for field in ["channel_scale", "illumination", "options"] {
            legacy.as_object_mut().unwrap().remove(field);
        }
        let legacy: crate::Analysis = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.channel_scale, [1.; 3]);
        assert_eq!(legacy.illumination, [0.; 5]);
        assert_eq!(legacy.options, CalibrationOptions::default());
        assert!(
            crate::render(
                &scan,
                Material::ColorNegative,
                Some(&legacy),
                Adjustments::default(),
                Rect::default(),
                0,
                false,
                &[],
                false
            )
            .is_ok()
        );
    }
}
