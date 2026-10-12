//! Calibrated linear scan analysis, density-domain conversion, and manual retouch.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use anyhow::{Result, bail, ensure};
use greign_pipeline::{self as p, LinearRgbImage, SrgbImage};
use greign_raw::{RawDecoder, RawlerDecoder};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;
mod adjustments;
pub use adjustments::{COLOR_NAMES, ColorBand};

/// Material captured by a roll.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Material {
    /// Chromogenic color negative.
    #[default]
    ColorNegative,
    /// Monochrome negative.
    BlackAndWhite,
    /// Already-positive photographs or slides.
    Positive,
}

/// Linear, untonemapped scan. RAW values stay in camera RGB until inversion.
#[derive(Clone, Debug)]
pub struct Scan {
    /// Oriented image, before aesthetic crop.
    pub image: LinearRgbImage,
    /// Camera transform, absent for sRGB raster inputs.
    pub camera_matrix: Option<[[f32; 3]; 3]>,
    /// As-shot balance, used only for positive RAWs.
    pub white_balance: [f32; 3],
    /// Human-readable capture identity for incompatible-roll checks.
    pub capture: String,
}

/// Normalized image rectangle; independent analysis and export crops use this type.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    /// Left.
    pub x: f32,
    /// Top.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}
impl Default for Rect {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        }
    }
}
impl Rect {
    /// Validate normalized bounds.
    #[must_use]
    pub fn valid(self) -> bool {
        [self.x, self.y, self.w, self.h]
            .iter()
            .all(|v| v.is_finite())
            && self.x >= 0.0
            && self.y >= 0.0
            && self.w > 0.0
            && self.h > 0.0
            && self.x + self.w <= 1.000_001
            && self.y + self.h <= 1.000_001
    }
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x <= self.x + self.w && y <= self.y + self.h
    }
}

/// Additive creative adjustments. Frame values are offsets from roll values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Adjustments {
    /// Exposure in stops.
    pub brightness: f32,
    /// Contrast, -50 through 50.
    pub contrast: f32,
    /// Red/blue balance, -50 through 50.
    pub warmth: f32,
    /// Green/magenta balance, -50 through 50.
    pub tint: f32,
    /// Saturation offset, -100 through 100.
    pub saturation: f32,
    /// Broad highlight exposure offset, -100 through 100.
    pub highlights: f32,
    /// Broad shadow exposure offset, -100 through 100.
    pub shadows: f32,
    /// Smooth, overlapping red/orange/yellow/green/aqua/blue/purple/magenta bands.
    pub colors: [ColorBand; 8],
}
impl Adjustments {
    /// Combine a roll adjustment with an independent frame correction.
    #[must_use]
    pub fn combined(self, other: Self) -> Self {
        Self {
            brightness: self.brightness + other.brightness,
            contrast: self.contrast + other.contrast,
            warmth: self.warmth + other.warmth,
            tint: self.tint + other.tint,
            saturation: self.saturation + other.saturation,
            highlights: self.highlights + other.highlights,
            shadows: self.shadows + other.shadows,
            colors: std::array::from_fn(|i| self.colors[i].combined(other.colors[i])),
        }
    }

    /// Difference used to apply a relative adjustment to a selection.
    #[must_use]
    pub fn difference(self, previous: Self) -> Self {
        self.combined(Self {
            brightness: -previous.brightness,
            contrast: -previous.contrast,
            warmth: -previous.warmth,
            tint: -previous.tint,
            saturation: -previous.saturation,
            highlights: -previous.highlights,
            shadows: -previous.shadows,
            colors: previous.colors.map(|b| ColorBand {
                hue: -b.hue,
                saturation: -b.saturation,
                lightness: -b.lightness,
            }),
        })
    }
}

/// Reproducible, shared roll calibration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Analysis {
    /// Implementation version.
    pub version: u32,
    /// Linear film base per capture channel.
    pub base: [f32; 3],
    /// Shared minimum density per channel.
    pub floor: [f32; 3],
    /// Shared scalar density range, avoiding independent per-frame color stretches.
    pub range: f32,
    /// Base measurement method.
    pub method: String,
    /// Actionable limitations.
    pub warnings: Vec<String>,
    /// Number of frames used.
    pub frames: usize,
}

/// Explicit manual repair in uncropped source coordinates.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Heal {
    /// Target center.
    pub target: [f32; 2],
    /// Source center, movable by the user.
    pub source: [f32; 2],
    /// Radius as a fraction of the shorter source dimension.
    pub radius: f32,
}

/// Load RAW or an sRGB raster without photographic tone mapping.
///
/// # Errors
/// Unsupported inputs (including X-Trans), missing paths, or invalid sensor data.
pub fn load_scan(path: &Path, max_dimension: Option<u32>, demo: bool) -> Result<Scan> {
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let raster = matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "tif" | "tiff");
    if raster {
        let mut image = image::ImageReader::open(path)?.decode()?;
        if let Some(size) = max_dimension {
            image = image.thumbnail(size, size);
        }
        let rgb = image.to_rgb32f();
        let mut pixels: Vec<_> = rgb.pixels().map(|p| p.0.map(srgb_to_linear)).collect();
        if demo {
            // Inverse of the baseline paper model, solely for an interactive demo.
            let base = [0.82, 0.52, 0.30];
            for pixel in &mut pixels {
                for c in 0..3 {
                    let u = -(1.0 - pixel[c].clamp(0.0, 0.98).powf(1.0 / 2.6) * 0.9)
                        .max(0.001)
                        .log10();
                    pixel[c] = base[c] * 10.0_f32.powf(-u * 2.0);
                }
            }
        }
        return Ok(Scan {
            image: LinearRgbImage {
                width: rgb.width(),
                height: rgb.height(),
                pixels,
            },
            camera_matrix: None,
            white_balance: [1.0; 3],
            capture: if demo {
                "Simulated negative"
            } else {
                "sRGB raster"
            }
            .into(),
        });
    }
    let raw = RawlerDecoder.mosaic(path)?;
    let mut sensor = p::normalize_sensor(&raw)?;
    if let Some(a) = raw.metadata.crop_area.or(raw.metadata.active_area) {
        sensor = p::active_crop(&sensor, a.x, a.y, a.width, a.height)?;
    }
    if let Some(size) = max_dimension {
        sensor = p::downsample_bayer(&sensor, size)?;
    }
    // No highlight invention, as-shot WB, display shoulder, or pre-inversion clipping.
    let rgb = p::ppg_demosaic(&sensor)?;
    let oriented = SrgbImage {
        width: rgb.width,
        height: rgb.height,
        pixels: rgb.pixels,
    }
    .oriented(raw.metadata.orientation);
    Ok(Scan {
        image: LinearRgbImage {
            width: oriented.width,
            height: oriented.height,
            pixels: oriented.pixels,
        },
        camera_matrix: Some(raw.metadata.xyz_to_camera_d65),
        white_balance: raw.metadata.white_balance,
        capture: format!("{} {}", raw.metadata.make, raw.metadata.model),
    })
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn quantile(mut values: Vec<f32>, q: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let index = ((values.len() - 1) as f32 * q).round() as usize;
    *values.select_nth_unstable_by(index, f32::total_cmp).1
}
fn channels(values: &[[f32; 3]], q: f32) -> [f32; 3] {
    std::array::from_fn(|c| quantile(values.iter().map(|p| p[c]).collect(), q))
}
fn samples(scan: &Scan, region: Rect) -> Vec<[f32; 3]> {
    let step = (scan.image.pixels.len() / 60_000).max(1);
    scan.image
        .pixels
        .iter()
        .enumerate()
        .step_by(step)
        .filter_map(|(i, p)| {
            let x = (i % scan.image.width as usize) as f32 / scan.image.width as f32;
            let y = (i / scan.image.width as usize) as f32 / scan.image.height as f32;
            (region.contains(x, y)
                && p.iter()
                    .all(|v| v.is_finite() && *v > 0.000_001 && *v < 0.995))
            .then_some(*p)
        })
        .collect()
}

/// Measure a user-selected clean patch. Reject clipping or strongly uneven patches.
///
/// # Errors
/// Invalid bounds, too few valid samples, or an unsuitable film-base patch.
pub fn measure_base(scan: &Scan, region: Rect) -> Result<[f32; 3]> {
    ensure!(region.valid(), "Film-base selection is outside the image");
    let v = samples(scan, region);
    let total = scan
        .image
        .pixels
        .iter()
        .enumerate()
        .step_by((scan.image.pixels.len() / 60_000).max(1))
        .filter(|(i, _)| {
            region.contains(
                (*i % scan.image.width as usize) as f32 / scan.image.width as f32,
                (*i / scan.image.width as usize) as f32 / scan.image.height as f32,
            )
        })
        .count();
    ensure!(
        v.len() as f32 >= total as f32 * 0.9,
        "That patch contains clipped or invalid samples; select clean, unclipped film base"
    );
    ensure!(v.len() >= 8, "Select a larger, unclipped film-base patch");
    let low = channels(&v, 0.1);
    let high = channels(&v, 0.9);
    let median = channels(&v, 0.5);
    ensure!(
        (0..3).all(|c| (high[c] - low[c]) / median[c] < 0.25),
        "That patch is uneven; choose clean, unexposed film"
    );
    Ok(median)
}

/// Estimate one calibration across a consistent capture roll.
///
/// # Errors
/// Empty/incompatible rolls or insufficient usable, unclipped scan data.
pub fn analyze(scans: &[&Scan], explicit_base: Option<[f32; 3]>, region: Rect) -> Result<Analysis> {
    ensure!(!scans.is_empty(), "Import some frames first");
    ensure!(region.valid(), "Invalid analysis area");
    ensure!(
        scans.iter().all(|s| s.capture == scans[0].capture),
        "Separate different cameras or input types into different rolls"
    );
    let mut candidates = Vec::new();
    let mut measured = Vec::new();
    let mut warnings = Vec::new();
    for scan in scans {
        let all = samples(scan, Rect::default());
        ensure!(all.len() > 32, "Not enough unclipped scan data");
        let candidate = channels(&all, 0.99);
        candidates.push(candidate);
        let border: Vec<_> = scan
            .image
            .pixels
            .iter()
            .enumerate()
            .step_by((scan.image.pixels.len() / 60_000).max(1))
            .filter_map(|(i, p)| {
                let x = (i % scan.image.width as usize) as f32 / scan.image.width as f32;
                let y = (i / scan.image.width as usize) as f32 / scan.image.height as f32;
                ((x < 0.04 || y < 0.04 || x > 0.96 || y > 0.96)
                    && p.iter().all(|v| *v > 0.000_001 && *v < 0.995))
                .then_some(*p)
            })
            .collect();
        if border.len() > 32 {
            let lo = channels(&border, 0.1);
            let hi = channels(&border, 0.9);
            let mid = channels(&border, 0.5);
            if (0..3).all(|c| (hi[c] - lo[c]) / mid[c] < 0.12 && mid[c] > candidate[c] * 0.85) {
                measured.push(mid);
            }
        }
        let bad = scan
            .image
            .pixels
            .iter()
            .filter(|p| p.iter().any(|v| !v.is_finite() || *v >= 0.995))
            .count();
        if bad as f32 / scan.image.pixels.len() as f32 > 0.005 {
            warnings.push("Some capture channels are clipped; those samples were excluded.".into());
        }
    }
    let (base, method) = if let Some(base) = explicit_base {
        ensure!(
            base.iter().all(|v| v.is_finite() && *v > 0.0 && *v < 0.995),
            "Invalid or clipped film base"
        );
        (base, "Selected film-base patch")
    } else if !measured.is_empty() {
        (channels(&measured, 0.5), "Measured uniform film border")
    } else {
        warnings.push("No reliable film border found. Select a clean film-base patch to improve the estimate.".into());
        (
            channels(&candidates, 0.8),
            if scans.len() >= 3 {
                "Inferred from roll · provisional"
            } else {
                "Single-frame estimate · provisional"
            },
        )
    };
    let mut density = Vec::new();
    for scan in scans {
        for value in samples(scan, region) {
            density.push(std::array::from_fn(|c| {
                (base[c] / value[c]).log10().max(0.0)
            }));
        }
    }
    ensure!(
        density.len() > 32,
        "The analysis area has too few valid samples"
    );
    let floor = channels(&density, 0.005);
    let top = channels(&density, 0.995);
    let range = ((top[0] - floor[0] + top[1] - floor[1] + top[2] - floor[2]) / 3.0).max(0.15);
    warnings.sort();
    warnings.dedup();
    Ok(Analysis {
        version: 1,
        base,
        floor,
        range,
        method: method.into(),
        warnings,
        frames: scans.len(),
    })
}

/// Render a positive with additive roll/frame edits and explicitly placed repairs.
///
/// # Errors
/// Missing negative calibration, invalid crop/repair, or color-transform errors.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn render(
    scan: &Scan,
    material: Material,
    analysis: Option<&Analysis>,
    edits: Adjustments,
    crop: Rect,
    rotation: u8,
    heals: &[Heal],
    show_negative: bool,
) -> Result<SrgbImage> {
    Ok(p::rec2020_to_srgb(render_linear(
        scan,
        material,
        analysis,
        edits,
        crop,
        rotation,
        heals,
        show_negative,
    )?))
}

/// Render the edited positive in linear-light Rec.2020, before sRGB conversion
/// and output clipping. Conversion, creative tone, crop and healing are baked in.
///
/// # Errors
/// Same validation and color-transform errors as [`render`].
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn render_linear(
    scan: &Scan,
    material: Material,
    analysis: Option<&Analysis>,
    edits: Adjustments,
    crop: Rect,
    rotation: u8,
    heals: &[Heal],
    show_negative: bool,
) -> Result<LinearRgbImage> {
    let prepared = prepare(scan, material, analysis, show_negative, &|| false)?;
    finish(&prepared, edits, crop, rotation, heals, &|| false)
}

/// Cached conversion before creative edits; independent of crop and healing.
pub struct Prepared {
    /// Linear Rec.2020 converted pixels.
    pub image: LinearRgbImage,
    material: Material,
    negative: bool,
}

/// Convert once, then reuse for lighting/color changes.
/// # Errors
/// Invalid analysis/matrix, or a superseded preview request.
pub fn prepare(
    scan: &Scan,
    material: Material,
    analysis: Option<&Analysis>,
    show_negative: bool,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Prepared> {
    ensure!(!cancelled(), "Preview superseded");
    let mut rgb = scan.image.clone();
    if show_negative {
        let high = rgb
            .pixels
            .iter()
            .flat_map(|p| p.iter())
            .copied()
            .filter(|v| v.is_finite())
            .fold(1.0_f32, f32::max);
        for p in &mut rgb.pixels {
            for c in p {
                *c = (*c / high).clamp(0.0, 1.0);
            }
        }
    } else if material != Material::Positive {
        let a = analysis.ok_or_else(|| anyhow::anyhow!("Analyze & convert this roll first"))?;
        rgb.pixels
            .par_chunks_mut(8192)
            .try_for_each(|chunk| -> Result<()> {
                ensure!(!cancelled(), "Preview superseded");
                for pixel in chunk {
                    for (c, v) in pixel.iter_mut().enumerate() {
                        let density =
                            ((a.base[c] / v.max(0.000_001)).log10() - a.floor[c]) / a.range;
                        let paper = ((1.0 - 10.0_f32.powf(-density.max(0.0))) / 0.9)
                            .max(0.0)
                            .powf(2.6);
                        *v = if paper > 0.92 {
                            0.92 + 0.08 * (1.0 - (-(paper - 0.92) / 0.08).exp())
                        } else {
                            paper
                        };
                    }
                }
                Ok(())
            })?;
    } else if scan.camera_matrix.is_some() {
        for pixel in &mut rgb.pixels {
            for (c, v) in pixel.iter_mut().enumerate() {
                *v *= scan.white_balance[c];
            }
        }
    }
    if let Some(matrix) = scan.camera_matrix {
        p::camera_to_rec2020(&mut rgb, matrix)?;
    } else {
        for v in &mut rgb.pixels {
            let [r, g, b] = *v;
            *v = [
                0.627_404 * r + 0.329_283 * g + 0.043_313 * b,
                0.069_097 * r + 0.919_54 * g + 0.011_362 * b,
                0.016_391 * r + 0.088_013 * g + 0.895_595 * b,
            ];
        }
    }
    ensure!(!cancelled(), "Preview superseded");
    Ok(Prepared {
        image: rgb,
        material,
        negative: show_negative,
    })
}

/// Apply creative edits to a cached conversion, with cooperative cancellation.
/// # Errors
/// Invalid crop/heal or a superseded preview request.
pub fn finish(
    prepared: &Prepared,
    edits: Adjustments,
    crop: Rect,
    rotation: u8,
    heals: &[Heal],
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<LinearRgbImage> {
    ensure!(crop.valid(), "Invalid crop");
    ensure!(!cancelled(), "Preview superseded");
    let mut rgb = prepared.image.clone();
    let material = prepared.material;
    let show_negative = prepared.negative;
    if !show_negative {
        let mixer_active = edits.colors.iter().any(|b| *b != ColorBand::default());
        let warmth = (edits.warmth * 0.008).exp2();
        let cool = (-edits.warmth * 0.008).exp2();
        let tint = (-edits.tint * 0.008).exp2();
        let exposure = edits.brightness.clamp(-6.0, 6.0).exp2();
        rgb.pixels
            .par_chunks_mut(8192)
            .try_for_each(|chunk| -> Result<()> {
                ensure!(!cancelled(), "Preview superseded");
                for v in chunk {
                    v[0] *= warmth;
                    v[2] *= cool;
                    v[1] *= tint;
                    let y = p::luminance(*v).max(0.000_001);
                    let contrast = (y / 0.18).powf((edits.contrast / 100.0).clamp(-0.9, 0.9));
                    let saturation = (1.0 + edits.saturation / 100.0).max(0.0);
                    for c in v.iter_mut() {
                        *c = (y + (*c - y) * saturation) * contrast * exposure;
                    }
                    adjustments::lighting(v, edits.shadows, edits.highlights);
                    if mixer_active && material != Material::BlackAndWhite {
                        adjustments::mix_color(v, &edits.colors);
                    }
                    if material == Material::BlackAndWhite {
                        *v = [p::luminance(*v); 3];
                    }
                }
                Ok(())
            })?;
        if material == Material::Positive {
            p::highlight_tone_curve(&mut rgb);
        }
        for heal in heals {
            ensure!(!cancelled(), "Preview superseded");
            apply_heal(&mut rgb, *heal)?;
        }
    }
    let x = (crop.x * rgb.width as f32) as u32;
    let y = (crop.y * rgb.height as f32) as u32;
    let w = ((crop.w * rgb.width as f32) as u32)
        .max(1)
        .min(rgb.width - x);
    let h = ((crop.h * rgb.height as f32) as u32)
        .max(1)
        .min(rgb.height - y);
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for row in y..y + h {
        ensure!(!cancelled(), "Preview superseded");
        pixels.extend_from_slice(
            &rgb.pixels[(row * rgb.width + x) as usize..(row * rgb.width + x + w) as usize],
        );
    }
    // Orientation only permutes pixels; this container performs no transfer curve.
    let output = SrgbImage {
        width: w,
        height: h,
        pixels,
    }
    .oriented(match rotation % 4 {
        1 => greign_raw::Orientation::Rotate90,
        2 => greign_raw::Orientation::Rotate180,
        3 => greign_raw::Orientation::Rotate270,
        _ => greign_raw::Orientation::Normal,
    });
    Ok(LinearRgbImage {
        width: output.width,
        height: output.height,
        pixels: output.pixels,
    })
}

/// Select a nearby source for a user-marked spot. Never detects or places targets.
#[must_use]
pub fn suggest_source(target: [f32; 2], radius: f32, aspect: f32) -> [f32; 2] {
    let dx = radius * 3.0 / aspect.max(1.0);
    [
        (if target[0] + dx < 0.98 {
            target[0] + dx
        } else {
            target[0] - dx
        })
        .clamp(0.01, 0.99),
        target[1].clamp(0.01, 0.99),
    ]
}
#[allow(clippy::many_single_char_names)]
fn apply_heal(image: &mut LinearRgbImage, heal: Heal) -> Result<()> {
    ensure!(
        heal.target
            .into_iter()
            .chain(heal.source)
            .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
            && heal.radius.is_finite()
            && (0.000_1..=0.1).contains(&heal.radius),
        "Invalid dust correction"
    );
    let w = i32::try_from(image.width)?;
    let h = i32::try_from(image.height)?;
    let r = (heal.radius * w.min(h) as f32).max(1.0);
    let ir = r.ceil() as i32;
    let tx = (heal.target[0] * (w - 1) as f32).round() as i32;
    let ty = (heal.target[1] * (h - 1) as f32).round() as i32;
    let sx = (heal.source[0] * (w - 1) as f32).round() as i32;
    let sy = (heal.source[1] * (h - 1) as f32).round() as i32;
    if tx == sx && ty == sy {
        bail!("Move the source circle away from the dust spot");
    }
    let index = |x: i32, y: i32| (y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize;
    let mut delta = [0.0; 3];
    let mut n = 0.0;
    for dy in -ir..=ir {
        for dx in -ir..=ir {
            let d = (dx * dx + dy * dy) as f32;
            if d > r * r * 0.72 && d <= r * r {
                let a = image.pixels[index(tx + dx, ty + dy)];
                let b = image.pixels[index(sx + dx, sy + dy)];
                for c in 0..3 {
                    delta[c] += a[c] - b[c];
                }
                n += 1.0;
            }
        }
    }
    if n > 0.0 {
        delta = delta.map(|v| v / n);
    }
    // Snapshot just the source patch: memory and work scale with marked area.
    let patch: Vec<_> = (-ir..=ir)
        .flat_map(|dy| (-ir..=ir).map(move |dx| (dx, dy)))
        .map(|(dx, dy)| image.pixels[index(sx + dx, sy + dy)])
        .collect();
    for (i, (dx, dy)) in (-ir..=ir)
        .flat_map(|dy| (-ir..=ir).map(move |dx| (dx, dy)))
        .enumerate()
    {
        let distance = ((dx * dx + dy * dy) as f32).sqrt() / r;
        if distance > 1.0 || tx + dx < 0 || ty + dy < 0 || tx + dx >= w || ty + dy >= h {
            continue;
        }
        let a = ((1.0 - distance) / 0.25).clamp(0.0, 1.0);
        let a = a * a * (3.0 - 2.0 * a);
        let dest = &mut image.pixels[index(tx + dx, ty + dy)];
        for c in 0..3 {
            dest[c] = dest[c] * (1.0 - a) + (patch[i][c] + delta[c]) * a;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    #[test]
    fn cached_conversion_matches_direct_render_and_cancels() {
        let scan = scan();
        let analysis = analyze(&[&scan], Some([0.8, 0.5, 0.3]), Rect::default()).unwrap();
        let prepared = prepare(
            &scan,
            Material::ColorNegative,
            Some(&analysis),
            false,
            &|| false,
        )
        .unwrap();
        let edits = Adjustments {
            shadows: 60.0,
            highlights: -40.0,
            ..Adjustments::default()
        };
        let direct = render_linear(
            &scan,
            Material::ColorNegative,
            Some(&analysis),
            edits,
            Rect::default(),
            1,
            &[],
            false,
        )
        .unwrap();
        let cached = finish(&prepared, edits, Rect::default(), 1, &[], &|| false).unwrap();
        assert_eq!(direct.pixels, cached.pixels);
        assert!(finish(&prepared, edits, Rect::default(), 0, &[], &|| true).is_err());
        assert!(
            prepare(
                &scan,
                Material::ColorNegative,
                Some(&analysis),
                false,
                &|| true
            )
            .is_err()
        );
    }
    fn scan() -> Scan {
        Scan {
            image: LinearRgbImage {
                width: 100,
                height: 80,
                pixels: (0..8000)
                    .map(|i| {
                        let d = (i % 100) as f32 / 100.0 * 1.8;
                        [0.8, 0.5, 0.3].map(|v| v * 10.0_f32.powf(-d))
                    })
                    .collect(),
            },
            camera_matrix: None,
            white_balance: [1.0; 3],
            capture: "test".into(),
        }
    }
    #[test]
    fn inversion_is_monotonic_and_bw_is_neutral() {
        let s = scan();
        let a = analyze(&[&s], Some([0.8, 0.5, 0.3]), Rect::default()).unwrap();
        let out = render(
            &s,
            Material::BlackAndWhite,
            Some(&a),
            Adjustments::default(),
            Rect::default(),
            0,
            &[],
            false,
        )
        .unwrap();
        assert!(out.pixels[80][0] > out.pixels[20][0]);
        for p in out.pixels {
            assert!((p[0] - p[1]).abs() < 0.0001 && (p[1] - p[2]).abs() < 0.0001);
        }
    }
    #[test]
    fn source_is_unchanged_and_heal_is_local() {
        let mut image = scan().image;
        image.pixels[40 * 100 + 50] = [1.0; 3];
        let before = image.clone();
        apply_heal(
            &mut image,
            Heal {
                target: [50.0 / 99.0, 40.0 / 79.0],
                source: [0.5, 0.2],
                radius: 0.04,
            },
        )
        .unwrap();
        assert_eq!(image.pixels[0], before.pixels[0]);
        assert!(image.pixels[4050][0] < 0.5);
    }
    #[test]
    fn creative_crop_does_not_change_analysis() {
        let s = scan();
        let a = analyze(&[&s], Some([0.8, 0.5, 0.3]), Rect::default()).unwrap();
        let out = render(
            &s,
            Material::ColorNegative,
            Some(&a),
            Adjustments::default(),
            Rect {
                x: 0.2,
                y: 0.1,
                w: 0.5,
                h: 0.5,
            },
            1,
            &[],
            false,
        )
        .unwrap();
        assert_eq!((out.width, out.height), (40, 50));
        assert_eq!(a.base, [0.8, 0.5, 0.3]);
    }

    #[test]
    fn clipped_patch_and_mixed_capture_roll_are_rejected() {
        let mut s = scan();
        s.image.pixels[..7000].fill([1.0; 3]);
        assert!(measure_base(&s, Rect::default()).is_err());
        let a = scan();
        let mut b = scan();
        b.capture = "different camera".into();
        assert!(analyze(&[&a, &b], None, Rect::default()).is_err());
    }
}
