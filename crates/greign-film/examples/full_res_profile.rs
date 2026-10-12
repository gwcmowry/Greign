//! Diagnostic only: isolate warm full-resolution preview stages, no exports.
use greign_film::{Adjustments, Material, Rect};
use std::{hint::black_box, path::Path, time::Instant};

fn main() -> anyhow::Result<()> {
    let path = Path::new("exports/r7-render-fix/152A0123-after.jpg");
    let start = Instant::now();
    let scan = greign_film::load_scan(path, None, true)?;
    println!("load_demo,0,{:.3}", start.elapsed().as_secs_f64() * 1000.0);
    eprintln!("Dimensions: {} x {}", scan.image.width, scan.image.height);
    let analysis = greign_film::analyze(&[&scan], Some([0.82, 0.52, 0.30]), Rect::default())?;
    let start = Instant::now();
    let prepared = greign_film::prepare(
        &scan,
        Material::ColorNegative,
        Some(&analysis),
        false,
        &|| false,
    )?;
    println!("prepare,0,{:.3}", start.elapsed().as_secs_f64() * 1000.0);
    for repeat in 0..6 {
        let start = Instant::now();
        let linear = greign_film::finish(
            &prepared,
            Adjustments {
                shadows: 30.0,
                ..Adjustments::default()
            },
            Rect::default(),
            0,
            &[],
            &|| false,
        )?;
        println!(
            "adjust_and_crop,{repeat},{:.3}",
            start.elapsed().as_secs_f64() * 1000.0
        );
        let start = Instant::now();
        let display = greign_pipeline::rec2020_to_srgb(linear);
        println!(
            "display_color_conversion,{repeat},{:.3}",
            start.elapsed().as_secs_f64() * 1000.0
        );
        let start = Instant::now();
        let bytes = black_box(display.to_rgb8());
        println!(
            "rgb8_packing,{repeat},{:.3}",
            start.elapsed().as_secs_f64() * 1000.0
        );
        black_box(bytes);
    }
    Ok(())
}
