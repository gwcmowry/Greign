//! Deterministic CPU reference operations for RAW development.

mod color;
mod demosaic;
mod detail;
mod develop;
mod highlights;
mod scheduler;
mod sensor;
mod types;

pub use color::{
    apply_exposure, camera_to_rec2020, camera_to_rec2020_matrix, highlight_tone_curve,
    rec2020_to_srgb, tone_curve,
};
pub use demosaic::{bilinear_demosaic, ppg_demosaic, ppg_demosaic_tiled};
pub use detail::{capture_sharpen, chroma_denoise, luminance};
pub use develop::{
    DevelopSettings, PreparedPreview, QualitySettings, develop_quality, develop_reference,
    finish_quality_preview, prepare_quality_preview,
};
pub use highlights::{HIGHLIGHT_TILE, HighlightContext};
pub use scheduler::{
    BudgetedCache, CacheError, CacheKey, Generation, JobKind, PipelineStage, PresentationSlot,
    ProgressiveRequest, QualityTier, Rect, RenderCoordinator, RenderRequest, ScheduledJob,
    SourceId, TileRegion, ViewZoom, ViewportLevel, WorkQueue, pyramid_dimensions,
    select_viewport_level, tile_regions,
};
pub use sensor::{
    active_crop, apply_white_balance, downsample_bayer, normalize_sensor,
    reconstruct_clipped_samples, repair_bad_pixels,
};
pub use types::{
    CfaColor, CfaPattern, LinearRgbImage, NormalizedMosaic, PipelineError, SensorPoint, SrgbImage,
};
