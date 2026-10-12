//! Backend-neutral RAW decoding contract.

use std::path::Path;

use serde::Serialize;
use thiserror::Error;

/// Maximum source size accepted before a decoder inspects the input.
pub const MAX_RAW_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Capabilities advertised by a decoder backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct DecoderCapabilities {
    /// Metadata extraction without decoding the mosaic.
    pub metadata: bool,
    /// Embedded preview or thumbnail extraction without decoding the mosaic.
    pub embedded_preview: bool,
    /// Full sensor mosaic decoding.
    pub mosaic: bool,
}

/// EXIF orientation represented without backend types.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    /// Top-left origin.
    Normal,
    /// Mirrored horizontally.
    HorizontalFlip,
    /// Rotated 180 degrees.
    Rotate180,
    /// Mirrored vertically.
    VerticalFlip,
    /// Transposed across the top-left diagonal.
    Transpose,
    /// Rotated 90 degrees clockwise.
    Rotate90,
    /// Transposed across the anti-diagonal.
    Transverse,
    /// Rotated 270 degrees clockwise.
    Rotate270,
    /// Missing or invalid orientation.
    Unknown,
}

/// A rectangular usable sensor region.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ActiveArea {
    /// Left coordinate in sensor pixels.
    pub x: u32,
    /// Top coordinate in sensor pixels.
    pub y: u32,
    /// Region width.
    pub width: u32,
    /// Region height.
    pub height: u32,
}

/// Normalized metadata required by the reference pipeline.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RawMetadata {
    /// Decoder backend used for this result.
    pub backend: &'static str,
    /// Camera-reported manufacturer.
    pub make: String,
    /// Camera-reported model.
    pub model: String,
    /// Decoder-normalized recording mode, such as `raw` or `craw`.
    pub mode: String,
    /// Full stored sensor width.
    pub width: u32,
    /// Full stored sensor height.
    pub height: u32,
    /// Components stored for each pixel.
    pub components_per_pixel: u8,
    /// Nominal stored bits per component.
    pub bits_per_component: u8,
    /// EXIF orientation.
    pub orientation: Orientation,
    /// Usable sensor rectangle when declared by the format.
    pub active_area: Option<ActiveArea>,
    /// Camera-recommended presentation crop, when distinct from active area.
    pub crop_area: Option<ActiveArea>,
    /// Repeating black levels in row-major CFA order.
    pub black_levels: Vec<f32>,
    /// White levels, either scalar or per component/CFA plane.
    pub white_levels: Vec<f32>,
    /// As-shot white-balance coefficients in RGB order.
    pub white_balance: [f32; 3],
    /// Repeating CFA pattern, for example `RGGB`.
    pub cfa_pattern: Option<String>,
    /// D65 matrix mapping XYZ values to camera components.
    pub xyz_to_camera_d65: [[f32; 3]; 3],
}

/// Encoding of an extracted embedded preview.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewEncoding {
    /// Original JPEG bitstream copied from the RAW container.
    Jpeg,
    /// Interleaved 8-bit sRGB pixels when encoded bytes are unavailable.
    Rgb8,
}

/// An embedded preview, preferably preserved in its source encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbeddedPreview {
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Encoding of [`Self::bytes`].
    pub encoding: PreviewEncoding,
    /// Encoded image or interleaved RGB bytes.
    pub bytes: Vec<u8>,
}

/// A decoded integer sensor image.
#[derive(Clone, Debug, PartialEq)]
pub struct Mosaic {
    /// Metadata describing the sensor samples.
    pub metadata: RawMetadata,
    /// Interleaved samples. Bayer images have one sample per pixel.
    pub samples: Vec<u16>,
}

impl Mosaic {
    /// Ensures the decoded allocation agrees with its dimensions.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::InvalidDimensions`] when the multiplication
    /// overflows or the decoded sample count differs from metadata.
    pub fn validate(&self) -> Result<(), DecodeError> {
        let expected = usize::try_from(self.metadata.width)
            .ok()
            .and_then(|width| {
                usize::try_from(self.metadata.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(usize::from(self.metadata.components_per_pixel)))
            .ok_or(DecodeError::InvalidDimensions)?;
        if expected != self.samples.len() {
            return Err(DecodeError::SampleCount {
                expected,
                actual: self.samples.len(),
            });
        }
        Ok(())
    }
}

/// Errors exposed by every decoder backend.
#[derive(Debug, Error)]
pub enum DecodeError {
    /// Source metadata could not be read.
    #[error("could not inspect {path}: {source}")]
    Io {
        /// Source path.
        path: String,
        /// Underlying filesystem error.
        source: std::io::Error,
    },
    /// Empty inputs are never valid RAW files.
    #[error("RAW input is empty: {0}")]
    Empty(String),
    /// The input exceeds the defensive parser limit.
    #[error("RAW input is too large ({bytes} bytes; limit is {limit}): {path}")]
    TooLarge {
        /// Source path.
        path: String,
        /// Observed byte count.
        bytes: u64,
        /// Configured limit.
        limit: u64,
    },
    /// The backend recognizes no supported camera/format combination.
    #[error("unsupported RAW input: {0}")]
    Unsupported(String),
    /// The input is malformed or truncated.
    #[error("corrupt or truncated RAW input: {0}")]
    Corrupt(String),
    /// The backend panicked while handling untrusted input.
    #[error("decoder aborted while handling malformed input")]
    DecoderPanic,
    /// Version one requires integer sensor samples.
    #[error("floating-point RAW mosaics are not supported by the integer sensor contract")]
    FloatingPointMosaic,
    /// Dimensions cannot describe a safe allocation.
    #[error("RAW dimensions overflow the addressable sample count")]
    InvalidDimensions,
    /// Backend output length disagrees with metadata.
    #[error("decoder returned {actual} samples; metadata requires {expected}")]
    SampleCount {
        /// Required samples.
        expected: usize,
        /// Returned samples.
        actual: usize,
    },
    /// The file has no directly extractable embedded preview.
    #[error("RAW input has no embedded preview")]
    PreviewUnavailable,
}

/// Backend-independent RAW operations.
pub trait RawDecoder: Send + Sync {
    /// Stable backend identifier used in diagnostics and cache keys.
    fn name(&self) -> &'static str;

    /// Capabilities available in this backend build.
    fn capabilities(&self) -> DecoderCapabilities;

    /// Extracts metadata without decoding the full mosaic.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] for invalid, unsupported, or unreadable inputs.
    fn metadata(&self, path: &Path) -> Result<RawMetadata, DecodeError>;

    /// Extracts a directly embedded preview without decoding the full mosaic.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] for invalid inputs or when no preview exists.
    fn embedded_preview(&self, path: &Path) -> Result<EmbeddedPreview, DecodeError>;

    /// Decodes the full sensor mosaic.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] for invalid inputs or inconsistent backend output.
    fn mosaic(&self, path: &Path) -> Result<Mosaic, DecodeError>;
}

/// Rejects paths which are unsafe or pointless to pass into a decoder.
///
/// # Errors
///
/// Returns a classified I/O, empty-file, or excessive-size error.
pub(crate) fn preflight(path: &Path) -> Result<(), DecodeError> {
    let metadata = std::fs::metadata(path).map_err(|source| DecodeError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let bytes = metadata.len();
    if bytes == 0 {
        return Err(DecodeError::Empty(path.display().to_string()));
    }
    if bytes > MAX_RAW_FILE_BYTES {
        return Err(DecodeError::TooLarge {
            path: path.display().to_string(),
            bytes,
            limit: MAX_RAW_FILE_BYTES,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{DecodeError, MAX_RAW_FILE_BYTES, Mosaic, Orientation, RawMetadata, preflight};
    use std::io::Write;

    fn metadata(width: u32, height: u32) -> RawMetadata {
        RawMetadata {
            backend: "test",
            make: String::new(),
            model: String::new(),
            mode: String::new(),
            width,
            height,
            components_per_pixel: 1,
            bits_per_component: 16,
            orientation: Orientation::Normal,
            active_area: None,
            crop_area: None,
            black_levels: vec![0.0],
            white_levels: vec![65_535.0],
            white_balance: [1.0; 3],
            cfa_pattern: Some("RGGB".into()),
            xyz_to_camera_d65: [[0.0; 3]; 3],
        }
    }

    #[test]
    fn mosaic_length_is_checked() {
        let mosaic = Mosaic {
            metadata: metadata(2, 2),
            samples: vec![0; 3],
        };
        assert!(matches!(
            mosaic.validate(),
            Err(DecodeError::SampleCount {
                expected: 4,
                actual: 3
            })
        ));
    }

    #[test]
    fn zero_length_input_is_classified_before_decoder() {
        let file = tempfile::NamedTempFile::new().expect("create fixture");
        assert!(matches!(preflight(file.path()), Err(DecodeError::Empty(_))));
    }

    #[test]
    fn excessive_input_is_classified_before_decoder() {
        let mut file = tempfile::NamedTempFile::new().expect("create fixture");
        file.write_all(b"raw").expect("seed sparse file");
        file.as_file()
            .set_len(MAX_RAW_FILE_BYTES + 1)
            .expect("extend sparse fixture");
        assert!(matches!(
            preflight(file.path()),
            Err(DecodeError::TooLarge { .. })
        ));
    }
}
