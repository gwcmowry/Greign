//! Small viewable metadata fixtures, with no writes to original photographs.
use greign_export::{
    CaptureDate, ExportOptions, export_linear_dng_dated, export_rgb8_jpeg_fast_dated,
    export_srgb_dated,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::Path::new("exports/capture-date-check");
    std::fs::create_dir_all(directory)?;
    let image = greign_pipeline::SrgbImage {
        width: 64,
        height: 48,
        pixels: (0..64 * 48)
            .map(|i| [f32::from(u8::try_from(i % 64).unwrap()) / 64., 0.4, 0.6])
            .collect(),
    };
    let date = CaptureDate::parse("1998-07-03")
        .map_err(std::io::Error::other)?
        .unwrap();
    export_rgb8_jpeg_fast_dated(
        image.width,
        image.height,
        &image.to_rgb8(),
        &directory.join("dated.jpg"),
        95,
        Some(&date),
    )?;
    export_srgb_dated(
        &image,
        &directory.join("dated.tif"),
        ExportOptions::default(),
        Some(&date),
    )?;
    export_linear_dng_dated(
        &greign_pipeline::LinearRgbImage {
            width: 64,
            height: 48,
            pixels: vec![[0.2, 0.3, 1.4]; 64 * 48],
        },
        &directory.join("dated.dng"),
        Some(&date),
    )?;
    Ok(())
}
