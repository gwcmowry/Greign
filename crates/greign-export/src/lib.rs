//! Color-managed JPEG and 16-bit TIFF proof export.

mod capture_date;
mod srgb_profile;
pub use capture_date::CaptureDate;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use greign_pipeline::SrgbImage;
use image::ImageEncoder;
use image::codecs::jpeg::JpegEncoder;
use thiserror::Error;
use tiff::encoder::{TiffEncoder, colortype};
use tiff::tags::Tag;
mod dng;
pub use dng::export_linear_dng;
pub use dng::export_linear_dng_dated;

pub use srgb_profile::srgb_icc;

/// Default JPEG quality for proof and final exports.
pub const DEFAULT_JPEG_QUALITY: u8 = 95;

/// Proof-export settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportOptions {
    /// JPEG quality from 1 through 100.
    pub jpeg_quality: u8,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            jpeg_quality: DEFAULT_JPEG_QUALITY,
        }
    }
}

/// Export failures.
#[derive(Debug, Error)]
pub enum ExportError {
    /// Output path has no supported extension.
    #[error("output extension must be .jpg, .jpeg, .tif, or .tiff")]
    UnsupportedExtension,
    /// JPEG quality is outside its valid range.
    #[error("JPEG quality must be between 1 and 100")]
    InvalidQuality,
    /// Pixel length does not match dimensions.
    #[error("RGB pixel length does not match output dimensions")]
    InvalidDimensions,
    /// Linear export contains non-finite samples.
    #[error("linear export contains NaN or infinite samples")]
    InvalidSamples,
    /// Output file could not be created or finalized.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JPEG encoding failed.
    #[error(transparent)]
    Image(#[from] image::ImageError),
    /// TIFF encoding failed.
    #[error(transparent)]
    Tiff(#[from] tiff::TiffError),
    /// The native `MozJPEG` error manager unwound on invalid state.
    #[error("MozJPEG encoder aborted")]
    MozJpegPanic,
}

/// Writes an already color-managed interleaved RGB8 JPEG without a redundant
/// floating-point quantization pass.
///
/// # Errors
///
/// Returns [`ExportError`] for invalid dimensions, quality, I/O, or codec
/// failures.
pub fn export_rgb8_jpeg(
    width: u32,
    height: u32,
    pixels: &[u8],
    path: &Path,
    quality: u8,
) -> Result<(), ExportError> {
    validate_rgb8(width, height, pixels)?;
    encode_jpeg(width, height, pixels, path, quality)
}

/// Writes RGB8 through `MozJPEG`'s fastest SIMD profile.
///
/// This path retains baseline JPEG compatibility and the same sRGB ICC
/// profile while avoiding the scalar encoder used by the reference oracle.
///
/// # Errors
///
/// Returns [`ExportError`] for invalid dimensions, quality, I/O, codec, or
/// native encoder failures.
pub fn export_rgb8_jpeg_fast(
    width: u32,
    height: u32,
    pixels: &[u8],
    path: &Path,
    quality: u8,
) -> Result<(), ExportError> {
    export_rgb8_jpeg_fast_dated(width, height, pixels, path, quality, None)
}
/// JPEG export with the original film capture date.
/// # Errors
/// Invalid pixels/quality or encoder/I/O failure.
pub fn export_rgb8_jpeg_fast_dated(
    width: u32,
    height: u32,
    pixels: &[u8],
    path: &Path,
    quality: u8,
    date: Option<&CaptureDate>,
) -> Result<(), ExportError> {
    validate_rgb8(width, height, pixels)?;
    if !(1..=100).contains(&quality) {
        return Err(ExportError::InvalidQuality);
    }
    let encoded = std::panic::catch_unwind(|| -> std::io::Result<Vec<u8>> {
        const ICC_HEADER_BYTES: usize = 14;
        const MAX_MARKER_DATA_BYTES: usize = 65_533;

        let mut compressor = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
        compressor.set_size(
            usize::try_from(width).map_err(std::io::Error::other)?,
            usize::try_from(height).map_err(std::io::Error::other)?,
        );
        compressor.set_fastest_defaults();
        compressor.set_quality(f32::from(quality));
        let mut started = compressor.start_compress(Vec::new())?;
        if let Some(date) = date {
            started.write_marker(mozjpeg::Marker::APP(1), &date.jpeg_payload());
        }
        // mozjpeg 0.10.13 numbers ICC chunks from zero, but ICC.1 requires
        // one-based sequence numbers. Write the APP2 markers ourselves so
        // standards-compliant decoders can discover the embedded profile.
        let profile = srgb_icc();
        let chunk_bytes = MAX_MARKER_DATA_BYTES - ICC_HEADER_BYTES;
        let chunk_count = profile.len().div_ceil(chunk_bytes);
        for (chunk_index, chunk) in profile.chunks(chunk_bytes).enumerate() {
            let mut marker = Vec::with_capacity(ICC_HEADER_BYTES + chunk.len());
            marker.extend_from_slice(b"ICC_PROFILE\0");
            marker.push(u8::try_from(chunk_index + 1).map_err(std::io::Error::other)?);
            marker.push(u8::try_from(chunk_count).map_err(std::io::Error::other)?);
            marker.extend_from_slice(chunk);
            started.write_marker(mozjpeg::Marker::APP(2), &marker);
        }
        started.write_scanlines(pixels)?;
        started.finish()
    })
    .map_err(|_| ExportError::MozJpegPanic)??;
    std::fs::write(path, encoded)?;
    Ok(())
}

fn validate_rgb8(width: u32, height: u32, pixels: &[u8]) -> Result<(), ExportError> {
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or(ExportError::InvalidDimensions)?;
    if expected == pixels.len() {
        Ok(())
    } else {
        Err(ExportError::InvalidDimensions)
    }
}

/// Writes JPEG or 16-bit TIFF based on the output extension.
///
/// Both formats embed the same CC0 sRGB ICC profile.
///
/// # Errors
///
/// Returns [`ExportError`] for invalid settings, unsupported extensions, I/O,
/// or codec failures.
pub fn export_srgb(
    image: &SrgbImage,
    path: &Path,
    options: ExportOptions,
) -> Result<(), ExportError> {
    export_srgb_dated(image, path, options, None)
}
/// Color-managed export with an optional original film date.
/// # Errors
/// Invalid settings or encoder/I/O failure.
pub fn export_srgb_dated(
    image: &SrgbImage,
    path: &Path,
    options: ExportOptions,
    date: Option<&CaptureDate>,
) -> Result<(), ExportError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .ok_or(ExportError::UnsupportedExtension)?;
    if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
        if date.is_some() {
            export_rgb8_jpeg_fast_dated(
                image.width,
                image.height,
                &image.to_rgb8(),
                path,
                options.jpeg_quality,
                date,
            )
        } else {
            export_jpeg(image, path, options.jpeg_quality)
        }
    } else if extension.eq_ignore_ascii_case("tif") || extension.eq_ignore_ascii_case("tiff") {
        export_tiff(image, path, date)
    } else {
        Err(ExportError::UnsupportedExtension)
    }
}

fn export_jpeg(image: &SrgbImage, path: &Path, quality: u8) -> Result<(), ExportError> {
    encode_jpeg(image.width, image.height, &image.to_rgb8(), path, quality)
}

fn encode_jpeg(
    width: u32,
    height: u32,
    pixels: &[u8],
    path: &Path,
    quality: u8,
) -> Result<(), ExportError> {
    if !(1..=100).contains(&quality) {
        return Err(ExportError::InvalidQuality);
    }
    let file = BufWriter::new(File::create(path)?);
    let mut encoder = JpegEncoder::new_with_quality(file, quality);
    encoder
        .set_icc_profile(srgb_icc())
        .map_err(image::ImageError::Unsupported)?;
    encoder.encode(pixels, width, height, image::ExtendedColorType::Rgb8)?;
    Ok(())
}

fn export_tiff(
    image: &SrgbImage,
    path: &Path,
    date: Option<&CaptureDate>,
) -> Result<(), ExportError> {
    let mut file = BufWriter::new(File::create(path)?);
    {
        let mut encoder = TiffEncoder::new(&mut file)?
            .with_compression(tiff::encoder::Compression::Deflate(
                tiff::encoder::compression::DeflateLevel::Fast,
            ))
            .with_predictor(tiff::encoder::Predictor::Horizontal);
        let exif = capture_date::tiff_ifd(&mut encoder, date)?;
        let mut output = encoder.new_image::<colortype::RGB16>(image.width, image.height)?;
        if let (Some(offset), Some(date)) = (exif, date) {
            output.encoder().write_tag(Tag::Unknown(34665), offset)?;
            output.encoder().write_tag(Tag::DateTime, date.as_exif())?;
        }
        output.rows_per_strip(64)?;
        let profile = srgb_icc();
        output
            .encoder()
            .write_tag(Tag::IccProfile, profile.as_slice())?;
        output.write_data(&image.to_rgb16())?;
    }
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::BufReader;

    use image::ImageDecoder;
    use image::codecs::jpeg::JpegDecoder;
    use image::codecs::tiff::TiffDecoder;

    use super::{
        DEFAULT_JPEG_QUALITY, ExportOptions, export_rgb8_jpeg_fast, export_srgb, srgb_icc,
    };
    use greign_pipeline::SrgbImage;

    fn fixture() -> SrgbImage {
        SrgbImage {
            width: 2,
            height: 1,
            pixels: vec![[0.0, 0.5, 1.0], [1.0, 0.5, 0.0]],
        }
    }

    #[test]
    fn export_default_is_quality_95() {
        assert_eq!(ExportOptions::default().jpeg_quality, 95);
        assert_eq!(DEFAULT_JPEG_QUALITY, 95);
    }

    #[test]
    fn jpeg_reopens_with_dimensions_and_icc_profile() {
        let directory = tempfile::tempdir().expect("temporary output");
        let path = directory.path().join("proof.jpg");
        export_srgb(&fixture(), &path, ExportOptions::default()).expect("JPEG export");
        let mut decoder = JpegDecoder::new(BufReader::new(File::open(path).expect("open JPEG")))
            .expect("decode JPEG");
        assert_eq!(decoder.dimensions(), (2, 1));
        assert_eq!(decoder.color_type(), image::ColorType::Rgb8);
        assert_eq!(decoder.icc_profile().expect("ICC read"), Some(srgb_icc()));
    }

    #[test]
    fn fast_jpeg_reopens_with_dimensions_and_icc_profile() {
        let directory = tempfile::tempdir().expect("temporary output");
        let path = directory.path().join("fast-proof.jpg");
        export_rgb8_jpeg_fast(2, 1, &fixture().to_rgb8(), &path, 92).expect("fast JPEG");
        let mut decoder = JpegDecoder::new(BufReader::new(File::open(path).expect("open JPEG")))
            .expect("decode JPEG");
        assert_eq!(decoder.dimensions(), (2, 1));
        assert_eq!(decoder.color_type(), image::ColorType::Rgb8);
        assert_eq!(decoder.icc_profile().expect("ICC read"), Some(srgb_icc()));
    }

    #[test]
    fn tiff_reopens_as_sixteen_bit_with_icc_profile() {
        let directory = tempfile::tempdir().expect("temporary output");
        let path = directory.path().join("proof.tiff");
        export_srgb(&fixture(), &path, ExportOptions::default()).expect("TIFF export");
        let mut decoder = TiffDecoder::new(BufReader::new(File::open(path).expect("open TIFF")))
            .expect("decode TIFF");
        assert_eq!(decoder.dimensions(), (2, 1));
        assert_eq!(decoder.color_type(), image::ColorType::Rgb16);
        assert_eq!(decoder.icc_profile().expect("ICC read"), Some(srgb_icc()));
    }

    #[test]
    fn tiff_zip_predictor_is_lossless_and_reduces_redundant_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("compressed.tif");
        let image = greign_pipeline::SrgbImage {
            width: 256,
            height: 256,
            pixels: (0..256 * 256)
                .map(|i| {
                    let v = f32::from(u8::try_from(i % 256).unwrap()) / 255.;
                    [v, 0.3, v * 0.7]
                })
                .collect(),
        };
        export_srgb(&image, &path, ExportOptions::default()).unwrap();
        let mut decoder =
            tiff::decoder::Decoder::new(BufReader::new(File::open(&path).unwrap())).unwrap();
        assert_eq!(
            decoder.get_tag_u32(tiff::tags::Tag::Compression).unwrap(),
            8
        );
        assert_eq!(decoder.get_tag_u32(tiff::tags::Tag::Predictor).unwrap(), 2);
        assert_eq!(
            decoder.get_tag_u8_vec(tiff::tags::Tag::IccProfile).unwrap(),
            srgb_icc()
        );
        let tiff::decoder::DecodingResult::U16(samples) = decoder.read_image().unwrap() else {
            panic!("Expected RGB16");
        };
        assert_eq!(samples, image.to_rgb16());
        assert!(std::fs::metadata(path).unwrap().len() < 256 * 256 * 6 / 4);
    }
}
