//! GPU bilinear demosaic parameter contract.

/// Supported 2×2 Bayer layouts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BayerPattern {
    /// Red, green / green, blue.
    Rggb,
    /// Blue, green / green, red.
    Bggr,
    /// Green, red / blue, green.
    Grbg,
    /// Green, blue / red, green.
    Gbrg,
}

impl BayerPattern {
    pub(crate) const fn codes(self) -> [u32; 4] {
        match self {
            Self::Rggb => [0, 1, 1, 2],
            Self::Bggr => [2, 1, 1, 0],
            Self::Grbg => [1, 0, 2, 1],
            Self::Gbrg => [1, 2, 0, 1],
        }
    }
}

/// Exact host layout expected by `bilinear_demosaic.wgsl`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct DemosaicParams {
    /// Sensor width.
    pub width: u32,
    /// Sensor height.
    pub height: u32,
    /// Uniform alignment padding.
    pub padding: [u32; 2],
    /// Row-major CFA color codes: 0=R, 1=G, 2=B.
    pub cfa: [u32; 4],
}

impl DemosaicParams {
    /// Creates parameters for a non-empty Bayer image.
    #[must_use]
    pub const fn new(width: u32, height: u32, pattern: BayerPattern) -> Option<Self> {
        if width == 0 || height == 0 {
            None
        } else {
            Some(Self {
                width,
                height,
                padding: [0; 2],
                cfa: pattern.codes(),
            })
        }
    }
}

const _: () = assert!(size_of::<DemosaicParams>() == 32);

#[cfg(test)]
mod tests {
    use super::{BayerPattern, DemosaicParams};

    #[test]
    fn bayer_rotations_have_expected_codes() {
        assert_eq!(BayerPattern::Rggb.codes(), [0, 1, 1, 2]);
        assert_eq!(BayerPattern::Bggr.codes(), [2, 1, 1, 0]);
        assert_eq!(BayerPattern::Grbg.codes(), [1, 0, 2, 1]);
        assert_eq!(BayerPattern::Gbrg.codes(), [1, 2, 0, 1]);
    }

    #[test]
    fn uniform_layout_is_two_vec4_slots() {
        assert_eq!(size_of::<DemosaicParams>(), 32);
    }
}
