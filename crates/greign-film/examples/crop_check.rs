#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
//! CPU-only crop regression diagnostics on local RAW files or cached linear previews.
use greign_film::{
    Adjustments, Material, Rect, Scan, analyze, detect_frame_crop, load_scan, render,
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

fn cache(path: &Path, directory: &Path) -> anyhow::Result<Scan> {
    let name = path.file_stem().unwrap().to_string_lossy();
    let pixels = directory.join(format!("{name}.f32"));
    let metadata = directory.join(format!("{name}.json"));
    if pixels.exists() && metadata.exists() {
        let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(metadata)?)?;
        let data = std::fs::read(pixels)?;
        return Ok(Scan {
            image: greign_pipeline::LinearRgbImage {
                width: meta["width"].as_u64().unwrap() as u32,
                height: meta["height"].as_u64().unwrap() as u32,
                pixels: data
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
        });
    }
    let scan = load_scan(path, Some(960), false)?;
    std::fs::create_dir_all(directory)?;
    let mut file = std::io::BufWriter::new(std::fs::File::create(pixels)?);
    for channel in scan.image.pixels.iter().flatten() {
        file.write_all(&channel.to_le_bytes())?;
    }
    std::fs::write(
        metadata,
        serde_json::to_vec(
            &serde_json::json!({"width":scan.image.width,"height":scan.image.height,"camera_matrix":scan.camera_matrix,"white_balance":scan.white_balance,"capture":scan.capture}),
        )?,
    )?;
    Ok(scan)
}
fn overlay(scan: &Scan, crop: Option<Rect>, path: &Path) -> anyhow::Result<()> {
    let rgb = render(
        scan,
        Material::ColorNegative,
        None,
        Adjustments::default(),
        Rect::default(),
        0,
        false,
        &[],
        true,
    )?;
    let mut rgb = image::RgbImage::from_raw(rgb.width, rgb.height, rgb.to_rgb8()).unwrap();
    if let Some(crop) = crop {
        for i in 0..4000 {
            let t = (i % 1000) as f32 / 999.;
            let p = match i / 1000 {
                0 => [t, 0.],
                1 => [1., t],
                2 => [1. - t, 1.],
                _ => [0., 1. - t],
            };
            let p = crop.source_point(p, rgb.width() as f32 / rgb.height() as f32);
            let (x, y) = (
                (p[0] * rgb.width() as f32) as u32,
                (p[1] * rgb.height() as f32) as u32,
            );
            if x < rgb.width() && y < rgb.height() {
                rgb.put_pixel(x, y, image::Rgb([0, 255, 0]));
            }
        }
    }
    rgb.save(path)?;
    Ok(())
}
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let input = PathBuf::from(args.next().expect("RAW directory"));
    let output = PathBuf::from(args.next().expect("diagnostic output directory"));
    let cached = PathBuf::from(args.next().expect("linear preview cache directory"));
    let repetitions: u32 = args.next().map_or(Ok(1), |v| v.parse())?;
    anyhow::ensure!(repetitions > 0, "Repetitions must be positive");
    std::fs::create_dir_all(&output)?;
    let mut paths: Vec<_> = std::fs::read_dir(input)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            !p.file_name().unwrap().to_string_lossy().starts_with('.')
                && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cr3"))
        })
        .collect();
    paths.sort();
    let mut sheet = image::RgbImage::new(7 * 240, paths.len().div_ceil(7) as u32 * 160);
    for (index, path) in paths.iter().enumerate() {
        let name = path.file_stem().unwrap().to_string_lossy();
        let scan = cache(path, &cached)?;
        let start = Instant::now();
        let mut crop = None;
        for _ in 0..repetitions {
            crop = std::hint::black_box(detect_frame_crop(std::hint::black_box(&scan)));
        }
        println!(
            "{}",
            serde_json::json!({"name":name,"crop":crop,"elapsed_ms":start.elapsed().as_secs_f64()*1000./f64::from(repetitions)})
        );
        if repetitions > 1 {
            continue;
        }
        overlay(&scan, crop, &output.join(format!("{name}-boundary.jpg")))?;
        let tile = image::open(output.join(format!("{name}-boundary.jpg")))?
            .thumbnail(240, 160)
            .to_rgb8();
        image::imageops::replace(
            &mut sheet,
            &tile,
            i64::try_from(index % 7 * 240)?,
            i64::try_from(index / 7 * 160)?,
        );
        if let Some(crop) = crop {
            let analysis = analyze(&[&scan], None, Rect::default())?;
            let rgb = render(
                &scan,
                Material::ColorNegative,
                Some(&analysis),
                Adjustments::default(),
                crop,
                0,
                false,
                &[],
                false,
            )?;
            image::save_buffer(
                output.join(format!("{name}-crop.jpg")),
                &rgb.to_rgb8(),
                rgb.width,
                rgb.height,
                image::ColorType::Rgb8,
            )?;
        }
    }
    if repetitions == 1 {
        sheet.save(output.join("boundaries.jpg"))?;
    }
    Ok(())
}
