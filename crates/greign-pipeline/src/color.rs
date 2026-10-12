//! Scene-linear color and tone operations.

use crate::{LinearRgbImage, PipelineError, SrgbImage};

const REC2020_TO_XYZ: [[f32; 3]; 3] = [
    [0.636_958_06, 0.144_616_9, 0.168_880_97],
    [0.262_700_2, 0.677_998_07, 0.059_301_715],
    [0.0, 0.028_072_694, 1.060_985_1],
];

const REC2020_TO_SRGB: [[f32; 3]; 3] = [
    [1.660_491, -0.587_641_1, -0.072_849_86],
    [-0.124_550_47, 1.132_899_9, -0.008_349_42],
    [-0.018_150_76, -0.100_578_9, 1.118_729_7],
];

/// Converts camera RGB to the linear Rec. 2020 D65 working space.
///
/// # Errors
///
/// Returns [`PipelineError::InvalidCameraMatrix`] when inversion is unsafe.
pub fn camera_to_rec2020(
    image: &mut LinearRgbImage,
    xyz_to_camera: [[f32; 3]; 3],
) -> Result<(), PipelineError> {
    let camera_to_working = camera_to_rec2020_matrix(xyz_to_camera)?;
    apply_matrix(image, camera_to_working);
    Ok(())
}

/// Builds the linear camera-RGB to Rec. 2020 D65 transform.
///
/// # Errors
///
/// Returns [`PipelineError::InvalidCameraMatrix`] when inversion is unsafe.
pub fn camera_to_rec2020_matrix(
    xyz_to_camera: [[f32; 3]; 3],
) -> Result<[[f32; 3]; 3], PipelineError> {
    // dcraw-style camera matrices are normalized after composition with the
    // target working primaries. This makes a white-balanced camera neutral
    // map to a neutral working-space value before inversion.
    let mut working_to_camera = multiply_3x3(xyz_to_camera, REC2020_TO_XYZ);
    for row in &mut working_to_camera {
        let sum: f32 = row.iter().sum();
        if !sum.is_finite() || sum.abs() < 1.0e-9 {
            return Err(PipelineError::InvalidCameraMatrix);
        }
        for value in row {
            *value /= sum;
        }
    }
    invert_3x3(working_to_camera)
}

/// Applies exposure in stops without clamping scene values.
///
/// # Errors
///
/// Returns [`PipelineError::NonFiniteParameter`] for NaN or infinity.
pub fn apply_exposure(image: &mut LinearRgbImage, exposure_ev: f32) -> Result<(), PipelineError> {
    if !exposure_ev.is_finite() {
        return Err(PipelineError::NonFiniteParameter);
    }
    let multiplier = exposure_ev.exp2();
    if !multiplier.is_finite() {
        return Err(PipelineError::NonFiniteParameter);
    }
    for pixel in &mut image.pixels {
        for channel in pixel {
            *channel *= multiplier;
        }
    }
    Ok(())
}

/// Applies a hue-preserving signed Reinhard scene-to-display curve.
///
/// One scale derived from the largest absolute RGB component is applied to all
/// three channels. This keeps channel ratios stable instead of progressively
/// neutralizing bright colors through independent per-channel compression.
pub fn tone_curve(image: &mut LinearRgbImage) {
    for pixel in &mut image.pixels {
        let peak = pixel.iter().copied().map(f32::abs).fold(0.0_f32, f32::max);
        let scale = 1.0 / (1.0 + peak);
        for channel in pixel {
            *channel *= scale;
        }
    }
}

/// White-anchored shoulder for photographic quality rendering.
///
/// Extended Reinhard maps a scene neutral of two to display white. One scale
/// preserves RGB ratios below output gamut limits; reconstructed highlights
/// no longer asymptotically approach white from a conspicuous gray plateau.
pub fn highlight_tone_curve(image: &mut LinearRgbImage) {
    for pixel in &mut image.pixels {
        let peak = pixel.iter().copied().map(f32::abs).fold(0.0_f32, f32::max);
        let scale = (1.0 + peak * 0.25) / (1.0 + peak);
        for channel in pixel {
            *channel *= scale;
        }
    }
}

/// Converts working RGB to bounded, gamma-encoded sRGB.
#[must_use]
pub fn rec2020_to_srgb(mut image: LinearRgbImage) -> SrgbImage {
    apply_matrix(&mut image, REC2020_TO_SRGB);
    let pixels = image
        .pixels
        .into_iter()
        .map(|pixel| pixel.map(|channel| srgb_oetf(channel.clamp(0.0, 1.0))))
        .collect();
    SrgbImage {
        width: image.width,
        height: image.height,
        pixels,
    }
}

fn apply_matrix(image: &mut LinearRgbImage, matrix: [[f32; 3]; 3]) {
    for pixel in &mut image.pixels {
        *pixel = [
            dot(matrix[0], *pixel),
            dot(matrix[1], *pixel),
            dot(matrix[2], *pixel),
        ];
    }
}

const fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn invert_3x3(matrix: [[f32; 3]; 3]) -> Result<[[f32; 3]; 3], PipelineError> {
    if matrix.iter().flatten().any(|value| !value.is_finite()) {
        return Err(PipelineError::InvalidCameraMatrix);
    }
    let determinant = matrix[0][0] * (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1])
        - matrix[0][1] * (matrix[1][0] * matrix[2][2] - matrix[1][2] * matrix[2][0])
        + matrix[0][2] * (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0]);
    if !determinant.is_finite() || determinant.abs() < 1.0e-9 {
        return Err(PipelineError::InvalidCameraMatrix);
    }
    let inverse = [
        [
            matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1],
            matrix[0][2] * matrix[2][1] - matrix[0][1] * matrix[2][2],
            matrix[0][1] * matrix[1][2] - matrix[0][2] * matrix[1][1],
        ],
        [
            matrix[1][2] * matrix[2][0] - matrix[1][0] * matrix[2][2],
            matrix[0][0] * matrix[2][2] - matrix[0][2] * matrix[2][0],
            matrix[0][2] * matrix[1][0] - matrix[0][0] * matrix[1][2],
        ],
        [
            matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0],
            matrix[0][1] * matrix[2][0] - matrix[0][0] * matrix[2][1],
            matrix[0][0] * matrix[1][1] - matrix[0][1] * matrix[1][0],
        ],
    ];
    Ok(inverse.map(|row| row.map(|value| value / determinant)))
}

fn multiply_3x3(left: [[f32; 3]; 3], right: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut result = [[0.0_f32; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            result[row][column] = left[row][0] * right[0][column]
                + left[row][1] * right[1][column]
                + left[row][2] * right[2][column];
        }
    }
    result
}

fn srgb_oetf(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::highlight_tone_curve;
    use super::{apply_exposure, camera_to_rec2020, rec2020_to_srgb, tone_curve};

    #[test]
    fn photographic_shoulder_reaches_white_and_retains_warm_channel_order() {
        let mut neutral = image([2.0; 3]);
        highlight_tone_curve(&mut neutral);
        assert_eq!(neutral.pixels[0], [1.0; 3]);
        let mut warm = image([1.8, 1.5, 0.8]);
        highlight_tone_curve(&mut warm);
        let rgb = rec2020_to_srgb(warm).pixels[0];
        assert!(rgb[0] > rgb[1] && rgb[1] > rgb[2]);
        assert!(rgb[0] > 0.95);
    }
    use crate::{LinearRgbImage, PipelineError};

    fn image(pixel: [f32; 3]) -> LinearRgbImage {
        LinearRgbImage {
            width: 1,
            height: 1,
            pixels: vec![pixel],
        }
    }

    #[test]
    fn exposure_preserves_negative_and_high_values() {
        let mut image = image([-0.5, 0.5, 2.0]);
        apply_exposure(&mut image, 1.0).expect("exposure");
        assert_eq!(image.pixels[0], [-1.0, 1.0, 4.0]);
    }

    #[test]
    fn non_finite_exposure_is_rejected() {
        assert_eq!(
            apply_exposure(&mut image([0.0; 3]), f32::NAN),
            Err(PipelineError::NonFiniteParameter)
        );
    }

    #[test]
    fn singular_camera_matrix_is_rejected() {
        assert_eq!(
            camera_to_rec2020(&mut image([1.0; 3]), [[0.0; 3]; 3]),
            Err(PipelineError::InvalidCameraMatrix)
        );
    }

    #[test]
    fn white_balanced_neutral_stays_neutral() {
        let mut image = image([0.5; 3]);
        let r7 = [
            [1.0424, -0.3138, -0.13],
            [-0.4221, 1.1938, 0.2584],
            [-0.0547, 0.1658, 0.6183],
        ];
        camera_to_rec2020(&mut image, r7).expect("camera transform");
        let [red, green, blue] = image.pixels[0];
        assert!((red - green).abs() < 1.0e-6);
        assert!((green - blue).abs() < 1.0e-6);
    }

    #[test]
    fn tone_curve_is_signed_and_bounded() {
        let mut image = image([-2.0, 0.0, 2.0]);
        tone_curve(&mut image);
        assert_eq!(image.pixels[0], [-2.0 / 3.0, 0.0, 2.0 / 3.0]);
    }

    #[test]
    fn tone_curve_preserves_bright_channel_ratios() {
        let mut image = image([2.0, 1.0, 0.5]);
        tone_curve(&mut image);
        let [red, green, blue] = image.pixels[0];
        assert!((red / green - 2.0).abs() < 1.0e-6);
        assert!((green / blue - 2.0).abs() < 1.0e-6);
    }

    #[test]
    fn final_output_clamps_only_at_display_boundary() {
        let output = rec2020_to_srgb(image([-1.0, 0.5, 2.0]));
        assert!(
            output.pixels[0]
                .iter()
                .all(|value| (0.0..=1.0).contains(value))
        );
    }
}
