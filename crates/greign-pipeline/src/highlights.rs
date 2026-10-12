//! Color context from the boundaries of connected clipped regions.

use greign_raw::Mosaic;

use crate::{CfaColor, CfaPattern, NormalizedMosaic, PipelineError};

/// Context resolution in sensor pixels; reconstruction still runs at full resolution.
pub const HIGHLIGHT_TILE: u32 = 16;

/// Per-region opposed-plane chrominance, shared by CPU and GPU reconstruction.
#[derive(Clone, Debug)]
pub struct HighlightContext {
    /// Number of tiles in one row.
    pub width: u32,
    /// Row-major RGB residuals and a confidence flag, suitable for GPU storage.
    pub corrections: Vec<[f32; 4]>,
}

impl HighlightContext {
    /// Analyze white-balanced, cropped sensor values.
    ///
    /// # Errors
    /// Returns an error for invalid dimensions, thresholds, or non-Bayer input.
    pub fn from_normalized(
        mosaic: &NormalizedMosaic,
        thresholds: [f32; 3],
    ) -> Result<Self, PipelineError> {
        if mosaic.width == 0
            || mosaic.height == 0
            || mosaic.samples.len() != mosaic.width as usize * mosaic.height as usize
        {
            return Err(PipelineError::InvalidDimensions);
        }
        if mosaic.cfa.width() != 2 || mosaic.cfa.height() != 2 {
            return Err(PipelineError::UnsupportedCfa(
                "highlight context requires Bayer".into(),
            ));
        }
        if thresholds.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(PipelineError::NonFiniteParameter);
        }
        Ok(Self::build(
            mosaic.width,
            mosaic.height,
            &mosaic.cfa,
            thresholds,
            |x, y| mosaic.samples[mosaic.index(x, y)],
        ))
    }

    /// Analyze integer RAW without allocating a full normalized image on the GPU path.
    ///
    /// # Errors
    /// Returns an error for invalid sensor metadata or an unsupported CFA.
    pub fn from_raw(raw: &Mosaic, threshold: f32) -> Result<Self, PipelineError> {
        let meta = &raw.metadata;
        if meta.width == 0
            || meta.height == 0
            || meta.components_per_pixel != 1
            || raw.samples.len() != meta.width as usize * meta.height as usize
        {
            return Err(PipelineError::InvalidDimensions);
        }
        let cfa = CfaPattern::parse(meta.cfa_pattern.as_deref().unwrap_or(""))?;
        if cfa.width() != 2 || cfa.height() != 2 {
            return Err(PipelineError::UnsupportedCfa(
                "highlight context requires Bayer".into(),
            ));
        }
        let crop = meta
            .crop_area
            .or(meta.active_area)
            .unwrap_or(greign_raw::ActiveArea {
                x: 0,
                y: 0,
                width: meta.width,
                height: meta.height,
            });
        // Normalization owns the same level validation in the CPU path.
        if crop.width == 0
            || crop.height == 0
            || crop
                .x
                .checked_add(crop.width)
                .is_none_or(|v| v > meta.width)
            || crop
                .y
                .checked_add(crop.height)
                .is_none_or(|v| v > meta.height)
        {
            return Err(PipelineError::InvalidCrop);
        }
        if !threshold.is_finite()
            || threshold <= 0.0
            || !matches!(meta.black_levels.len(), 1 | 4)
            || !matches!(meta.white_levels.len(), 1 | 4)
            || meta
                .white_balance
                .iter()
                .any(|v| !v.is_finite() || *v <= 0.0)
        {
            return Err(PipelineError::InvalidLevels);
        }
        let level =
            |levels: &[f32], phase: usize| levels[if levels.len() == 1 { 0 } else { phase }];
        let mut black = [0.0; 4];
        let mut scale = [0.0; 4];
        for phase in 0..4 {
            black[phase] = level(&meta.black_levels, phase);
            let white = level(&meta.white_levels, phase);
            if !black[phase].is_finite() || !white.is_finite() || white <= black[phase] {
                return Err(PipelineError::InvalidLevels);
            }
            scale[phase] = white - black[phase];
        }
        Ok(Self::build(
            crop.width,
            crop.height,
            &cfa.shifted(crop.x, crop.y),
            meta.white_balance.map(|v| v * threshold),
            |x, y| {
                let x = x + crop.x;
                let y = y + crop.y;
                let phase = ((y % 2) * 2 + x % 2) as usize;
                let channel = channel(cfa.color_at(x, y));
                (f32::from(raw.samples[(y * meta.width + x) as usize]) - black[phase])
                    / scale[phase]
                    * meta.white_balance[channel]
            },
        ))
    }

    /// Residual at a cropped sensor coordinate.
    #[must_use]
    pub fn at(&self, x: u32, y: u32) -> [f32; 3] {
        let value =
            self.corrections[((y / HIGHLIGHT_TILE) * self.width + x / HIGHLIGHT_TILE) as usize];
        [value[0], value[1], value[2]]
    }

    #[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
    fn build(
        width: u32,
        height: u32,
        cfa: &CfaPattern,
        thresholds: [f32; 3],
        sample: impl Fn(u32, u32) -> f32,
    ) -> Self {
        let tw = width.div_ceil(HIGHLIGHT_TILE) as usize;
        let th = height.div_ceil(HIGHLIGHT_TILE) as usize;
        let mut clipped = vec![false; tw * th];
        let mut sums = vec![[0.0_f64; 3]; tw * th];
        let mut counts = vec![0_u32; tw * th];
        // Same 2x2 superpixel for all channels prevents estimating color from
        // differently clipped populations. Only reliable boundary color is used.
        for y in (0..height.saturating_sub(1)).step_by(2) {
            for x in (0..width.saturating_sub(1)).step_by(2) {
                let tile = (y / HIGHLIGHT_TILE) as usize * tw + (x / HIGHLIGHT_TILE) as usize;
                let mut rgb = [0.0_f32; 3];
                let mut valid = true;
                for dy in 0..2 {
                    for dx in 0..2 {
                        let c = channel(cfa.color_at(x + dx, y + dy));
                        let v = sample(x + dx, y + dy);
                        clipped[tile] |= v >= thresholds[c];
                        valid &= v > thresholds[c] * 0.2 && v < thresholds[c] * 0.98;
                        rgb[c] += v * if c == 1 { 0.5 } else { 1.0 };
                    }
                }
                if valid {
                    for c in 0..3 {
                        sums[tile][c] += f64::from(rgb[c]);
                    }
                    counts[tile] += 1;
                }
            }
        }
        let neighbors = |i: usize| {
            let x = i % tw;
            let y = i / tw;
            (y.saturating_sub(1)..=(y + 1).min(th - 1)).flat_map(move |ny| {
                (x.saturating_sub(1)..=(x + 1).min(tw - 1)).map(move |nx| ny * tw + nx)
            })
        };
        let mut labels = vec![0_usize; tw * th];
        let mut regions = 0;
        let mut queue = Vec::new();
        for i in 0..labels.len() {
            if !clipped[i] || labels[i] != 0 {
                continue;
            }
            regions += 1;
            labels[i] = regions;
            queue.clear();
            queue.push(i);
            let mut head = 0;
            while head < queue.len() {
                let cell = queue[head];
                head += 1;
                for n in neighbors(cell) {
                    if clipped[n] && labels[n] == 0 {
                        labels[n] = regions;
                        queue.push(n);
                    }
                }
            }
        }
        let mut region_sums = vec![[0.0; 3]; regions + 1];
        let mut region_counts = vec![0_u32; regions + 1];
        for i in 0..labels.len() {
            if counts[i] == 0 {
                continue;
            }
            let mut seen = [0_usize; 9];
            let mut nseen = 0;
            for n in neighbors(i) {
                let label = labels[n];
                if label == 0 || seen[..nseen].contains(&label) {
                    continue;
                }
                seen[nseen] = label;
                nseen += 1;
                for c in 0..3 {
                    region_sums[label][c] += sums[i][c];
                }
                region_counts[label] += counts[i];
            }
        }
        let mut residuals = vec![[0.0_f32; 4]; regions + 1];
        for i in 1..=regions {
            if region_counts[i] < 3 {
                continue;
            }
            let mean = region_sums[i].map(|v| (v / f64::from(region_counts[i])) as f32);
            let roots = mean.map(f32::cbrt);
            for c in 0..3 {
                residuals[i][c] =
                    mean[c] - ((roots[(c + 1) % 3] + roots[(c + 2) % 3]) * 0.5).powi(3);
            }
            residuals[i][3] = 1.0;
        }
        Self {
            width: tw as u32,
            corrections: labels.iter().map(|i| residuals[*i]).collect(),
        }
    }
}

const fn channel(color: CfaColor) -> usize {
    match color {
        CfaColor::Red => 0,
        CfaColor::Green => 1,
        CfaColor::Blue => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reconstruct_clipped_samples;

    #[test]
    fn large_highlight_uses_distant_warm_boundary_and_preserves_unclipped_samples() {
        for pattern in ["RGGB", "BGGR", "GRBG", "GBRG"] {
            let cfa = CfaPattern::parse(pattern).unwrap();
            let mut mosaic = NormalizedMosaic {
                width: 160,
                height: 128,
                cfa,
                samples: vec![0.0; 160 * 128],
            };
            for y in 0..128 {
                for x in 0..160 {
                    let c = channel(mosaic.cfa.color_at(x, y));
                    let rgb = if (32..128).contains(&x) && (32..96).contains(&y) {
                        [1.8, 1.0, 1.3]
                    } else {
                        [1.5, 0.85, 0.65]
                    };
                    let i = mosaic.index(x, y);
                    mosaic.samples[i] = rgb[c];
                }
            }
            let before = mosaic.samples.clone();
            let thresholds = [1.79, 0.99, 1.29];
            let context = HighlightContext::from_normalized(&mosaic, thresholds).unwrap();
            assert!(context.at(80, 64)[0] > 0.5);
            assert!(context.at(80, 64)[2] < -0.4);
            reconstruct_clipped_samples(&mut mosaic, thresholds).unwrap();
            for y in 0..128 {
                for x in 0..160 {
                    let i = mosaic.index(x, y);
                    let c = channel(mosaic.cfa.color_at(x, y));
                    if before[i] < thresholds[c] {
                        assert_eq!(before[i].to_bits(), mosaic.samples[i].to_bits());
                    }
                }
            }
        }
    }

    #[test]
    fn distinct_highlights_do_not_share_color_context() {
        let cfa = CfaPattern::parse("RGGB").unwrap();
        let mut mosaic = NormalizedMosaic {
            width: 256,
            height: 128,
            cfa,
            samples: vec![0.0; 256 * 128],
        };
        for y in 0..128 {
            for x in 0..256 {
                let c = channel(mosaic.cfa.color_at(x, y));
                let clipped =
                    (32..96).contains(&y) && ((32..96).contains(&x) || (160..224).contains(&x));
                let rgb = if clipped {
                    [2.0; 3]
                } else if x < 128 {
                    [0.8, 0.5, 0.3]
                } else {
                    [0.3, 0.5, 0.8]
                };
                let i = mosaic.index(x, y);
                mosaic.samples[i] = rgb[c];
            }
        }
        let context = HighlightContext::from_normalized(&mosaic, [1.0; 3]).unwrap();
        assert!(context.at(64, 64)[0] > 0.3);
        assert!(context.at(192, 64)[0] < -0.3);
    }
}
