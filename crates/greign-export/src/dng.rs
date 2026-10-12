//! DNG 1.4 `LinearRaw`: demosaiced, edited linear Rec.2020 D65 positive.
use crate::ExportError;
use greign_pipeline::LinearRgbImage;
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};
use tiff::{
    encoder::{Rational, SRational, TiffEncoder, colortype},
    tags::Tag,
};

/// Write an uncompressed 32-bit floating-point DNG, not a mosaiced camera RAW.
/// Values above one are retained; negative working-RGB channels are floored at
/// zero because they cannot represent physical sensor responses. No sRGB OETF
/// or sRGB gamut clipping is applied. Edits are baked into the linear samples.
///
/// # Errors
/// Invalid dimensions, non-finite samples, I/O or TIFF encoding failures.
pub fn export_linear_dng(image: &LinearRgbImage, path: &Path) -> Result<(), ExportError> {
    export_linear_dng_dated(image, path, None)
}
/// Linear DNG with an optional original film capture date.
/// # Errors
/// Invalid image or TIFF/I/O error.
pub fn export_linear_dng_dated(
    image: &LinearRgbImage,
    path: &Path,
    date: Option<&crate::CaptureDate>,
) -> Result<(), ExportError> {
    let count = u64::from(image.width) * u64::from(image.height);
    if count == 0 || usize::try_from(count).ok() != Some(image.pixels.len()) {
        return Err(ExportError::InvalidDimensions);
    }
    if image.pixels.iter().flatten().any(|v| !v.is_finite()) {
        return Err(ExportError::InvalidSamples);
    }
    let mut file = BufWriter::new(File::create(path)?);
    {
        let mut encoder = TiffEncoder::new(&mut file)?;
        let exif = crate::capture_date::tiff_ifd(&mut encoder, date)?;
        let mut output = encoder.new_image::<colortype::RGB32Float>(image.width, image.height)?;
        let tags = output.encoder();
        if let (Some(offset), Some(date)) = (exif, date) {
            tags.write_tag(Tag::Unknown(34665), offset)?;
            tags.write_tag(Tag::DateTime, date.as_exif())?;
        }
        tags.write_tag(Tag::PhotometricInterpretation, 34892_u16)?; // LinearRaw, never CFA.
        tags.write_tag(Tag::NewSubfileType, 0_u32)?;
        tags.write_tag(Tag::Orientation, 1_u16)?;
        tags.write_tag(Tag::PlanarConfiguration, 1_u16)?;
        tags.write_tag(Tag::Make, "Greign")?;
        tags.write_tag(Tag::Model, "Linear Rec2020 D65 positive")?;
        tags.write_tag(Tag::Software, concat!("Greign ", env!("CARGO_PKG_VERSION")))?;
        tags.write_tag(Tag::ImageDescription,"Edited film-workspace positive; conversion, adjustments, crop and healing are baked in. Not original camera RAW. Linear Rec.2020 D65.")?;
        tags.write_tag(Tag::Unknown(50706), &[1_u8, 4, 0, 0][..])?; // DNGVersion
        tags.write_tag(Tag::Unknown(50707), &[1_u8, 4, 0, 0][..])?; // float requires 1.4
        tags.write_tag(Tag::Unknown(50708), "Greign Linear Rec2020 D65")?;
        tags.write_tag(Tag::Unknown(50713), &[1_u16, 1][..])?;
        tags.write_tag(
            Tag::Unknown(50714),
            &[const { Rational { n: 0, d: 1 } }; 3][..],
        )?;
        tags.write_tag(Tag::Unknown(50717), &[1_u32; 3][..])?;
        tags.write_tag(
            Tag::Unknown(50718),
            &[const { Rational { n: 1, d: 1 } }; 2][..],
        )?;
        tags.write_tag(Tag::Unknown(50719), &[0_u32, 0][..])?;
        tags.write_tag(Tag::Unknown(50720), &[image.width, image.height][..])?;
        // XYZ D65 -> Rec.2020. It describes the exported primaries, not the
        // source camera (reusing its matrix would apply its transform twice).
        let matrix = [
            1_716_651, -355_671, -253_366, -666_684, 1_616_481, 15_769, 17_640, -42_771, 942_103,
        ]
        .map(|n| SRational { n, d: 1_000_000 });
        tags.write_tag(Tag::Unknown(50721), matrix.as_slice())?;
        tags.write_tag(
            Tag::Unknown(50728),
            &[const { Rational { n: 1, d: 1 } }; 3][..],
        )?;
        tags.write_tag(Tag::Unknown(50730), SRational { n: 0, d: 1 })?;
        tags.write_tag(Tag::Unknown(50778), 21_u16)?; // D65
        tags.write_tag(Tag::Unknown(50936), "Greign linear Rec.2020")?;
        tags.write_tag(Tag::Unknown(50940), &[0_f32, 0., 1., 1.][..])?; // identity profile tone curve
        tags.write_tag(Tag::Unknown(51110), 1_u32)?; // DefaultBlackRender: none
        let pixels: Vec<_> = image.pixels.iter().flatten().map(|v| v.max(0.0)).collect();
        output.write_data(&pixels)?;
    }
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dng_tags_and_float_samples_preserve_highlight_headroom() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("positive.dng");
        let image = LinearRgbImage {
            width: 2,
            height: 1,
            pixels: vec![[0.125, 0.5, 2.5], [1., 0., -0.1]],
        };
        export_linear_dng(&image, &path).unwrap();
        // The generic TIFF decoder rejects LinearRaw by design. Inspect the
        // real DNG directory independently rather than relabeling it as RGB.
        let bytes = std::fs::read(path).unwrap();
        let little = &bytes[..2] == b"II";
        let short = |b: &[u8]| {
            if little {
                u16::from_le_bytes(b[..2].try_into().unwrap())
            } else {
                u16::from_be_bytes(b[..2].try_into().unwrap())
            }
        };
        let long = |b: &[u8]| {
            if little {
                u32::from_le_bytes(b[..4].try_into().unwrap())
            } else {
                u32::from_be_bytes(b[..4].try_into().unwrap())
            }
        };
        assert_eq!(short(&bytes[2..]), 42);
        let ifd = usize::try_from(long(&bytes[4..])).unwrap();
        let count = usize::from(short(&bytes[ifd..]));
        let mut tags = std::collections::BTreeMap::new();
        for i in 0..count {
            let entry = &bytes[ifd + 2 + i * 12..][..12];
            let kind = short(&entry[2..]);
            let unit = match kind {
                1 | 2 => 1,
                3 => 2,
                4 | 9 | 11 => 4,
                5 | 10 | 12 => 8,
                _ => panic!("Unknown field type"),
            };
            let size = usize::try_from(long(&entry[4..])).unwrap() * unit;
            let value = if size <= 4 {
                entry[8..8 + size].to_vec()
            } else {
                let offset = usize::try_from(long(&entry[8..])).unwrap();
                bytes[offset..offset + size].to_vec()
            };
            tags.insert(short(entry), (kind, value));
        }
        assert_eq!(long(&tags[&256].1), 2);
        assert_eq!(long(&tags[&257].1), 1);
        assert_eq!(short(&tags[&262].1), 34892);
        assert_eq!(tags[&50706].1, [1, 4, 0, 0]);
        assert_eq!(tags[&50707].1, [1, 4, 0, 0]);
        assert_eq!(
            tags[&258].1.chunks_exact(2).map(short).collect::<Vec<_>>(),
            [32, 32, 32]
        );
        assert_eq!(
            tags[&339].1.chunks_exact(2).map(short).collect::<Vec<_>>(),
            [3, 3, 3]
        );
        assert_eq!(
            tags[&50717].1.chunks_exact(4).map(long).collect::<Vec<_>>(),
            [1, 1, 1]
        );
        assert_eq!(tags[&50721].0, 10); // signed rational matrix
        assert_eq!(tags[&50721].1.len(), 72);
        assert!(!tags.contains_key(&33422)); // no fake Bayer pattern
        let offset = usize::try_from(long(&tags[&273].1)).unwrap();
        let samples: Vec<_> = bytes[offset..offset + 24]
            .chunks_exact(4)
            .map(|b| {
                if little {
                    f32::from_le_bytes(b.try_into().unwrap())
                } else {
                    f32::from_be_bytes(b.try_into().unwrap())
                }
            })
            .collect();
        assert_eq!(samples, vec![0.125, 0.5, 2.5, 1., 0., 0.]);
    }
}
