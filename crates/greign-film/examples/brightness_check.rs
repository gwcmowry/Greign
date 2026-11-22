//! Compare saved and current calibrations using local cached linear scans.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
use greign_film::{Adjustments, Analysis, Material, Rect, Scan};
use std::path::Path;

fn scan(directory: &Path, name: &str) -> anyhow::Result<Scan> {
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join(format!("{name}.json")))?)?;
    let raw = std::fs::read(directory.join(format!("{name}.f32")))?;
    Ok(Scan {
        image: greign_pipeline::LinearRgbImage {
            width: meta["width"].as_u64().unwrap() as u32,
            height: meta["height"].as_u64().unwrap() as u32,
            pixels: raw
                .chunks_exact(12)
                .map(|p| {
                    std::array::from_fn(|c| {
                        f32::from_le_bytes(p[c * 4..c * 4 + 4].try_into().unwrap())
                    })
                })
                .collect(),
        },
        camera_matrix: serde_json::from_value(meta["camera_matrix"].clone())?,
        white_balance: serde_json::from_value(meta["white_balance"].clone())?,
        capture: meta["capture"].as_str().unwrap().into(),
    })
}
fn stats(rgb: &greign_pipeline::LinearRgbImage) -> serde_json::Value {
    let mut light: Vec<_> = rgb
        .pixels
        .iter()
        .map(|&p| greign_pipeline::luminance(p).max(0.))
        .collect();
    light.sort_unstable_by(f32::total_cmp);
    let q = |fraction: f32| light[((light.len() - 1) as f32 * fraction) as usize];
    serde_json::json!({"p10":q(0.1),"p50":q(0.5),"p75":q(0.75),"p95":q(0.95),"p99":q(0.99),"over_white_percent":light.iter().filter(|v|**v>1.).count() as f64*100./light.len() as f64})
}
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let recipe: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let cache = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    std::fs::create_dir_all(output)?;
    let frames = recipe["frames"].as_array().unwrap();
    let mut sheets: [image::RgbImage; 3] = std::array::from_fn(|_| {
        image::RgbImage::new(7 * 240, frames.len().div_ceil(7) as u32 * 180)
    });
    for (index, frame) in frames.iter().enumerate() {
        let name = Path::new(frame["name"].as_str().unwrap())
            .file_stem()
            .unwrap()
            .to_string_lossy();
        let source = scan(cache, &name)?;
        let previous: Analysis = serde_json::from_value(
            recipe["analysis"]["per_frame"][frame["id"].as_str().unwrap()].clone(),
        )?;
        let fresh = greign_film::analyze(
            &[&source],
            None,
            serde_json::from_value(recipe["analysis_area"].clone())?,
        )?;
        let crop: Rect = serde_json::from_value(frame["crop"].clone())?;
        let roll: Adjustments = serde_json::from_value(recipe["adjustments"].clone())?;
        let edits = roll.combined(serde_json::from_value(frame["adjustments"].clone())?);
        let mut metrics = Vec::new();
        for (variant, (label, analysis, edits)) in [
            ("saved", &previous, edits),
            ("reanalyzed", &fresh, edits),
            ("neutral", &fresh, Adjustments::default()),
        ]
        .into_iter()
        .enumerate()
        {
            let rgb = greign_film::render_linear(
                &source,
                Material::ColorNegative,
                Some(analysis),
                edits,
                crop,
                frame["rotation"].as_u64().unwrap() as u8,
                frame["mirrored"].as_bool().unwrap_or(false),
                &[],
                false,
            )?;
            metrics.push(serde_json::json!({"variant":label,"stats":stats(&rgb)}));
            let rgb = greign_pipeline::rec2020_to_srgb(rgb);
            let image = image::RgbImage::from_raw(rgb.width, rgb.height, rgb.to_rgb8()).unwrap();
            image.save(output.join(format!("{name}-{label}.jpg")))?;
            let tile = image::imageops::thumbnail(&image, 240, 180);
            image::imageops::replace(
                &mut sheets[variant],
                &tile,
                i64::try_from(index % 7 * 240)?,
                i64::try_from(index / 7 * 180)?,
            );
        }
        println!(
            "{}",
            serde_json::json!({"name":name,"saved_range":previous.range,"fresh":fresh,"variants":metrics})
        );
    }
    for (name, sheet) in ["saved", "reanalyzed", "neutral"].into_iter().zip(sheets) {
        sheet.save(output.join(format!("{name}-sheet.jpg")))?;
    }
    Ok(())
}
