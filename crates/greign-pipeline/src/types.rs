//! Image-domain types and their invariants.

use greign_raw::Orientation;
use thiserror::Error;

/// One sensor color-filter plane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CfaColor {
    /// Red-filtered sample.
    Red,
    /// Green-filtered sample.
    Green,
    /// Blue-filtered sample.
    Blue,
}

/// A repeating color-filter-array pattern.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CfaPattern {
    width: u32,
    height: u32,
    colors: Vec<CfaColor>,
}

impl CfaPattern {
    /// Parses a Bayer (2×2) or X-Trans (6×6) pattern.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::UnsupportedCfa`] for an invalid size or color.
    pub fn parse(pattern: &str) -> Result<Self, PipelineError> {
        let (width, height) = match pattern.len() {
            4 => (2, 2),
            36 => (6, 6),
            _ => return Err(PipelineError::UnsupportedCfa(pattern.into())),
        };
        let colors = pattern
            .bytes()
            .map(|byte| match byte {
                b'R' => Ok(CfaColor::Red),
                b'G' => Ok(CfaColor::Green),
                b'B' => Ok(CfaColor::Blue),
                _ => Err(PipelineError::UnsupportedCfa(pattern.into())),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            width,
            height,
            colors,
        })
    }

    /// Pattern width.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Pattern height.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Number of samples in one repeat.
    #[must_use]
    pub fn len(&self) -> usize {
        self.colors.len()
    }

    /// Whether the pattern has no samples.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }

    /// Color at an unbounded sensor coordinate.
    ///
    /// # Panics
    ///
    /// Panics only if this value was constructed internally with an invalid
    /// empty or inconsistent pattern; [`Self::parse`] never creates one.
    #[must_use]
    pub fn color_at(&self, x: u32, y: u32) -> CfaColor {
        let index = (y % self.height) * self.width + (x % self.width);
        self.colors[usize::try_from(index).expect("small CFA index")]
    }

    /// Returns the same pattern with a new origin after a crop.
    #[must_use]
    pub fn shifted(&self, x: u32, y: u32) -> Self {
        let mut colors = Vec::with_capacity(self.colors.len());
        for row in 0..self.height {
            for column in 0..self.width {
                colors.push(self.color_at(column + x, row + y));
            }
        }
        Self {
            width: self.width,
            height: self.height,
            colors,
        }
    }
}

/// Integer coordinate of a known bad sensor pixel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SensorPoint {
    /// Horizontal coordinate.
    pub x: u32,
    /// Vertical coordinate.
    pub y: u32,
}

/// Linear normalized sensor plane.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedMosaic {
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// CFA aligned to the current top-left pixel.
    pub cfa: CfaPattern,
    /// Row-major linear samples, intentionally not clamped.
    pub samples: Vec<f32>,
}

impl NormalizedMosaic {
    /// Indexes a sample after bounds have been established.
    #[must_use]
    pub(crate) fn index(&self, x: u32, y: u32) -> usize {
        usize::try_from(y * self.width + x).expect("image dimensions validated")
    }
}

/// Linear scene-referred RGB image.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearRgbImage {
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Row-major RGB pixels, intentionally not clamped.
    pub pixels: Vec<[f32; 3]>,
}

/// Bounded, gamma-encoded sRGB image ready for output.
#[derive(Clone, Debug, PartialEq)]
pub struct SrgbImage {
    /// Pixel width after orientation.
    pub width: u32,
    /// Pixel height after orientation.
    pub height: u32,
    /// Row-major encoded values in `[0, 1]`.
    pub pixels: Vec<[f32; 3]>,
}

impl SrgbImage {
    /// Quantizes to interleaved 8-bit RGB.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn to_rgb8(&self) -> Vec<u8> {
        self.pixels
            .iter()
            .flat_map(|pixel| pixel.map(|value| (value * 255.0).round() as u8))
            .collect()
    }

    /// Quantizes to interleaved 16-bit RGB.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn to_rgb16(&self) -> Vec<u16> {
        self.pixels
            .iter()
            .flat_map(|pixel| pixel.map(|value| (value * 65_535.0).round() as u16))
            .collect()
    }

    /// Applies EXIF orientation to display coordinates.
    ///
    /// # Panics
    ///
    /// Panics only if dimensions and the pixel buffer disagree. Pipeline
    /// constructors maintain this invariant.
    #[must_use]
    pub fn oriented(self, orientation: Orientation) -> Self {
        if matches!(orientation, Orientation::Normal | Orientation::Unknown) {
            return self;
        }
        let swaps_axes = matches!(
            orientation,
            Orientation::Transpose
                | Orientation::Rotate90
                | Orientation::Transverse
                | Orientation::Rotate270
        );
        let (output_width, output_height) = if swaps_axes {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };
        let mut pixels = vec![[0.0; 3]; self.pixels.len()];
        for y in 0..output_height {
            for x in 0..output_width {
                let (source_x, source_y) = match orientation {
                    Orientation::Normal | Orientation::Unknown => (x, y),
                    Orientation::HorizontalFlip => (self.width - 1 - x, y),
                    Orientation::Rotate180 => (self.width - 1 - x, self.height - 1 - y),
                    Orientation::VerticalFlip => (x, self.height - 1 - y),
                    Orientation::Transpose => (y, x),
                    Orientation::Rotate90 => (y, self.height - 1 - x),
                    Orientation::Transverse => (self.width - 1 - y, self.height - 1 - x),
                    Orientation::Rotate270 => (self.width - 1 - y, x),
                };
                let source = usize::try_from(source_y * self.width + source_x)
                    .expect("source index validated");
                let destination =
                    usize::try_from(y * output_width + x).expect("destination index validated");
                pixels[destination] = self.pixels[source];
            }
        }
        Self {
            width: output_width,
            height: output_height,
            pixels,
        }
    }
}

/// Numerical or representation failures in the reference pipeline.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum PipelineError {
    /// The input is not a single-plane integer mosaic.
    #[error("reference pipeline requires one sensor component per pixel")]
    UnsupportedComponents,
    /// No supported CFA can be derived.
    #[error("unsupported CFA pattern: {0}")]
    UnsupportedCfa(String),
    /// Dimensions or buffer lengths are inconsistent.
    #[error("image dimensions or sample count are invalid")]
    InvalidDimensions,
    /// Black/white level arrays cannot map to the CFA.
    #[error("black or white levels do not match the CFA repeat")]
    InvalidLevels,
    /// Crop rectangle escapes the source image.
    #[error("active-area crop is outside the sensor image")]
    InvalidCrop,
    /// A parameter is NaN or infinite.
    #[error("pipeline parameter must be finite")]
    NonFiniteParameter,
    /// Camera matrix is singular or non-finite.
    #[error("camera color matrix is singular or non-finite")]
    InvalidCameraMatrix,
    /// Capture sharpening amount is outside its conservative range.
    #[error("capture sharpening amount must be finite and between 0 and 2")]
    InvalidSharpening,
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use greign_raw::Orientation;

    use super::SrgbImage;

    fn labeled_image() -> SrgbImage {
        SrgbImage {
            width: 2,
            height: 3,
            pixels: (0_u8..6)
                .map(|label| [f32::from(label), 0.0, 0.0])
                .collect(),
        }
    }

    fn labels(image: &SrgbImage) -> Vec<f32> {
        image.pixels.iter().map(|pixel| pixel[0]).collect()
    }

    #[test]
    fn rotate_90_maps_odd_dimensions_clockwise() {
        let image = labeled_image().oriented(Orientation::Rotate90);
        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!(labels(&image), vec![4.0, 2.0, 0.0, 5.0, 3.0, 1.0]);
    }

    #[test]
    fn rotate_270_maps_odd_dimensions_counterclockwise() {
        let image = labeled_image().oriented(Orientation::Rotate270);
        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!(labels(&image), vec![1.0, 3.0, 5.0, 0.0, 2.0, 4.0]);
    }
}
