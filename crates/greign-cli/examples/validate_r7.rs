//! Full-resolution R7 corpus exports, decoder digests, and real-image GPU parity.

use anyhow::Result;
use greign_raw::{RawDecoder, RawlerDecoder};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::Instant};

#[allow(clippy::too_many_lines)]
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 4,
        "usage: validate_r7 RAW_DIRECTORY EXPORT_DIRECTORY REPORT"
    );
    fs::create_dir_all(&args[2])?;
    let mut inputs: Vec<_> = fs::read_dir(&args[1])?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cr3")))
        .collect();
    inputs.sort();
    let mut runtime = greign_gpu::GpuRuntime::initialize(2 * 1024 * 1024 * 1024)?;
    let mut reports = Vec::new();
    for input in inputs {
        let stem = input.file_stem().unwrap().to_string_lossy();
        let decode_start = Instant::now();
        let raw = RawlerDecoder.mosaic(&input)?;
        let decode_ms = decode_start.elapsed().as_secs_f64() * 1000.0;
        let crop = raw
            .metadata
            .crop_area
            .or(raw.metadata.active_area)
            .expect("R7 crop");
        let mut digest = Sha256::new();
        for y in crop.y..crop.y + crop.height {
            let start = (y * raw.metadata.width + crop.x) as usize;
            let row: Vec<_> = raw.samples[start..start + crop.width as usize]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect();
            digest.update(row);
        }
        let started = Instant::now();
        let settings = greign_pipeline::QualitySettings::default();
        let (image, timing) = runtime.develop_quality_readback(&raw, settings)?;
        let render_ms = started.elapsed().as_secs_f64() * 1000.0;
        let output = Path::new(&args[2]).join(format!("{stem}-after.jpg"));
        let encode_start = Instant::now();
        greign_export::export_rgb8_jpeg_fast(
            image.width,
            image.height,
            &image.pixels,
            &output,
            95,
        )?;
        let encode_ms = encode_start.elapsed().as_secs_f64() * 1000.0;
        let parity = if stem == "152A9949" || stem == "152A9971" {
            let cpu = greign_pipeline::develop_quality(&raw, settings)?;
            let bytes = cpu.to_rgb8();
            let mut max = 0_u8;
            let mut squared = 0.0;
            let mut over_two = 0_u32;
            for (a, b) in bytes.iter().zip(&image.pixels) {
                let d = a.abs_diff(*b);
                max = max.max(d);
                squared += f64::from(d).powi(2);
                over_two += u32::from(d > 2);
            }
            if max > 2 {
                for (i, (a, b)) in bytes
                    .iter()
                    .zip(&image.pixels)
                    .enumerate()
                    .filter(|(_, (a, b))| a.abs_diff(**b) > 2)
                    .take(16)
                {
                    eprintln!(
                        "parity outlier x={} y={} channel={} cpu={} gpu={}",
                        (i / 3) % image.width as usize,
                        (i / 3) / image.width as usize,
                        i % 3,
                        a,
                        b
                    );
                }
            }
            let cpu_output = Path::new(&args[2]).join(format!("{stem}-after-cpu.jpg"));
            greign_export::export_rgb8_jpeg_fast(cpu.width, cpu.height, &bytes, &cpu_output, 95)?;
            #[allow(clippy::cast_precision_loss)]
            let rmse = (squared / bytes.len() as f64).sqrt();
            anyhow::ensure!(
                max <= 2,
                "{stem}: GPU parity failed, maximum error {max}, over-two {over_two}"
            );
            Some(
                serde_json::json!({"max_byte_difference":max,"rmse":rmse,"bytes_over_two":over_two}),
            )
        } else {
            None
        };
        reports.push(serde_json::json!({
            "input":input,"output":output,"metadata":raw.metadata,
            "crop_sha256_le_u16":format!("{:x}",digest.finalize()),
            "decode_ms":decode_ms,"gpu_render_ms":render_ms,"jpeg_ms":encode_ms,
            "highlight_analysis_ms":timing.highlight_analysis_ms,
            "gpu_sensor_ms":timing.sensor_ms,"gpu_demosaic_ms":timing.demosaic_ms,
            "gpu_color_ms":timing.color_ms,"gpu_output_ms":timing.output_ms,"parity":parity,
        }));
        fs::write(&args[3], serde_json::to_vec_pretty(&reports)?)?;
        println!("Exported {stem}, GPU render {render_ms:.1} ms");
    }
    Ok(())
}
