//! Opt-in hardware check: production export jobs, isolated recipes, visible files.
use crate::{
    jobs,
    model::{Frame, Roll},
};
use anyhow::{Result, ensure};
use eframe::egui;
use greign_film::{self as film, Adjustments, Material, Rect};
use greign_gl_preview::{Preview, Settings};
use std::{
    io::Write,
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

pub fn run(
    renderer: &Preview,
    ctx: &egui::Context,
    root: &Path,
    jpeg: bool,
    long_edge: u32,
) -> Result<()> {
    let directory = root
        .join(if jpeg {
            "exports/jpeg-export"
        } else {
            "exports/gpu-export"
        })
        .join(crate::model::id());
    std::fs::create_dir_all(&directory)?;
    let mut log = std::fs::File::create(directory.join("check.csv"))?;
    println!("Export validation: {}", directory.display());
    writeln!(log, "directory,{}", directory.display())?;
    writeln!(log, "long-edge,{long_edge}")?;
    let edits = Adjustments {
        brightness: 0.3,
        contrast: 15.,
        shadows: 60.,
        highlights: -45.,
        warmth: 8.,
        tint: -4.,
        saturation: 12.,
        colors: std::array::from_fn(|i| film::ColorBand {
            hue: if i % 2 == 0 { 12. } else { -15. },
            saturation: 18.,
            lightness: -5.,
        }),
    };
    // Direct float comparisons cover values a display-oriented RGB8 test hides.
    for material in [
        Material::Positive,
        Material::ColorNegative,
        Material::BlackAndWhite,
    ] {
        let scan = film::Scan {
            image: greign_pipeline::LinearRgbImage {
                width: 128,
                height: 96,
                pixels: (0..128 * 96)
                    .map(|i| {
                        if i % 11 == 0 {
                            [0.18; 3]
                        } else {
                            [
                                (i % 127) as f32 / 50. - 0.05,
                                (i % 89) as f32 / 80.,
                                (i % 61) as f32 / 60.,
                            ]
                        }
                    })
                    .collect(),
            },
            camera_matrix: None,
            white_balance: [1.; 3],
            capture: "export float fixture".into(),
        };
        let analysis = film::analyze(&[&scan], Some([0.8; 3]), Rect::default())?;
        let mut prepared = film::prepare(&scan, material, Some(&analysis), false, &|| false)?;
        // Deliberately signed/HDR working-space data tests the finishing stage.
        prepared.image = scan.image;
        for rotation in 0..4 {
            let crop = Rect {
                x: 0.127,
                y: 0.137,
                w: 0.753,
                h: 0.751,
            };
            let cpu = film::finish(&prepared, edits, crop, rotation, &[], &|| false)?;
            let gpu = renderer.export_source(
                &prepared.image,
                Settings {
                    edits,
                    material,
                    crop,
                    rotation,
                },
            )?;
            ensure!(
                (cpu.width, cpu.height) == (gpu.width, gpu.height),
                "Float export geometry mismatch"
            );
            let mut maximum = 0_f32;
            for (a, b) in cpu.pixels.iter().flatten().zip(gpu.pixels.iter().flatten()) {
                maximum = maximum.max((a - b).abs());
                ensure!(
                    (a - b).abs() <= 0.00001 + a.abs() * 0.00002,
                    "Float parity failed: {material:?}/{rotation}: {a} vs {b}"
                );
            }
            writeln!(log, "float-parity,{material:?},{rotation},{maximum}")?;
            if jpeg {
                let actual = renderer.export_jpeg_source(
                    &prepared.image,
                    Settings {
                        edits,
                        material,
                        crop,
                        rotation,
                    },
                )?;
                let reference = greign_pipeline::rec2020_to_srgb(cpu.clone());
                let expected = reference.to_rgb8();
                let maximum = actual
                    .as_raw()
                    .iter()
                    .zip(&expected)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or(0);
                ensure!(maximum <= 1, "Pre-codec JPEG RGB8 parity failed: {maximum}");
                writeln!(log, "jpeg-rgb8-parity,{material:?},{rotation},{maximum}")?;
                // Resized exports retain float sRGB until final quantization.
                let gpu_srgb = renderer.export_srgb_source(
                    &prepared.image,
                    Settings {
                        edits,
                        material,
                        crop,
                        rotation,
                    },
                )?;
                let gpu_float = image::Rgb32FImage::from_raw(
                    gpu_srgb.width,
                    gpu_srgb.height,
                    gpu_srgb.pixels.iter().flatten().copied().collect(),
                )
                .unwrap();
                let gpu_resized = image::imageops::resize(
                    &gpu_float,
                    31,
                    23,
                    image::imageops::FilterType::Lanczos3,
                );
                let compact = greign_pipeline::SrgbImage {
                    width: 31,
                    height: 23,
                    pixels: gpu_resized.pixels().map(|p| p.0).collect(),
                }
                .to_rgb8();
                let floats = image::Rgb32FImage::from_raw(
                    reference.width,
                    reference.height,
                    reference.pixels.iter().flatten().copied().collect(),
                )
                .unwrap();
                let resized =
                    image::imageops::resize(&floats, 31, 23, image::imageops::FilterType::Lanczos3);
                let expected = greign_pipeline::SrgbImage {
                    width: 31,
                    height: 23,
                    pixels: resized.pixels().map(|p| p.0).collect(),
                }
                .to_rgb8();
                let maximum = compact
                    .iter()
                    .zip(&expected)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or(0);
                ensure!(maximum <= 1, "Resized JPEG RGB8 parity failed: {maximum}");
                writeln!(log, "jpeg-resize-parity,{material:?},{rotation},{maximum}")?;
            }
            if material == Material::Positive && rotation == 1 {
                // Exercise the linear DNG writer with signed/HDR float output,
                // then independently read its strips rather than trusting the
                // in-memory render comparison alone.
                for (name, image) in [("cpu", &cpu), ("gpu", &gpu)] {
                    let path = directory.join(format!("{name}-float-fixture.dng"));
                    greign_export::export_linear_dng(image, &path)?;
                    let decoded = dng_samples(&path)?;
                    ensure!(
                        decoded.iter().copied().eq(image
                            .pixels
                            .iter()
                            .flatten()
                            .map(|v| v.max(0.0))),
                        "DNG did not preserve float samples"
                    );
                }
                writeln!(log, "dng-float-roundtrip,exact")?;
            }
        }
    }
    // Two real full-resolution camera files, not a preview texture or synthetic RAW.
    let mut roll = Roll::new("GPU export validation");
    roll.material = Material::Positive;
    roll.adjustments = edits;
    roll.frames = ["152A0123.CR3", "152A0325.CR3"]
        .into_iter()
        .map(|name| Frame::new(root.join("raw").join(name)))
        .collect();
    roll.export.tiff = !jpeg;
    roll.export.long_edge = if jpeg { long_edge } else { 0 };
    for (backend, mode) in [
        ("serial-cpu", jobs::ExportMode::SerialCpu),
        ("gpu", jobs::ExportMode::Gpu),
        ("cpu", jobs::ExportMode::Cpu),
    ] {
        let output = directory.join(backend);
        std::fs::create_dir_all(&output)?;
        let profile = output.join("stages.jsonl");
        let (tx, rx) = mpsc::channel();
        let start = Instant::now();
        jobs::export_with_mode(
            roll.clone(),
            output.clone(),
            None,
            tx,
            ctx.clone(),
            profile,
            mode,
        );
        loop {
            match rx.recv_timeout(Duration::from_secs(120))? {
                jobs::Event::JpegFloatRender {
                    prepared,
                    settings,
                    reply,
                } => {
                    let start = Instant::now();
                    let result = renderer.export_srgb_source(&prepared.image, settings);
                    writeln!(
                        log,
                        "gpu-jpeg-float-upload-draw-readback-ms,{:.3}",
                        start.elapsed().as_secs_f64() * 1000.
                    )?;
                    let error = result.as_ref().err().map(ToString::to_string);
                    let _ = reply.send(result.map_err(|e| e.to_string()));
                    ensure!(error.is_none(), "GPU float JPEG error: {error:?}");
                }
                jobs::Event::JpegRender {
                    prepared,
                    settings,
                    reply,
                } => {
                    let start = Instant::now();
                    let result = renderer.export_jpeg_source(&prepared.image, settings);
                    writeln!(
                        log,
                        "gpu-jpeg-upload-draw-readback-ms,{:.3}",
                        start.elapsed().as_secs_f64() * 1000.
                    )?;
                    let error = result.as_ref().err().map(ToString::to_string);
                    let _ = reply.send(result.map_err(|e| e.to_string()));
                    ensure!(error.is_none(), "GPU JPEG export error: {error:?}");
                }
                jobs::Event::ExportRender {
                    prepared,
                    settings,
                    reply,
                } => {
                    let start = Instant::now();
                    let result = renderer.export_source(&prepared.image, settings);
                    writeln!(
                        log,
                        "gpu-upload-draw-readback-ms,{:.3}",
                        start.elapsed().as_secs_f64() * 1000.
                    )?;
                    // Hardware validation must not silently pass via CPU fallback.
                    let error = result.as_ref().err().map(ToString::to_string);
                    let _ = reply.send(result.map_err(|e| e.to_string()));
                    ensure!(error.is_none(), "GPU export error: {error:?}");
                }
                jobs::Event::Exported(_, failures, _) => {
                    ensure!(failures.is_empty(), "Export failed: {failures:?}");
                }
                jobs::Event::Finished => break,
                _ => {}
            }
        }
        writeln!(
            log,
            "batch-ms,{backend},{:.3}",
            start.elapsed().as_secs_f64() * 1000.
        )?;
        log.flush()?;
    }
    for file in std::fs::read_dir(directory.join("cpu"))? {
        let file = file?;
        if file
            .path()
            .extension()
            .is_none_or(|e| e != if jpeg { "jpg" } else { "tif" })
        {
            continue;
        }
        let cpu = image::open(file.path())?.to_rgb16();
        let gpu = image::open(directory.join("gpu").join(file.file_name()))?.to_rgb16();
        ensure!(
            cpu.dimensions() == gpu.dimensions(),
            "TIFF dimensions differ"
        );
        let mut sum = 0_u64;
        let mut max = 0_u16;
        for (a, b) in cpu.as_raw().iter().zip(gpu.as_raw()) {
            let difference = a.abs_diff(*b);
            sum += u64::from(difference);
            max = max.max(difference);
        }
        let mean = sum as f64 / cpu.as_raw().len() as f64;
        writeln!(
            log,
            "tiff-rgb16-parity,{},{mean},{max}",
            file.file_name().to_string_lossy()
        )?;
        log.flush()?;
        ensure!(
            mean < if jpeg { 25.7 } else { 0.2 } && max <= if jpeg { 2056 } else { 8 },
            "TIFF parity failed: mean {mean}, max {max}"
        );
    }
    Ok(())
}

// Our DNG writer emits classic TIFF with uncompressed float strips. Generic
// image decoders reject DNG's LinearRaw photometric tag, so read the strip
// directory directly without modifying or relabeling the exported file.
fn dng_samples(path: &Path) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path)?;
    let little = &bytes[..2] == b"II";
    let short = |b: &[u8]| {
        if little {
            u16::from_le_bytes(b[..2].try_into().unwrap())
        } else {
            u16::from_be_bytes(b[..2].try_into().unwrap())
        }
    };
    let long = |b: &[u8]| {
        if little {
            u32::from_le_bytes(b[..4].try_into().unwrap())
        } else {
            u32::from_be_bytes(b[..4].try_into().unwrap())
        }
    };
    ensure!(short(&bytes[2..]) == 42, "Expected classic TIFF");
    let ifd = long(&bytes[4..]) as usize;
    let mut offsets = Vec::new();
    let mut counts = Vec::new();
    for i in 0..usize::from(short(&bytes[ifd..])) {
        let entry = &bytes[ifd + 2 + i * 12..][..12];
        let tag = short(entry);
        if tag != 273 && tag != 279 {
            continue;
        }
        ensure!(short(&entry[2..]) == 4, "Expected LONG strip directory");
        let count = long(&entry[4..]) as usize;
        let value = if count == 1 {
            &entry[8..12]
        } else {
            let start = long(&entry[8..]) as usize;
            &bytes[start..start + count * 4]
        };
        let values = value.chunks_exact(4).map(long).collect::<Vec<_>>();
        if tag == 273 {
            offsets = values;
        } else {
            counts = values;
        }
    }
    ensure!(
        !offsets.is_empty() && offsets.len() == counts.len(),
        "Invalid strips"
    );
    let mut samples = Vec::new();
    for (offset, count) in offsets.into_iter().zip(counts) {
        samples.extend(
            bytes[offset as usize..(offset + count) as usize]
                .chunks_exact(4)
                .map(|b| f32::from_bits(long(b))),
        );
    }
    Ok(samples)
}
