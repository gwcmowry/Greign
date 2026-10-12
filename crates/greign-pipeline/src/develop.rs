//! Ordered composition of the CPU reference pipeline.

use greign_raw::{Mosaic, Orientation};

use crate::{
    PipelineError, active_crop, apply_exposure, apply_white_balance, bilinear_demosaic,
    camera_to_rec2020, capture_sharpen, chroma_denoise, downsample_bayer, highlight_tone_curve,
    normalize_sensor, ppg_demosaic, rec2020_to_srgb, reconstruct_clipped_samples,
    repair_bad_pixels, tone_curve,
};

/// Parameters for the Gate 2 reference rendering path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DevelopSettings {
    /// Exposure compensation in stops.
    pub exposure_ev: f32,
}

impl Default for DevelopSettings {
    fn default() -> Self {
        Self { exposure_ev: 0.0 }
    }
}

/// Parameters for the Gate 4 quality rendering path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QualitySettings {
    /// Exposure compensation in stops.
    pub exposure_ev: f32,
    /// Normalized sensor level treated as clipped.
    pub highlight_threshold: f32,
    /// Linear unsharp-mask amount.
    pub capture_sharpening: f32,
    /// Edge-aware chroma-noise reduction, zero (off) through one.
    pub chroma_denoise: f32,
}

impl Default for QualitySettings {
    fn default() -> Self {
        Self {
            exposure_ev: 0.0,
            highlight_threshold: 0.995,
            capture_sharpening: 0.2,
            chroma_denoise: 0.5,
        }
    }
}

/// Cached scene-linear working image used for fast slider updates.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedPreview {
    working: crate::LinearRgbImage,
    orientation: Orientation,
}

impl PreparedPreview {
    /// Width before display orientation.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.working.width
    }

    /// Height before display orientation.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.working.height
    }

    /// Accounted pixel payload for a CPU preview cache.
    #[must_use]
    pub fn byte_len(&self) -> u64 {
        u64::try_from(std::mem::size_of_val(self.working.pixels.as_slice())).unwrap_or(u64::MAX)
    }
}

/// Runs the deterministic CPU reference path from integer mosaic to sRGB.
///
/// # Errors
///
/// Returns [`PipelineError`] when input metadata or parameters violate an
/// operator invariant.
pub fn develop_reference(
    mosaic: &Mosaic,
    settings: DevelopSettings,
) -> Result<crate::SrgbImage, PipelineError> {
    let mut normalized = normalize_sensor(mosaic)?;
    repair_bad_pixels(&mut normalized, &[]);
    if let Some(area) = mosaic.metadata.crop_area.or(mosaic.metadata.active_area) {
        normalized = active_crop(&normalized, area.x, area.y, area.width, area.height)?;
    }
    apply_white_balance(&mut normalized, mosaic.metadata.white_balance)?;
    let mut rgb = bilinear_demosaic(&normalized)?;
    camera_to_rec2020(&mut rgb, mosaic.metadata.xyz_to_camera_d65)?;
    apply_exposure(&mut rgb, settings.exposure_ev)?;
    tone_curve(&mut rgb);
    Ok(rec2020_to_srgb(rgb).oriented(mosaic.metadata.orientation))
}

/// Runs the initial Gate 4 quality path using edge-aware demosaic, conservative
/// clipped-sample reconstruction, and capture sharpening.
///
/// # Errors
///
/// Returns [`PipelineError`] when input metadata or parameters violate an
/// operator invariant.
pub fn develop_quality(
    mosaic: &Mosaic,
    settings: QualitySettings,
) -> Result<crate::SrgbImage, PipelineError> {
    let mut normalized = normalize_sensor(mosaic)?;
    repair_bad_pixels(&mut normalized, &[]);
    if let Some(area) = mosaic.metadata.crop_area.or(mosaic.metadata.active_area) {
        normalized = active_crop(&normalized, area.x, area.y, area.width, area.height)?;
    }
    apply_white_balance(&mut normalized, mosaic.metadata.white_balance)?;
    reconstruct_clipped_samples(
        &mut normalized,
        mosaic
            .metadata
            .white_balance
            .map(|coefficient| coefficient * settings.highlight_threshold),
    )?;
    let mut rgb = ppg_demosaic(&normalized)?;
    camera_to_rec2020(&mut rgb, mosaic.metadata.xyz_to_camera_d65)?;
    chroma_denoise(&mut rgb, settings.chroma_denoise)?;
    apply_exposure(&mut rgb, settings.exposure_ev)?;
    capture_sharpen(&mut rgb, settings.capture_sharpening)?;
    highlight_tone_curve(&mut rgb);
    Ok(rec2020_to_srgb(rgb).oriented(mosaic.metadata.orientation))
}

/// Builds the expensive, edit-independent portion of a reduced quality
/// preview for reuse across exposure and tone interactions.
///
/// # Errors
///
/// Returns [`PipelineError`] when input metadata, the highlight threshold, or
/// the requested maximum dimension violates an operator invariant.
pub fn prepare_quality_preview(
    mosaic: &Mosaic,
    max_dimension: u32,
    highlight_threshold: f32,
) -> Result<PreparedPreview, PipelineError> {
    let mut normalized = normalize_sensor(mosaic)?;
    repair_bad_pixels(&mut normalized, &[]);
    if let Some(area) = mosaic.metadata.crop_area.or(mosaic.metadata.active_area) {
        normalized = active_crop(&normalized, area.x, area.y, area.width, area.height)?;
    }
    apply_white_balance(&mut normalized, mosaic.metadata.white_balance)?;
    reconstruct_clipped_samples(
        &mut normalized,
        mosaic
            .metadata
            .white_balance
            .map(|coefficient| coefficient * highlight_threshold),
    )?;
    let reduced = downsample_bayer(&normalized, max_dimension)?;
    let mut working = ppg_demosaic(&reduced)?;
    camera_to_rec2020(&mut working, mosaic.metadata.xyz_to_camera_d65)?;
    Ok(PreparedPreview {
        working,
        orientation: mosaic.metadata.orientation,
    })
}

/// Applies interaction-dependent operations to a cached prepared preview.
///
/// # Errors
///
/// Returns [`PipelineError`] for invalid exposure or sharpening parameters.
pub fn finish_quality_preview(
    preview: &PreparedPreview,
    settings: QualitySettings,
) -> Result<crate::SrgbImage, PipelineError> {
    let mut working = preview.working.clone();
    chroma_denoise(&mut working, settings.chroma_denoise)?;
    apply_exposure(&mut working, settings.exposure_ev)?;
    capture_sharpen(&mut working, settings.capture_sharpening)?;
    highlight_tone_curve(&mut working);
    Ok(rec2020_to_srgb(working).oriented(preview.orientation))
}
