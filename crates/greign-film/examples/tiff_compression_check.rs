//! CPU-only TIFF compression timing and exact round-trip check on a real export.
use std::{path::PathBuf, time::Instant};
fn main() -> anyhow::Result<()> {
    let source = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("Pass an existing 16-bit Greign TIFF export"))?,
    );
    let start = Instant::now();
    let original = image::open(&source)?.to_rgb16();
    println!("decode_ms,{:.3}", start.elapsed().as_secs_f64() * 1000.);
    let input = greign_pipeline::SrgbImage {
        width: original.width(),
        height: original.height(),
        pixels: original
            .pixels()
            .map(|p| p.0.map(|v| f32::from(v) / 65535.))
            .collect(),
    };
    let directory = PathBuf::from("exports/tiff-compression-check");
    std::fs::create_dir_all(&directory)?;
    let output = directory.join("compressed-positive.tif");
    let start = Instant::now();
    greign_export::export_srgb(&input, &output, greign_export::ExportOptions::default())?;
    println!("encode_zip_ms,{:.3}", start.elapsed().as_secs_f64() * 1000.);
    let start = Instant::now();
    let decoded = image::open(&output)?.to_rgb16();
    println!("decode_zip_ms,{:.3}", start.elapsed().as_secs_f64() * 1000.);
    anyhow::ensure!(original == decoded, "Compressed TIFF pixel values differ");
    println!("original_bytes,{}", std::fs::metadata(source)?.len());
    println!("compressed_bytes,{}", std::fs::metadata(output)?.len());
    println!("roundtrip,exact");
    Ok(())
}
