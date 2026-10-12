//! Rawler adapter for the backend-neutral decoder contract.

use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use rawler::analyze::FormatDump;
use rawler::decoders::{Decoder, FormatHint, RawDecodeParams};
use rawler::imgop::xyz::Illuminant;
use rawler::rawimage::{RawImage, RawImageData, RawPhotometricInterpretation};
use rawler::rawsource::RawSource;

use crate::decoder::{
    ActiveArea, DecodeError, DecoderCapabilities, EmbeddedPreview, Mosaic, Orientation,
    PreviewEncoding, RawDecoder, RawMetadata, preflight,
};

/// Primary pure-Rust decoder candidate.
#[derive(Clone, Copy, Debug, Default)]
pub struct RawlerDecoder;

impl RawDecoder for RawlerDecoder {
    fn name(&self) -> &'static str {
        "rawler-0.7"
    }

    fn capabilities(&self) -> DecoderCapabilities {
        DecoderCapabilities {
            metadata: true,
            embedded_preview: true,
            mosaic: true,
        }
    }

    fn metadata(&self, path: &Path) -> Result<RawMetadata, DecodeError> {
        preflight(path)?;
        with_rawler(path, |source, decoder| {
            // The dummy flag parses sensor descriptors but leaves pixel storage
            // uninitialized, so this path never decompresses the full mosaic.
            let raw = decoder
                .raw_image(source, &RawDecodeParams::default(), true)
                .map_err(classify_rawler)?;
            let general = decoder
                .raw_metadata(source, &RawDecodeParams::default())
                .map_err(classify_rawler)?;
            Ok(convert_metadata(
                self.name(),
                &raw,
                general.exif.orientation,
            ))
        })
    }

    fn embedded_preview(&self, path: &Path) -> Result<EmbeddedPreview, DecodeError> {
        preflight(path)?;
        with_rawler(path, |source, decoder| {
            match cr3_preview(source, decoder) {
                Ok(preview) => return Ok(preview),
                Err(DecodeError::PreviewUnavailable) => {}
                Err(error) => return Err(error),
            }
            let params = RawDecodeParams::default();
            let image = match decoder
                .preview_image(source, &params)
                .map_err(classify_rawler)?
            {
                Some(image) => image,
                None => match decoder
                    .thumbnail_image(source, &params)
                    .map_err(classify_rawler)?
                {
                    Some(image) => image,
                    None => decoder
                        .full_image(source, &params)
                        .map_err(classify_rawler)?
                        .ok_or(DecodeError::PreviewUnavailable)?,
                },
            };
            let rgb = image.to_rgb8();
            Ok(EmbeddedPreview {
                width: rgb.width(),
                height: rgb.height(),
                encoding: PreviewEncoding::Rgb8,
                bytes: rgb.into_raw(),
            })
        })
    }

    fn mosaic(&self, path: &Path) -> Result<Mosaic, DecodeError> {
        preflight(path)?;
        with_rawler(path, |source, decoder| {
            let raw = decoder
                .raw_image(source, &RawDecodeParams::default(), false)
                .map_err(classify_rawler)?;
            let general = decoder
                .raw_metadata(source, &RawDecodeParams::default())
                .map_err(classify_rawler)?;
            let metadata = convert_metadata(self.name(), &raw, general.exif.orientation);
            let samples = match raw.data {
                RawImageData::Integer(samples) => samples,
                RawImageData::Float(_) => return Err(DecodeError::FloatingPointMosaic),
            };
            let mosaic = Mosaic { metadata, samples };
            mosaic.validate()?;
            Ok(mosaic)
        })
    }
}

fn cr3_preview(source: &RawSource, decoder: &dyn Decoder) -> Result<EmbeddedPreview, DecodeError> {
    // CCTP type 2 identifies the preview in older CR3 files; current R7
    // firmware uses type 16 for its JPEG preview track.
    const CR3_PREVIEW_TYPES: [u64; 2] = [2, 16];
    if decoder.format_hint() != FormatHint::CR3 {
        return Err(DecodeError::PreviewUnavailable);
    }
    let FormatDump::Cr3(format) = decoder.format_dump() else {
        return Err(DecodeError::PreviewUnavailable);
    };
    let descriptor = format
        .filebox
        .moov
        .cr3desc
        .as_ref()
        .ok_or(DecodeError::PreviewUnavailable)?;
    let track_index = descriptor
        .cctp
        .ccdts
        .iter()
        .find(|record| CR3_PREVIEW_TYPES.contains(&record.image_type))
        .and_then(|record| record.trak_index.checked_sub(1))
        .and_then(|index| usize::try_from(index).ok())
        .ok_or(DecodeError::PreviewUnavailable)?;
    let track = format
        .filebox
        .moov
        .traks
        .get(track_index)
        .ok_or_else(|| DecodeError::Corrupt("CR3 preview track is out of range".into()))?;
    let (offset, size) = track
        .mdia
        .minf
        .stbl
        .get_sample_offset(1)
        .ok_or_else(|| DecodeError::Corrupt("CR3 preview has no sample offset".into()))?;
    let bytes = source
        .subview(
            u64::try_from(offset).map_err(|_| DecodeError::InvalidDimensions)?,
            u64::try_from(size).map_err(|_| DecodeError::InvalidDimensions)?,
        )
        .map_err(|error| DecodeError::Corrupt(error.to_string()))?;
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| DecodeError::Corrupt(format!("invalid CR3 preview: {error}")))?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|error| DecodeError::Corrupt(format!("invalid CR3 preview: {error}")))?;
    Ok(EmbeddedPreview {
        width,
        height,
        encoding: PreviewEncoding::Jpeg,
        bytes: bytes.to_vec(),
    })
}

fn with_rawler<T>(
    path: &Path,
    operation: impl FnOnce(&RawSource, &dyn Decoder) -> Result<T, DecodeError>,
) -> Result<T, DecodeError> {
    catch_unwind(AssertUnwindSafe(|| {
        let source = RawSource::new(path).map_err(|source| DecodeError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let decoder = rawler::get_decoder(&source).map_err(classify_rawler)?;
        operation(&source, decoder.as_ref())
    }))
    .map_err(|_| DecodeError::DecoderPanic)?
}

fn classify_rawler(error: rawler::RawlerError) -> DecodeError {
    match error {
        rawler::RawlerError::Unsupported { .. } => DecodeError::Unsupported(error.to_string()),
        rawler::RawlerError::DecoderFailed(message) => DecodeError::Corrupt(message),
    }
}

fn convert_metadata(
    backend: &'static str,
    raw: &RawImage,
    exif_orientation: Option<u16>,
) -> RawMetadata {
    let active_area = raw.active_area.map(|area| ActiveArea {
        x: u32::try_from(area.p.x).unwrap_or(u32::MAX),
        y: u32::try_from(area.p.y).unwrap_or(u32::MAX),
        width: u32::try_from(area.d.w).unwrap_or(u32::MAX),
        height: u32::try_from(area.d.h).unwrap_or(u32::MAX),
    });
    let crop_area = raw.crop_area.map(|area| ActiveArea {
        x: u32::try_from(area.p.x).unwrap_or(u32::MAX),
        y: u32::try_from(area.p.y).unwrap_or(u32::MAX),
        width: u32::try_from(area.d.w).unwrap_or(u32::MAX),
        height: u32::try_from(area.d.h).unwrap_or(u32::MAX),
    });
    let cfa_pattern = match &raw.photometric {
        RawPhotometricInterpretation::Cfa(config) => Some(config.cfa.name.clone()),
        RawPhotometricInterpretation::BlackIsZero | RawPhotometricInterpretation::LinearRaw => None,
    };
    let matrix = raw
        .color_matrix
        .get(&Illuminant::D65)
        .or_else(|| raw.color_matrix.values().next());
    let mut xyz_to_camera_d65 = [[0.0_f32; 3]; 3];
    if let Some(matrix) = matrix.filter(|matrix| matrix.len() >= 9) {
        for row in 0..3 {
            xyz_to_camera_d65[row].copy_from_slice(&matrix[row * 3..row * 3 + 3]);
        }
    }
    RawMetadata {
        backend,
        make: raw.make.clone(),
        model: raw.model.clone(),
        mode: raw.camera.mode.clone(),
        width: u32::try_from(raw.width).unwrap_or(u32::MAX),
        height: u32::try_from(raw.height).unwrap_or(u32::MAX),
        components_per_pixel: u8::try_from(raw.cpp).unwrap_or(u8::MAX),
        bits_per_component: u8::try_from(raw.bps).unwrap_or(u8::MAX),
        orientation: exif_orientation
            .map(rawler::Orientation::from_u16)
            .map_or_else(|| convert_orientation(raw.orientation), convert_orientation),
        active_area,
        crop_area,
        black_levels: raw.blacklevel.as_vec(),
        white_levels: raw.whitelevel.as_vec(),
        white_balance: [raw.wb_coeffs[0], raw.wb_coeffs[1], raw.wb_coeffs[2]],
        cfa_pattern,
        xyz_to_camera_d65,
    }
}

const fn convert_orientation(orientation: rawler::Orientation) -> Orientation {
    match orientation {
        rawler::Orientation::Normal => Orientation::Normal,
        rawler::Orientation::HorizontalFlip => Orientation::HorizontalFlip,
        rawler::Orientation::Rotate180 => Orientation::Rotate180,
        rawler::Orientation::VerticalFlip => Orientation::VerticalFlip,
        rawler::Orientation::Transpose => Orientation::Transpose,
        rawler::Orientation::Rotate90 => Orientation::Rotate90,
        rawler::Orientation::Transverse => Orientation::Transverse,
        rawler::Orientation::Rotate270 => Orientation::Rotate270,
        rawler::Orientation::Unknown => Orientation::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::RawlerDecoder;
    use crate::{DecodeError, RawDecoder};
    use std::io::Write;

    #[test]
    fn public_r7_uses_saturation_not_linearity_margin() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/downloads/canon-eos-r7-raw.cr3");
        if !path.exists() {
            eprintln!("skipping R7 saturation regression: public fixture is absent");
            return;
        }
        let metadata = RawlerDecoder.metadata(&path).expect("R7 metadata");
        assert_eq!(metadata.white_levels, vec![13_036.0]);
        let decoded = RawlerDecoder.mosaic(&path).expect("R7 decode");
        assert_eq!(decoded.metadata.white_levels, metadata.white_levels);
    }

    #[test]
    fn renamed_non_raw_input_is_rejected_by_content() {
        let mut file = tempfile::Builder::new()
            .suffix(".CR3")
            .tempfile()
            .expect("create fixture");
        file.write_all(b"this is not a raw file")
            .expect("write fixture");
        let error = RawlerDecoder
            .metadata(file.path())
            .expect_err("content must be rejected");
        assert!(matches!(
            error,
            DecodeError::Unsupported(_) | DecodeError::Corrupt(_) | DecodeError::DecoderPanic
        ));
    }

    #[test]
    fn short_container_signature_does_not_escape_as_panic() {
        let mut file = tempfile::NamedTempFile::new().expect("create fixture");
        file.write_all(b"II*\0\xff\xff\xff\xff")
            .expect("write truncated TIFF");
        assert!(RawlerDecoder.metadata(file.path()).is_err());
    }
}
