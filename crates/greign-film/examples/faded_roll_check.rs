//! Reproducible CPU comparisons and optional full-resolution restoration exports.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
use greign_film::{Adjustments, Analysis, CalibrationOptions, Material, Rect};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

#[allow(clippy::too_many_lines)]
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        args.len() >= 2,
        "Usage: faded_roll_check roll-recipe.json output-directory [full]"
    );
    let mut recipe: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let output = PathBuf::from(&args[1]);
    anyhow::ensure!(
        !output.exists(),
        "Choose a new output directory; existing exports are preserved"
    );
    std::fs::create_dir_all(&output)?;
    let previews = output.join("previews");
    std::fs::create_dir_all(&previews)?;
    let full = args.get(2).is_some_and(|v| v == "full");
    let frames = recipe["frames"].as_array().unwrap().clone();
    let area: Rect = serde_json::from_value(recipe["analysis_area"].clone())?;
    let options = CalibrationOptions {
        illumination: true,
        color_balance: 1.,
    };
    let mut sheets: [image::RgbImage; 4] =
        std::array::from_fn(|_| image::RgbImage::new(1500, frames.len().div_ceil(5) as u32 * 200));
    let mut report = Vec::new();
    let mut per_frame = serde_json::Map::new();
    for (index, frame) in frames.iter().enumerate() {
        let path = Path::new(frame["path"].as_str().unwrap());
        let name = path.file_stem().unwrap().to_string_lossy();
        let id = frame["id"].as_str().unwrap();
        let source = greign_film::load_scan(path, Some(960), false)?;
        let start = Instant::now();
        let detected = greign_film::detect_frame_crop(&source);
        let crop = detected.unwrap_or(serde_json::from_value(frame["crop"].clone())?);
        // The existing manual crop supplies a picture-only analysis area for
        // incomplete captures; this explicit choice is recorded in the report.
        let area = if detected.is_none() {
            crop.intersection(area).unwrap_or(area)
        } else {
            area
        };
        let baseline = greign_film::analyze(&[&source], None, area)?;
        let balanced = greign_film::analyze_with_options(
            &[&source],
            None,
            area,
            CalibrationOptions {
                illumination: false,
                ..options
            },
        )?;
        let restored = greign_film::analyze_with_options(&[&source], None, area, options)?;
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.;
        let previous: Analysis = serde_json::from_value(
            if recipe["analysis"]["individual"].as_bool().unwrap_or(false) {
                recipe["analysis"]["per_frame"][id].clone()
            } else {
                recipe["analysis"].clone()
            },
        )?;
        let edits: Adjustments = serde_json::from_value(recipe["adjustments"].clone())?;
        let edits = edits.combined(serde_json::from_value(frame["adjustments"].clone())?);
        let rotation = frame["rotation"].as_u64().unwrap() as u8;
        let mirrored = frame["mirrored"].as_bool().unwrap_or(false);
        let heals: Vec<greign_film::Heal> = serde_json::from_value(frame["heals"].clone())?;
        for (variant, (label, analysis, edits, crop)) in [
            (
                "saved",
                &previous,
                edits,
                serde_json::from_value(frame["crop"].clone())?,
            ),
            ("recropped", &baseline, Adjustments::default(), crop),
            ("balanced", &balanced, Adjustments::default(), crop),
            ("restored", &restored, Adjustments::default(), crop),
        ]
        .into_iter()
        .enumerate()
        {
            let rgb = greign_film::render(
                &source,
                Material::ColorNegative,
                Some(analysis),
                edits,
                crop,
                rotation,
                mirrored,
                &heals,
                false,
            )?;
            greign_export::export_srgb(
                &rgb,
                &previews.join(format!("{name}-{label}.jpg")),
                greign_export::ExportOptions { jpeg_quality: 95 },
            )?;
            let im = image::RgbImage::from_raw(rgb.width, rgb.height, rgb.to_rgb8()).unwrap();
            let tile = image::DynamicImage::ImageRgb8(im)
                .thumbnail(300, 200)
                .to_rgb8();
            image::imageops::replace(
                &mut sheets[variant],
                &tile,
                i64::try_from(index % 5 * 300)?,
                i64::try_from(index / 5 * 200)?,
            );
        }
        if full {
            let full_scan = greign_film::load_scan(path, None, false)?;
            let rgb = greign_film::render(
                &full_scan,
                Material::ColorNegative,
                Some(&restored),
                Adjustments::default(),
                crop,
                rotation,
                mirrored,
                &heals,
                false,
            )?;
            greign_export::export_srgb(
                &rgb,
                &output.join(format!("{:02}-{name}-positive.jpg", index + 1)),
                greign_export::ExportOptions { jpeg_quality: 95 },
            )?;
        }
        let row = serde_json::json!({"name":name,"crop":detected,"analysis_area":area,"analysis":restored,"analysis_ms":elapsed_ms});
        println!("{row}");
        report.push(row);
        per_frame.insert(id.into(), serde_json::to_value(&restored)?);
        recipe["frames"][index]["crop"] = serde_json::to_value(crop)?;
        recipe["frames"][index]["adjustments"] = serde_json::to_value(Adjustments::default())?;
    }
    for (label, sheet) in ["saved", "recropped", "balanced", "restored"]
        .into_iter()
        .zip(sheets)
    {
        sheet.save(output.join(format!("{label}-sheet.jpg")))?;
    }
    recipe["previous_analysis"] = recipe["analysis"].clone();
    recipe["analysis"] = per_frame.values().next().unwrap().clone();
    recipe["analysis"]["individual"] = true.into();
    recipe["analysis"]["per_frame"] = per_frame.into();
    recipe["analysis"]["frames"] = frames.len().into();
    recipe["analysis"]["method"] = "Frames optimized individually".into();
    recipe["adjustments"] = serde_json::to_value(Adjustments::default())?;
    recipe["calibration"] = serde_json::to_value(options)?;
    recipe["optimize_individually"] = true.into();
    std::fs::write(
        output.join("roll-recipe.json"),
        serde_json::to_vec_pretty(&recipe)?,
    )?;
    std::fs::write(
        output.join("analysis-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
