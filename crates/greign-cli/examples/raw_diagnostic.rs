//! CPU-only evidence dump for independent RAW decoder and demosaic diagnosis.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};
use greign_pipeline::{
    active_crop, apply_white_balance, bilinear_demosaic, camera_to_rec2020, capture_sharpen,
    normalize_sensor, ppg_demosaic, reconstruct_clipped_samples,
};
use greign_raw::{RawDecoder, RawlerDecoder};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 3,
        "usage: raw_diagnostic INPUT OUTPUT_DIRECTORY"
    );
    let output = Path::new(&args[2]);
    fs::create_dir_all(output)?;
    let raw = RawlerDecoder.mosaic(Path::new(&args[1]))?;
    fs::write(
        output.join("metadata.json"),
        serde_json::to_vec_pretty(&raw.metadata)?,
    )?;
    let mut writer = BufWriter::new(File::create(output.join("mosaic.u16le"))?);
    for value in &raw.samples {
        writer.write_all(&value.to_le_bytes())?;
    }
    writer.flush()?;

    let mut sensor = normalize_sensor(&raw)?;
    if let Some(area) = raw.metadata.crop_area.or(raw.metadata.active_area) {
        sensor = active_crop(&sensor, area.x, area.y, area.width, area.height)?;
    }
    apply_white_balance(&mut sensor, raw.metadata.white_balance)?;
    // Fixed bokeh ROI in the public R7 flower fixture. Keep a halo and discard
    // it when measuring; no resized preview or JPEG enters these measurements.
    let roi = [1400, 800, 512, 512];
    let mut sensor = active_crop(&sensor, roi[0], roi[1], roi[2], roi[3])
        .context("diagnostic ROI requires a full-resolution input")?;
    let unrepaired = sensor.clone();
    reconstruct_clipped_samples(&mut sensor, raw.metadata.white_balance.map(|v| v * 0.995))?;
    let repaired_count = sensor
        .samples
        .iter()
        .zip(&unrepaired.samples)
        .filter(|(a, b)| a.to_bits() != b.to_bits())
        .count();
    let mut records = Vec::new();
    for (name, mut rgb) in [
        ("bilinear", bilinear_demosaic(&unrepaired)?),
        ("ppg", ppg_demosaic(&unrepaired)?),
        ("ppg_highlights", ppg_demosaic(&sensor)?),
    ] {
        let mut writer = BufWriter::new(File::create(output.join(format!("{name}.rgb.f32le")))?);
        for pixel in &rgb.pixels {
            for value in pixel {
                writer.write_all(&value.to_le_bytes())?;
            }
        }
        writer.flush()?;
        camera_to_rec2020(&mut rgb, raw.metadata.xyz_to_camera_d65)?;
        for amount in [0.0, 0.2] {
            let mut variant = rgb.clone();
            capture_sharpen(&mut variant, amount)?;
            // First differences suppress the smooth scene gradient; these are
            // chroma roughness measurements, not a calibrated sensor-noise model.
            let mut sum = [0.0_f64; 2];
            let mut count = 0_u32;
            for y in 8..variant.height - 8 {
                for x in 8..variant.width - 8 {
                    let p = variant.pixels[(y * variant.width + x) as usize];
                    let q = variant.pixels[(y * variant.width + x + 1) as usize];
                    for (i, channel) in [0, 2].iter().enumerate() {
                        let delta = f64::from((p[*channel] - p[1]) - (q[*channel] - q[1]));
                        sum[i] += delta * delta;
                    }
                    count += 1;
                }
            }
            records.push(serde_json::json!({
                "demosaic": name, "sharpening": amount,
                "rg_difference_rms": (sum[0] / f64::from(count)).sqrt(),
                "bg_difference_rms": (sum[1] / f64::from(count)).sqrt(),
            }));
        }
    }
    fs::write(
        output.join("stages.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "input": args[1], "roi_in_greign_crop": roi,
            "highlight_samples_changed": repaired_count, "working_space": "linear Rec.2020",
            "measurements": records,
        }))?,
    )?;
    println!(
        "Saved decoder samples and CPU stage measurements to {}",
        output.display()
    );
    Ok(())
}
