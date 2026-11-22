//! CPU-only stage timing and viewable control comparisons. Run from repo root.
use greign_film::{Adjustments, ColorBand, Material, Rect, analyze, load_scan, render};
use std::{hint::black_box, path::Path, time::Instant};

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "exports/r7-render-fix/152A0123-after.jpg".into());
    let out = Path::new("exports/film-controls");
    std::fs::create_dir_all(out)?;
    for dimension in [960, 3840] {
        let start = Instant::now();
        let scan = load_scan(Path::new(&path), Some(dimension), true)?;
        println!(
            "load,{dimension},{:.3}",
            start.elapsed().as_secs_f64() * 1000.0
        );
        let start = Instant::now();
        let analysis = analyze(&[&scan], Some([0.82, 0.52, 0.30]), Rect::default())?;
        println!(
            "analysis,{dimension},{:.3}",
            start.elapsed().as_secs_f64() * 1000.0
        );
        let lighting = Adjustments {
            shadows: 60.0,
            highlights: -60.0,
            ..Adjustments::default()
        };
        let mixer = Adjustments {
            colors: [ColorBand {
                hue: 15.0,
                saturation: 20.0,
                lightness: 5.0,
            }; 8],
            ..Adjustments::default()
        };
        for (name, edits) in [
            ("baseline", Adjustments::default()),
            ("lighting", lighting),
            ("mixer", mixer),
            ("combined", lighting.combined(mixer)),
        ] {
            let mut image = render(
                &scan,
                Material::ColorNegative,
                Some(&analysis),
                edits,
                Rect::default(),
                0,
                false,
                &[],
                false,
            )?;
            for repeat in 0..7 {
                let start = Instant::now();
                image = black_box(render(
                    &scan,
                    Material::ColorNegative,
                    Some(&analysis),
                    edits,
                    Rect::default(),
                    0,
                    false,
                    &[],
                    false,
                )?);
                println!(
                    "render-{name}-{repeat},{dimension},{:.3}",
                    start.elapsed().as_secs_f64() * 1000.0
                );
            }
            let start = Instant::now();
            image::save_buffer(
                out.join(format!("{dimension}-{name}.png")),
                &image.to_rgb8(),
                image.width,
                image.height,
                image::ColorType::Rgb8,
            )?;
            println!(
                "encode-{name},{dimension},{:.3}",
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
    Ok(())
}
