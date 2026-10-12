//! Repeated wall-clock stage profiling without privileged performance counters.

use anyhow::Result;
use greign_pipeline::{self as p, QualitySettings};
use greign_raw::{RawDecoder, RawlerDecoder};
use std::{fs, path::Path, time::Instant};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() >= 4,
        "usage: render_stages RAW JPEG REPORT [REPETITIONS] [DENOISE]"
    );
    let iterations: usize = args.get(4).map_or(Ok(3), |v| v.parse())?;
    anyhow::ensure!(iterations > 0, "repetitions must be positive");
    let settings = QualitySettings {
        chroma_denoise: args.get(5).map_or(Ok(0.5), |v| v.parse())?,
        ..QualitySettings::default()
    };
    let mut runs = Vec::new();
    for iteration in 0..iterations {
        let mut timings = serde_json::Map::new();
        let mut start = Instant::now();
        macro_rules! stage {
            ($name:expr, $body:expr) => {{
                let result = $body;
                timings.insert(
                    $name.into(),
                    serde_json::json!(start.elapsed().as_secs_f64() * 1000.0),
                );
                start = Instant::now();
                result
            }};
        }
        let raw = stage!("decode_ms", RawlerDecoder.mosaic(Path::new(&args[1]))?);
        let mut sensor = stage!("normalize_ms", p::normalize_sensor(&raw)?);
        if let Some(crop) = raw.metadata.crop_area.or(raw.metadata.active_area) {
            sensor = stage!(
                "crop_ms",
                p::active_crop(&sensor, crop.x, crop.y, crop.width, crop.height)?
            );
        }
        stage!(
            "white_balance_ms",
            p::apply_white_balance(&mut sensor, raw.metadata.white_balance)?
        );
        stage!(
            "highlights_ms",
            p::reconstruct_clipped_samples(
                &mut sensor,
                raw.metadata
                    .white_balance
                    .map(|v| v * settings.highlight_threshold)
            )?
        );
        let mut rgb = stage!("demosaic_ms", p::ppg_demosaic(&sensor)?);
        stage!(
            "color_ms",
            p::camera_to_rec2020(&mut rgb, raw.metadata.xyz_to_camera_d65)?
        );
        stage!(
            "chroma_denoise_ms",
            p::chroma_denoise(&mut rgb, settings.chroma_denoise)?
        );
        stage!(
            "sharpen_ms",
            p::capture_sharpen(&mut rgb, settings.capture_sharpening)?
        );
        stage!("tone_ms", p::highlight_tone_curve(&mut rgb));
        let image = stage!(
            "display_ms",
            p::rec2020_to_srgb(rgb).oriented(raw.metadata.orientation)
        );
        if iteration + 1 == iterations {
            let pixels = stage!("pack_ms", image.to_rgb8());
            stage!(
                "jpeg_ms",
                greign_export::export_rgb8_jpeg_fast(
                    image.width,
                    image.height,
                    &pixels,
                    Path::new(&args[2]),
                    95
                )?
            );
        }
        std::hint::black_box(start);
        runs.push(timings);
    }
    let report = serde_json::json!({"input": args[1], "output": args[2], "chroma_denoise": settings.chroma_denoise, "runs": runs});
    fs::write(&args[3], serde_json::to_vec_pretty(&report)?)?;
    println!("Saved {} render timings to {}", iterations, args[3]);
    Ok(())
}
