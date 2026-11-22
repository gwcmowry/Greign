//! Opt-in private-photo orientation regression and CPU timing. No uploads.
use anyhow::{Result, ensure};
use greign_film::UprightDetector;
use image::imageops;
use std::{io::Write, path::PathBuf, time::Instant};

fn oriented(image: &image::RgbImage, turn: u8) -> image::RgbImage {
    match turn {
        1 => imageops::rotate90(image),
        2 => imageops::rotate180(image),
        3 => imageops::rotate270(image),
        _ => image.clone(),
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let input = PathBuf::from(args.next().expect("positive preview directory"));
    let output = PathBuf::from(args.next().expect("output directory"));
    let verify_portra = args.next().is_some_and(|arg| arg == "--verify-portra");
    let backend = args.next();
    std::fs::create_dir_all(&output)?;
    let mut log = std::fs::File::create(output.join("upright.jsonl"))?;
    let start = Instant::now();
    let detector = match backend.as_deref() {
        Some("gpu") => UprightDetector::with_cuda(&PathBuf::from(
            std::env::var_os("GREIGN_UPRIGHT_GPU_LIBRARY").expect("GPU runtime library path"),
        ))?,
        Some(workers) => UprightDetector::with_workers(workers.parse()?)?,
        None => UprightDetector::new()?,
    };
    writeln!(
        log,
        "{}",
        serde_json::json!({"stage":"model-load","backend":detector.backend(),"ms":start.elapsed().as_secs_f64()*1000.})
    )?;
    let mut paths: Vec<_> = std::fs::read_dir(input)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("-individual.jpg")
        })
        .collect();
    paths.sort();
    let mut checked = 0;
    let mut decided = 0;
    ensure!(!paths.is_empty(), "No individual positive previews found");
    for path in paths {
        let name = path.file_stem().unwrap().to_string_lossy();
        let image = image::open(&path)?.to_rgb8();
        let expected = if name.starts_with("IMG_0011") {
            None
        } else if name.starts_with("IMG_0008") {
            Some(1_u8)
        } else {
            Some(0)
        };
        for turn in 0..4_u8 {
            let rotated = oriented(&image, turn);
            for mirrored in [false, true] {
                let variant = if mirrored {
                    imageops::flip_horizontal(&rotated)
                } else {
                    rotated.clone()
                };
                let start = Instant::now();
                let guess = detector.detect(&variant)?;
                let expected = expected.map(|base| {
                    if mirrored {
                        (turn + 4 - base) % 4
                    } else {
                        (base + 4 - turn) % 4
                    }
                });
                writeln!(
                    log,
                    "{}",
                    serde_json::json!({"file":name,"input_turn":turn,"mirrored":mirrored,"ms":start.elapsed().as_secs_f64()*1000.,"expected":expected,"guess":guess})
                )?;
                log.flush()?;
                decided += usize::from(guess.rotation.is_some());
                if verify_portra {
                    ensure!(
                        guess
                            .rotation
                            .is_none_or(|rotation| Some(rotation) == expected),
                        "Orientation mismatch {name}, input turn {turn}, mirrored {mirrored}: {guess:?}, expected {expected:?}"
                    );
                }
                if turn == 0
                    && !mirrored
                    && let Some(correction) = guess.rotation
                {
                    let upright = oriented(&image, correction);
                    upright.save(output.join(format!("{name}-upright.jpg")))?;
                }
                checked += 1;
            }
        }
    }
    if let Some(profile) = detector.finish_profile()? {
        writeln!(
            log,
            "{}",
            serde_json::json!({"stage":"cuda-profile","path":profile})
        )?;
    }
    println!(
        "Checked {checked} image variants: {decided} confident, {} left for review; details in {}",
        checked - decided,
        output.join("upright.jsonl").display()
    );
    Ok(())
}
