//! CPU-only DNG interoperability samples, saved visibly under exports/.
use greign_film::{Adjustments, Material, Rect};
use std::{path::Path, time::Instant};

fn main() -> anyhow::Result<()> {
    if let Some(path) = std::env::args().nth(1) {
        let image = image::open(path)?.to_rgb32f();
        if let Some(reference) = std::env::args().nth(2) {
            let reference = image::open(reference)?.to_rgb32f();
            anyhow::ensure!(
                image.dimensions() == reference.dimensions(),
                "Image dimensions differ"
            );
            let mut sum = 0.0_f64;
            let mut maximum = 0.0_f32;
            for (a, b) in image.as_raw().iter().zip(reference.as_raw()) {
                let error = (a - b).abs();
                sum += f64::from(error);
                maximum = maximum.max(error);
            }
            #[allow(clippy::cast_precision_loss)]
            let mean = sum / image.as_raw().len() as f64;
            println!("mean_absolute_srgb_error={mean:.8} max={maximum:.8}");
            return Ok(());
        }
        for i in 0..6 {
            println!(
                "patch {i}: {:?}",
                image
                    .get_pixel((i * 2 + 1) * image.width() / 12, image.height() / 2)
                    .0
            );
        }
        return Ok(());
    }
    let directory = Path::new("exports/dng-handoff");
    std::fs::create_dir_all(directory)?;
    let source = Path::new("exports/r7-render-fix/152A0123-after.jpg");
    let start = Instant::now();
    let scan = greign_film::load_scan(source, Some(1920), true)?;
    let analysis = greign_film::analyze(&[&scan], Some([0.82, 0.52, 0.30]), Rect::default())?;
    println!(
        "load_analyze_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.0
    );
    let start = Instant::now();
    let linear = greign_film::render_linear(
        &scan,
        Material::ColorNegative,
        Some(&analysis),
        Adjustments::default(),
        Rect::default(),
        0,
        false,
        &[],
        false,
    )?;
    println!("render_ms={:.3}", start.elapsed().as_secs_f64() * 1000.0);
    let start = Instant::now();
    greign_export::export_linear_dng(&linear, &directory.join("simulated-positive.dng"))?;
    println!(
        "dng_encode_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.0
    );
    greign_export::export_srgb(
        &greign_pipeline::rec2020_to_srgb(linear),
        &directory.join("greign-reference.tif"),
        greign_export::ExportOptions::default(),
    )?;
    let pixels = (0..128)
        .flat_map(|_| {
            (0..192).map(|x| match x / 32 {
                0 => [0.18; 3],
                1 => [0.5; 3],
                2 => [1.0; 3],
                3 => [0.5, 0.1, 0.1],
                4 => [0.1, 0.5, 0.1],
                _ => [0.1, 0.1, 0.5],
            })
        })
        .collect();
    greign_export::export_linear_dng(
        &greign_pipeline::LinearRgbImage {
            width: 192,
            height: 128,
            pixels,
        },
        &directory.join("color-check.dng"),
    )?;
    Ok(())
}
