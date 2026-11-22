//! CPU-only real-scan diagnostics. Inputs and generated photographs stay local.
use greign_film::{Adjustments, Material, Rect, analyze, detect_frame_crop, load_scan, render};
use std::{path::PathBuf, time::Instant};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let input = PathBuf::from(args.next().expect("input directory"));
    let output = PathBuf::from(args.next().expect("output directory"));
    // Optional repetitions isolate detector cost for cargo-flamegraph.
    let repetitions: u32 = args.next().map_or(Ok(1), |n| n.parse())?;
    anyhow::ensure!(repetitions > 0, "Repetitions must be positive");
    std::fs::create_dir_all(&output)?;
    let mut paths: Vec<_> = std::fs::read_dir(input)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            !path.file_name().unwrap().to_string_lossy().starts_with('.')
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("cr3"))
        })
        .collect();
    paths.sort();
    let mut scans = Vec::new();
    for path in &paths {
        let start = Instant::now();
        let scan = load_scan(path, Some(960), false)?;
        println!("load,{},{}", path.display(), start.elapsed().as_millis());
        scans.push(scan);
    }
    let refs: Vec<_> = scans.iter().collect();
    let analysis = analyze(
        &refs,
        None,
        Rect {
            x: 0.08,
            y: 0.08,
            w: 0.84,
            h: 0.84,
            angle: 0.0,
        },
    )?;
    println!("analysis,{analysis:?}");
    for (path, scan) in paths.iter().zip(&scans) {
        let name = path.file_stem().unwrap().to_string_lossy();
        let start = Instant::now();
        let mut crop = None;
        for _ in 0..repetitions {
            crop = std::hint::black_box(detect_frame_crop(std::hint::black_box(scan)));
        }
        let crop_us = start.elapsed().as_micros() / u128::from(repetitions);
        let individual = analyze(&[scan], None, Rect::default())?;
        println!("frame-analysis,{name},{individual:?}");
        println!("crop,{name},{crop:?},{crop_us}us");
        for (label, negative) in [("negative", true), ("positive", false)] {
            let rgb = render(
                scan,
                Material::ColorNegative,
                Some(&analysis),
                Adjustments::default(),
                Rect::default(),
                0,
                false,
                &[],
                negative,
            )?;
            image::save_buffer(
                output.join(format!("{name}-{label}.jpg")),
                &rgb.to_rgb8(),
                rgb.width,
                rgb.height,
                image::ColorType::Rgb8,
            )?;
        }
        if let Some(crop) = crop {
            for (label, calibration) in [("cropped", &analysis), ("individual", &individual)] {
                let rgb = render(
                    scan,
                    Material::ColorNegative,
                    Some(calibration),
                    Adjustments::default(),
                    crop,
                    2,
                    false,
                    &[],
                    false,
                )?;
                image::save_buffer(
                    output.join(format!("{name}-{label}.jpg")),
                    &rgb.to_rgb8(),
                    rgb.width,
                    rgb.height,
                    image::ColorType::Rgb8,
                )?;
            }
        }
    }
    Ok(())
}
