//! CPU stage timings for the optional calibration on one real RAW scan.
use greign_film::{Adjustments, CalibrationOptions, Material, Rect};
use std::{hint::black_box, path::PathBuf, time::Instant};

fn main() -> anyhow::Result<()> {
    let path = PathBuf::from(std::env::args().nth(1).expect("RAW path"));
    let start = Instant::now();
    let preview = greign_film::load_scan(&path, Some(960), false)?;
    println!(
        "preview_decode_ms,{:.3}",
        start.elapsed().as_secs_f64() * 1000.
    );
    let start = Instant::now();
    let scan = greign_film::load_scan(&path, None, false)?;
    println!(
        "full_decode_ms,{:.3}",
        start.elapsed().as_secs_f64() * 1000.
    );
    for enabled in [false, true] {
        let start = Instant::now();
        let analysis = greign_film::analyze_with_options(
            &[&preview],
            None,
            Rect::default(),
            CalibrationOptions {
                illumination: enabled,
                color_balance: if enabled { 1. } else { 0. },
            },
        )?;
        println!(
            "analysis_ms,{enabled},{:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
        for repeat in 0..3 {
            let start = Instant::now();
            let prepared = greign_film::prepare(
                &scan,
                Material::ColorNegative,
                Some(&analysis),
                false,
                &|| false,
            )?;
            println!(
                "prepare_ms,{enabled},{repeat},{:.3}",
                start.elapsed().as_secs_f64() * 1000.
            );
            let start = Instant::now();
            let linear = greign_film::finish(
                &prepared,
                Adjustments::default(),
                Rect::default(),
                0,
                false,
                &[],
                &|| false,
            )?;
            println!(
                "finish_ms,{enabled},{repeat},{:.3}",
                start.elapsed().as_secs_f64() * 1000.
            );
            let start = Instant::now();
            let srgb = greign_pipeline::rec2020_to_srgb(linear);
            println!(
                "display_ms,{enabled},{repeat},{:.3}",
                start.elapsed().as_secs_f64() * 1000.
            );
            let start = Instant::now();
            black_box(srgb.to_rgb8());
            println!(
                "pack_ms,{enabled},{repeat},{:.3}",
                start.elapsed().as_secs_f64() * 1000.
            );
        }
    }
    Ok(())
}
