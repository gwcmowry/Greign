//! Real-context GPU timing, CPU parity and visible proof outputs. Opt-in only.
use super::*;
use std::{io::Write, path::Path, time::Instant};

pub fn run(gl: Arc<glow::Context>, root: &Path) -> Result<()> {
    let directory = root.join("exports/gpu-preview");
    std::fs::create_dir_all(&directory)?;
    let artifacts = root.join("artifacts");
    std::fs::create_dir_all(&artifacts)?;
    let mut log = std::fs::File::create(artifacts.join("gpu-preview-benchmark.csv"))?;
    let mut preview = Preview::new(gl.clone())?;
    // SAFETY: eframe's creation callback owns the current GL context.
    unsafe {
        writeln!(log, "renderer,{}", gl.get_parameter_string(glow::RENDERER))?;
        writeln!(log, "version,{}", gl.get_parameter_string(glow::VERSION))?;
        gl.disable(glow::SCISSOR_TEST);
        gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
    }
    let path = root.join("exports/r7-render-fix/152A0123-after.jpg");
    let scan = greign_film::load_scan(&path, None, true)?;
    let analysis = greign_film::analyze(&[&scan], Some([0.82, 0.52, 0.30]), Rect::default())?;
    let prepared = greign_film::prepare(
        &scan,
        Material::ColorNegative,
        Some(&analysis),
        false,
        &|| false,
    )?;
    let start = Instant::now();
    preview.upload("benchmark".into(), &prepared.image)?;
    unsafe {
        gl.finish();
    }
    writeln!(
        log,
        "upload_with_mipmaps_ms,{:.6}",
        start.elapsed().as_secs_f64() * 1000.
    )?;
    let mut edits = Adjustments {
        brightness: 0.2,
        contrast: 12.,
        shadows: 60.,
        highlights: -50.,
        warmth: 8.,
        tint: -4.,
        saturation: 12.,
        ..Adjustments::default()
    };
    for (i, b) in edits.colors.iter_mut().enumerate() {
        b.hue = if i % 2 == 0 { 12. } else { -15. };
        b.saturation = 18.;
        b.lightness = -5.;
    }
    let settings = Settings {
        mirrored: false,
        edits,
        material: Material::ColorNegative,
        crop: Rect::default(),
        rotation: 0,
    };
    for (width, height) in [
        (960, 640),
        (1920, 1280),
        (3840, 2560),
        (scan.image.width, scan.image.height),
    ] {
        let target = Target::new(&gl, width, height)?;
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.fbo));
            gl.viewport(0, 0, width as i32, height as i32);
        }
        let mut frame_settings = settings;
        for repeat in 0..33 {
            frame_settings.edits.brightness =
                settings.edits.brightness + (repeat as f32 * 0.2).sin() * 0.1;
            let query = unsafe { gl.create_query().map_err(anyhow::Error::msg)? };
            let start = Instant::now();
            unsafe {
                gl.begin_query(glow::TIME_ELAPSED, query);
            }
            preview.draw(frame_settings);
            unsafe {
                gl.end_query(glow::TIME_ELAPSED);
                gl.finish();
            }
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            let nanos = unsafe { gl.get_query_parameter_u32(query, glow::QUERY_RESULT) };
            unsafe {
                gl.delete_query(query);
            }
            if repeat >= 3 {
                writeln!(
                    log,
                    "render,{width},{height},{repeat},{:.6},{elapsed:.6}",
                    f64::from(nanos) / 1e6
                )?;
            }
        }
        let bytes = target.read(&gl)?;
        image::save_buffer(
            directory.join(format!("gpu-{width}.png")),
            &bytes,
            width,
            height,
            image::ColorType::Rgba8,
        )?;
        if width == scan.image.width {
            let start = Instant::now();
            let cpu = greign_pipeline::rec2020_to_srgb(greign_film::finish(
                &prepared,
                frame_settings.edits,
                Rect::default(),
                0,
                false,
                &[],
                &|| false,
            )?);
            let cpu = cpu.to_rgb8();
            writeln!(
                log,
                "cpu_full_render_ms,{:.6}",
                start.elapsed().as_secs_f64() * 1000.
            )?;
            let (mean, max) = difference(&bytes, &cpu);
            writeln!(log, "parity-photo,{mean:.6},{max}")?;
            log.flush()?;
            ensure!(
                mean < 0.1 && max <= 3,
                "Full photo GPU parity failed: mean {mean}, max {max}"
            );
            image::save_buffer(
                directory.join("cpu-full-reference.png"),
                &cpu,
                width,
                height,
                image::ColorType::Rgb8,
            )?;
        }
        target.destroy(&gl);
    }
    // Include strong controls, signed values, neutral pixels, crop and rotations.
    let synthetic = greign_film::Scan {
        image: LinearRgbImage {
            width: 128,
            height: 96,
            pixels: (0..128 * 96)
                .map(|i| {
                    if i % 11 == 0 {
                        [0.18; 3]
                    } else {
                        [
                            (i % 127) as f32 / 100. - 0.05,
                            (i % 89) as f32 / 80.,
                            (i % 61) as f32 / 60.,
                        ]
                    }
                })
                .collect(),
        },
        camera_matrix: None,
        white_balance: [1.; 3],
        capture: "GPU parity fixture".into(),
    };
    for material in [
        Material::Positive,
        Material::BlackAndWhite,
        Material::ColorNegative,
    ] {
        let base = greign_film::prepare(&synthetic, material, Some(&analysis), false, &|| false)?;
        preview.upload("parity".into(), &base.image)?;
        for (rotation, angle) in
            (0..4).flat_map(|rotation| [0., -2.3, 3.1].map(move |angle| (rotation, angle)))
        {
            for mirrored in [false, true] {
                let crop = Rect {
                    x: 0.127,
                    y: 0.137,
                    w: 0.753,
                    h: 0.751,
                    angle,
                };
                let settings = Settings {
                    mirrored,
                    edits,
                    material,
                    crop,
                    rotation,
                };
                let cpu = greign_pipeline::rec2020_to_srgb(greign_film::finish(
                    &base,
                    edits,
                    crop,
                    rotation,
                    mirrored,
                    &[],
                    &|| false,
                )?);
                let target = Target::new(&gl, cpu.width, cpu.height)?;
                unsafe {
                    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.fbo));
                    gl.viewport(0, 0, cpu.width as i32, cpu.height as i32);
                }
                preview.draw(settings);
                let gpu = target.read(&gl)?;
                target.destroy(&gl);
                let cpu = cpu.to_rgb8();
                let (mean, max) = difference(&gpu, &cpu);
                writeln!(
                    log,
                    "parity,{material:?},{rotation},{mirrored},{angle},{mean:.6},{max}"
                )?;
                log.flush()?;
                ensure!(
                    mean < 0.1 && max <= 3,
                    "GPU parity failed for {material:?}, rotation {rotation}: mean {mean}, max {max} RGB8 levels"
                );
            }
        }
        let window = [0.25, 0.25, 0.5, 0.5];
        let crop = Rect {
            x: window[0],
            y: window[1],
            w: window[2],
            h: window[3],
            angle: 0.0,
        };
        let cpu = greign_pipeline::rec2020_to_srgb(greign_film::finish(
            &base,
            edits,
            crop,
            0,
            false,
            &[],
            &|| false,
        )?);
        let target = Target::new(&gl, cpu.width, cpu.height)?;
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.fbo));
            gl.viewport(0, 0, cpu.width as i32, cpu.height as i32);
        }
        preview.draw_window(
            Settings {
                mirrored: false,
                edits,
                material,
                crop: Rect::default(),
                rotation: 0,
            },
            window,
        );
        let gpu = target.read(&gl)?;
        target.destroy(&gl);
        let (mean, max) = difference(&gpu, &cpu.to_rgb8());
        writeln!(log, "parity-clipped-view,{material:?},{mean:.6},{max}")?;
        log.flush()?;
        ensure!(
            mean < 0.1 && max <= 3,
            "Clipped viewport parity failed: {mean}, {max}"
        );
    }
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
    }
    preview.destroy();
    log.flush()?;
    Ok(())
}
fn difference(gpu: &[u8], cpu: &[u8]) -> (f64, u8) {
    assert_eq!(gpu.len() / 4, cpu.len() / 3);
    let mut sum = 0_u64;
    let mut maximum = 0_u8;
    for (a, b) in gpu.chunks_exact(4).zip(cpu.chunks_exact(3)) {
        for c in 0..3 {
            let delta = a[c].abs_diff(b[c]);
            sum += u64::from(delta);
            maximum = maximum.max(delta);
        }
    }
    (sum as f64 / cpu.len() as f64, maximum)
}
struct Target {
    fbo: glow::Framebuffer,
    texture: glow::Texture,
    width: u32,
    height: u32,
}
impl Target {
    fn new(gl: &glow::Context, width: u32, height: u32) -> Result<Self> {
        // SAFETY: only used by run() on the current-context thread.
        unsafe {
            let texture = gl.create_texture().map_err(anyhow::Error::msg)?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            let fbo = gl.create_framebuffer().map_err(anyhow::Error::msg)?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            ensure!(
                gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE,
                "GPU benchmark target incomplete"
            );
            Ok(Self {
                fbo,
                texture,
                width,
                height,
            })
        }
    }
    fn read(&self, gl: &glow::Context) -> Result<Vec<u8>> {
        let stride = self.width as usize * 4;
        let mut bytes = vec![0_u8; stride * self.height as usize];
        unsafe {
            gl.read_pixels(
                0,
                0,
                self.width as i32,
                self.height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut bytes)),
            );
        }
        let mut top = vec![0_u8; bytes.len()];
        for (row, out) in top.chunks_exact_mut(stride).enumerate() {
            let start = (self.height as usize - 1 - row) * stride;
            out.copy_from_slice(&bytes[start..start + stride]);
        }
        Ok(top)
    }
    fn destroy(self, gl: &glow::Context) {
        unsafe {
            gl.delete_framebuffer(self.fbo);
            gl.delete_texture(self.texture);
        }
    }
}
