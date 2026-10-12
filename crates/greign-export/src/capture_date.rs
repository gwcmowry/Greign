//! Original film capture date, independent of scan/export filesystem dates.
use std::io::{Seek, Write};
use tiff::{encoder::TiffEncoder, tags::Tag};

/// Validated local capture timestamp; date-only input uses noon, without a zone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureDate(String);
impl CaptureDate {
    /// Parse YYYY-MM-DD or YYYY-MM-DD HH:MM:SS. Empty means unspecified.
    /// # Errors
    /// Invalid format, calendar date or time.
    pub fn parse(input: &str) -> Result<Option<Self>, String> {
        let s = input.trim();
        if s.is_empty() {
            return Ok(None);
        }
        let error = "Use YYYY-MM-DD or YYYY-MM-DD HH:MM:SS with a valid date/time";
        if !s.is_ascii() || !matches!(s.len(), 10 | 19) {
            return Err(error.into());
        }
        let b = s.as_bytes();
        if b[4] != b'-'
            || b[7] != b'-'
            || !b[..10]
                .iter()
                .enumerate()
                .all(|(i, c)| matches!(i, 4 | 7) || c.is_ascii_digit())
        {
            return Err(error.into());
        }
        let year = s[..4].parse::<u32>().map_err(|_| error)?;
        let month = s[5..7].parse::<u32>().map_err(|_| error)?;
        let day = s[8..10].parse::<u32>().map_err(|_| error)?;
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if leap {
                    29
                } else {
                    28
                }
            }
            _ => 0,
        };
        if year == 0 || day == 0 || day > days {
            return Err(error.into());
        }
        if s.len() == 19 {
            if b[10] != b' '
                || b[13] != b':'
                || b[16] != b':'
                || !b[11..]
                    .iter()
                    .enumerate()
                    .all(|(i, c)| matches!(i, 2 | 5) || c.is_ascii_digit())
            {
                return Err(error.into());
            }
            if s[11..13].parse::<u32>().map_err(|_| error)? > 23
                || s[14..16].parse::<u32>().map_err(|_| error)? > 59
                || s[17..19].parse::<u32>().map_err(|_| error)? > 59
            {
                return Err(error.into());
            }
        }
        Ok(Some(Self(format!(
            "{} {}",
            s[..10].replace('-', ":"),
            if s.len() == 19 { &s[11..] } else { "12:00:00" }
        ))))
    }
    /// Standard EXIF ASCII timestamp, with no inferred time zone.
    #[must_use]
    pub fn as_exif(&self) -> &str {
        &self.0
    }
    pub(crate) fn jpeg_payload(&self) -> Vec<u8> {
        let mut bytes = b"Exif\0\0II\x2a\0\x08\0\0\0".to_vec();
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        entry(&mut bytes, 306, 2, 20, 68);
        entry(&mut bytes, 34665, 4, 1, 38);
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        entry(&mut bytes, 36864, 7, 4, u32::from_le_bytes(*b"0232"));
        entry(&mut bytes, 36867, 2, 20, 68);
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(self.0.as_bytes());
        bytes.push(0);
        bytes
    }
}
fn entry(bytes: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: u32) {
    bytes.extend_from_slice(&tag.to_le_bytes());
    bytes.extend_from_slice(&kind.to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
}
pub(crate) fn tiff_ifd<W: Write + Seek>(
    encoder: &mut TiffEncoder<W>,
    date: Option<&CaptureDate>,
) -> tiff::TiffResult<Option<u32>> {
    if let Some(date) = date {
        let mut directory = encoder.extra_directory()?;
        directory.write_tag(Tag::Unknown(36867), date.as_exif())?;
        Ok(Some(directory.finish_with_offsets()?.offset))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates_are_validated_and_date_only_uses_noon() {
        assert_eq!(
            CaptureDate::parse("1998-07-03").unwrap().unwrap().as_exif(),
            "1998:07:03 12:00:00"
        );
        assert!(CaptureDate::parse("2000-02-29 23:59:59").is_ok());
        for bad in [
            "1900-02-29",
            "2025-02-29",
            "2024-13-01",
            "0000-01-01",
            "2024-01-01 24:00:00",
            "June 1998",
            "2024-01-01T12:00:00",
        ] {
            assert!(CaptureDate::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(CaptureDate::parse("").unwrap(), None);
    }
}
