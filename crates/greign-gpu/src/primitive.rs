//! Validated parameter layout shared with the WGSL primitive kernel.

use std::fmt;

/// Operation selector encoded in the shader uniform.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ShaderOperation {
    /// Black/white normalization.
    Normalize = 0,
    /// Per-channel white balance.
    WhiteBalance = 1,
    /// Exposure in stops.
    Exposure = 2,
    /// Signed Reinhard curve.
    ToneCurve = 3,
    /// Three-by-three color matrix.
    ColorMatrix = 4,
}

/// One validated primitive invocation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Primitive {
    /// `(value - black) / (white - black)` per RGB channel.
    Normalize {
        /// Per-channel black level.
        black: [f32; 3],
        /// Per-channel white level.
        white: [f32; 3],
    },
    /// Per-channel RGB multipliers.
    WhiteBalance([f32; 3]),
    /// Exposure in stops.
    Exposure(f32),
    /// Parameter-free signed tone curve.
    ToneCurve,
    /// Row-major three-by-three matrix.
    ColorMatrix([[f32; 3]; 3]),
}

/// Exact 64-byte host layout expected by `Params` in `primitives.wgsl`.
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct ShaderParams {
    /// Image width.
    pub width: u32,
    /// Image height.
    pub height: u32,
    /// [`ShaderOperation`] discriminant.
    pub operation: u32,
    /// Uniform alignment padding.
    pub padding: u32,
    /// First parameter vector or matrix row.
    pub a: [f32; 4],
    /// Second parameter vector or matrix row.
    pub b: [f32; 4],
    /// Third matrix row.
    pub c: [f32; 4],
}

/// Invalid shader parameters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimitiveError {
    /// Empty dimensions cannot be dispatched.
    EmptyImage,
    /// Parameter contains NaN or infinity.
    NonFinite,
    /// A white level is not greater than its black level.
    InvalidLevels,
    /// White-balance coefficient is not positive.
    InvalidWhiteBalance,
}

impl fmt::Display for PrimitiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyImage => formatter.write_str("GPU primitive image is empty"),
            Self::NonFinite => formatter.write_str("GPU primitive parameter is not finite"),
            Self::InvalidLevels => formatter.write_str("GPU white level must exceed black level"),
            Self::InvalidWhiteBalance => {
                formatter.write_str("GPU white-balance coefficients must be positive")
            }
        }
    }
}

impl std::error::Error for PrimitiveError {}

impl ShaderParams {
    /// Packs validated primitive parameters for the GPU uniform buffer.
    ///
    /// # Errors
    ///
    /// Returns [`PrimitiveError`] for empty dimensions, non-finite values, or
    /// invalid operator-specific bounds.
    pub fn new(width: u32, height: u32, primitive: Primitive) -> Result<Self, PrimitiveError> {
        if width == 0 || height == 0 {
            return Err(PrimitiveError::EmptyImage);
        }
        let (operation, a, b, c) = match primitive {
            Primitive::Normalize { black, white } => {
                finite(&black)?;
                finite(&white)?;
                if black
                    .iter()
                    .zip(white)
                    .any(|(black, white)| white <= *black)
                {
                    return Err(PrimitiveError::InvalidLevels);
                }
                (
                    ShaderOperation::Normalize,
                    vector(black),
                    vector(white),
                    [0.0; 4],
                )
            }
            Primitive::WhiteBalance(coefficients) => {
                finite(&coefficients)?;
                if coefficients.iter().any(|value| *value <= 0.0) {
                    return Err(PrimitiveError::InvalidWhiteBalance);
                }
                (
                    ShaderOperation::WhiteBalance,
                    vector(coefficients),
                    [0.0; 4],
                    [0.0; 4],
                )
            }
            Primitive::Exposure(exposure) => {
                finite(&[exposure])?;
                (
                    ShaderOperation::Exposure,
                    [exposure, 0.0, 0.0, 0.0],
                    [0.0; 4],
                    [0.0; 4],
                )
            }
            Primitive::ToneCurve => (ShaderOperation::ToneCurve, [0.0; 4], [0.0; 4], [0.0; 4]),
            Primitive::ColorMatrix(matrix) => {
                finite(&matrix.concat())?;
                (
                    ShaderOperation::ColorMatrix,
                    vector(matrix[0]),
                    vector(matrix[1]),
                    vector(matrix[2]),
                )
            }
        };
        Ok(Self {
            width,
            height,
            operation: operation as u32,
            padding: 0,
            a,
            b,
            c,
        })
    }

    /// Applies the same scalar math as the WGSL kernel for parity fixtures.
    #[must_use]
    pub fn apply_reference(self, input: [f32; 4]) -> [f32; 4] {
        let rgb = [input[0], input[1], input[2]];
        let output = match self.operation {
            operation if operation == ShaderOperation::Normalize as u32 => [
                (rgb[0] - self.a[0]) / (self.b[0] - self.a[0]),
                (rgb[1] - self.a[1]) / (self.b[1] - self.a[1]),
                (rgb[2] - self.a[2]) / (self.b[2] - self.a[2]),
            ],
            operation if operation == ShaderOperation::WhiteBalance as u32 => {
                [rgb[0] * self.a[0], rgb[1] * self.a[1], rgb[2] * self.a[2]]
            }
            operation if operation == ShaderOperation::Exposure as u32 => {
                let multiplier = self.a[0].exp2();
                [
                    rgb[0] * multiplier,
                    rgb[1] * multiplier,
                    rgb[2] * multiplier,
                ]
            }
            operation if operation == ShaderOperation::ToneCurve as u32 => {
                rgb.map(|value| value / (1.0 + value.abs()))
            }
            operation if operation == ShaderOperation::ColorMatrix as u32 => {
                [dot(self.a, rgb), dot(self.b, rgb), dot(self.c, rgb)]
            }
            _ => rgb,
        };
        [output[0], output[1], output[2], input[3]]
    }
}

const _: () = assert!(size_of::<ShaderParams>() == 64);

fn finite(values: &[f32]) -> Result<(), PrimitiveError> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(PrimitiveError::NonFinite)
    }
}

const fn vector(values: [f32; 3]) -> [f32; 4] {
    [values[0], values[1], values[2], 0.0]
}

const fn dot(row: [f32; 4], value: [f32; 3]) -> f32 {
    row[0] * value[0] + row[1] * value[1] + row[2] * value[2]
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::{Primitive, PrimitiveError, ShaderOperation, ShaderParams};

    #[test]
    fn uniform_layout_is_exactly_four_vec4_slots() {
        assert_eq!(size_of::<ShaderParams>(), 64);
        let params = ShaderParams::new(17, 31, Primitive::Exposure(1.0)).expect("params");
        assert_eq!(params.operation, ShaderOperation::Exposure as u32);
        assert_eq!(params.padding, 0);
    }

    #[test]
    fn normalization_preserves_out_of_range_values() {
        let params = ShaderParams::new(
            1,
            1,
            Primitive::Normalize {
                black: [64.0; 3],
                white: [1_064.0; 3],
            },
        )
        .expect("params");
        assert_eq!(
            params.apply_reference([0.0, 564.0, 2_064.0, 9.0]),
            [-0.064, 0.5, 2.0, 9.0]
        );
    }

    #[test]
    fn invalid_parameters_never_reach_shader() {
        assert_eq!(
            ShaderParams::new(0, 1, Primitive::ToneCurve),
            Err(PrimitiveError::EmptyImage)
        );
        assert_eq!(
            ShaderParams::new(1, 1, Primitive::Exposure(f32::NAN)),
            Err(PrimitiveError::NonFinite)
        );
        assert_eq!(
            ShaderParams::new(
                1,
                1,
                Primitive::Normalize {
                    black: [1.0; 3],
                    white: [1.0; 3],
                }
            ),
            Err(PrimitiveError::InvalidLevels)
        );
    }
}
