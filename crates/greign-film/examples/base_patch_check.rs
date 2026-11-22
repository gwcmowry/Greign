//! Compare a stored conversion with automatic and measured-base frame analysis.
#![allow(clippy::cast_possible_truncation)]
use greign_film::{Adjustments, Analysis, CalibrationOptions, Material, Rect};
use std::{collections::BTreeMap, path::Path};

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        args.len() == 3,
        "Usage: base_patch_check roll-recipe.json patches.json output-directory"
    );
    let recipe: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let patches: BTreeMap<String, Rect> = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let output = Path::new(&args[2]);
    std::fs::create_dir_all(output)?;
    let area: Rect = serde_json::from_value(recipe["analysis_area"].clone())?;
    let options: CalibrationOptions = serde_json::from_value(recipe["calibration"].clone())?;
    let roll_edits: Adjustments = serde_json::from_value(recipe["adjustments"].clone())?;
    let mut report = Vec::new();
    for frame in recipe["frames"].as_array().unwrap() {
        let name = frame["name"].as_str().unwrap();
        let Some(patch) = patches.get(name) else {
            continue;
        };
        let source_path = Path::new(frame["path"].as_str().unwrap());
        let stem = source_path.file_stem().unwrap().to_string_lossy();
        let scan = greign_film::load_scan(source_path, Some(960), false)?;
        let base = greign_film::measure_base(&scan, *patch)?;
        let manual = greign_film::analyze_with_options(&[&scan], Some(base), area, options)?;
        let automatic = greign_film::analyze_with_options(&[&scan], None, area, options)?;
        let saved: Analysis = serde_json::from_value(
            recipe["analysis"]["per_frame"][frame["id"].as_str().unwrap()].clone(),
        )?;
        let crop: Rect = serde_json::from_value(frame["crop"].clone())?;
        let edits = roll_edits.combined(serde_json::from_value(frame["adjustments"].clone())?);
        for (label, analysis, edits, negative) in [
            ("saved", &saved, edits, false),
            ("automatic", &automatic, Adjustments::default(), false),
            ("manual", &manual, Adjustments::default(), false),
            ("negative", &manual, Adjustments::default(), true),
        ] {
            let rgb = greign_film::render(
                &scan,
                Material::ColorNegative,
                Some(analysis),
                edits,
                crop,
                frame["rotation"].as_u64().unwrap() as u8,
                frame["mirrored"].as_bool().unwrap_or(false),
                &[],
                negative,
            )?;
            greign_export::export_srgb(
                &rgb,
                &output.join(format!("{stem}-{label}.jpg")),
                greign_export::ExportOptions { jpeg_quality: 95 },
            )?;
        }
        let row = serde_json::json!({"name":name,"patch":patch,"base":base,"automatic":automatic,"manual":manual});
        println!("{row}");
        report.push(row);
    }
    std::fs::write(
        output.join("measurements.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
