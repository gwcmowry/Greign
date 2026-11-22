//! Precision-specific export targets on the native context: linear floats for
//! TIFF/DNG finishing, sRGB floats for JPEG resizing, RGB8 for full-size JPEG.
//! No preview-sized buffers are used; unsupported edits stay on the CPU.
use super::*;

enum Output {
    Linear(LinearRgbImage),
    Srgb(greign_pipeline::SrgbImage),
    Jpeg(image::RgbImage),
}

impl Preview {
    /// Render a prepared full-resolution source without disturbing the preview
    /// texture. Must be called on the current native GL context's owning thread.
    pub fn export_source(
        &self,
        image: &LinearRgbImage,
        settings: Settings,
    ) -> Result<LinearRgbImage> {
        match self.export_pixels(image, settings, 1)? {
            Output::Linear(image) => Ok(image),
            _ => unreachable!(),
        }
    }
    /// JPEG-specific finishing: sRGB conversion and 8-bit quantization on GPU,
    /// reducing readback to three bytes per pixel without changing JPEG quality.
    pub fn export_jpeg_source(
        &self,
        image: &LinearRgbImage,
        settings: Settings,
    ) -> Result<image::RgbImage> {
        match self.export_pixels(image, settings, 2)? {
            Output::Jpeg(image) => Ok(image),
            _ => unreachable!(),
        }
    }
    /// Keep floating-point sRGB through resizing; quantize only at final size.
    pub fn export_srgb_source(
        &self,
        image: &LinearRgbImage,
        settings: Settings,
    ) -> Result<greign_pipeline::SrgbImage> {
        match self.export_pixels(image, settings, 3)? {
            Output::Srgb(image) => Ok(image),
            _ => unreachable!(),
        }
    }
    fn export_pixels(
        &self,
        image: &LinearRgbImage,
        settings: Settings,
        mode: i32,
    ) -> Result<Output> {
        let jpeg = mode == 2;
        self.check_thread();
        ensure!(
            self.alive && settings.crop.valid(),
            "Invalid export state/crop"
        );
        let (width, height) =
            dimensions(image.width, image.height, settings.crop, settings.rotation)?;
        let gl = &self.gl;
        // SAFETY: the context is current on its owner; all slices are sized to
        // the framebuffer. Resources and framebuffer/viewport state are restored
        // on both success and ordinary errors before returning to egui.
        unsafe {
            let draw = gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING);
            let read = gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING);
            let mut viewport = [0; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
            let scissor = gl.is_enabled(glow::SCISSOR_TEST);
            let dither = gl.is_enabled(glow::DITHER);
            let pack = gl.get_parameter_i32(glow::PACK_ALIGNMENT);
            let row_length = gl.get_parameter_i32(glow::PACK_ROW_LENGTH);
            let skip_rows = gl.get_parameter_i32(glow::PACK_SKIP_ROWS);
            let skip_pixels = gl.get_parameter_i32(glow::PACK_SKIP_PIXELS);
            let mut renderer = None;
            let mut target = None;
            let mut framebuffer = None;
            let result = (|| -> Result<Output> {
                renderer = Some(Preview::new(gl.clone())?);
                let renderer = renderer.as_mut().unwrap();
                renderer.upload_inner("export".into(), image, false)?;
                let texture = gl.create_texture().map_err(anyhow::Error::msg)?;
                target = Some(texture);
                gl.bind_texture(glow::TEXTURE_2D, target);
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    if jpeg { glow::RGBA8 } else { glow::RGBA32F } as i32,
                    i32::try_from(width)?,
                    i32::try_from(height)?,
                    0,
                    glow::RGBA,
                    glow::FLOAT,
                    glow::PixelUnpackData::Slice(None),
                );
                let fbo = gl.create_framebuffer().map_err(anyhow::Error::msg)?;
                framebuffer = Some(fbo);
                gl.bind_framebuffer(glow::FRAMEBUFFER, framebuffer);
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    target,
                    0,
                );
                ensure!(
                    gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE,
                    "GPU export framebuffer incomplete"
                );
                gl.disable(glow::SCISSOR_TEST);
                gl.disable(glow::DITHER);
                gl.viewport(0, 0, width as i32, height as i32);
                renderer.draw_mode(settings, [0., 0., 1., 1.], mode);
                gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
                gl.pixel_store_i32(glow::PACK_ROW_LENGTH, 0);
                gl.pixel_store_i32(glow::PACK_SKIP_ROWS, 0);
                gl.pixel_store_i32(glow::PACK_SKIP_PIXELS, 0);
                if jpeg {
                    let mut bytes = vec![0_u8; width as usize * height as usize * 3];
                    gl.read_pixels(
                        0,
                        0,
                        width as i32,
                        height as i32,
                        glow::RGB,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut bytes)),
                    );
                    let error = gl.get_error();
                    ensure!(
                        error == glow::NO_ERROR,
                        "GPU JPEG export failed: GL {error:#x}"
                    );
                    let mut image = image::RgbImage::from_raw(width, height, bytes)
                        .ok_or_else(|| anyhow::anyhow!("Invalid JPEG readback dimensions"))?;
                    image::imageops::flip_vertical_in_place(&mut image);
                    return Ok(Output::Jpeg(image));
                }
                let mut pixels = vec![[0_f32; 3]; width as usize * height as usize];
                gl.read_pixels(
                    0,
                    0,
                    width as i32,
                    height as i32,
                    glow::RGB,
                    glow::FLOAT,
                    glow::PixelPackData::Slice(Some(bytemuck::cast_slice_mut(&mut pixels))),
                );
                let error = gl.get_error();
                ensure!(error == glow::NO_ERROR, "GPU export failed: GL {error:#x}");
                ensure!(
                    pixels.iter().flatten().all(|x| x.is_finite()),
                    "Non-finite GPU export"
                );
                // GL rows begin at the bottom; CPU image rows begin at the top.
                for y in 0..height as usize / 2 {
                    let (top, bottom) =
                        pixels.split_at_mut((height as usize - 1 - y) * width as usize);
                    top[y * width as usize..(y + 1) * width as usize]
                        .swap_with_slice(&mut bottom[..width as usize]);
                }
                if mode == 3 {
                    return Ok(Output::Srgb(greign_pipeline::SrgbImage {
                        width,
                        height,
                        pixels,
                    }));
                }
                Ok(Output::Linear(LinearRgbImage {
                    width,
                    height,
                    pixels,
                }))
            })();
            if let Some(mut renderer) = renderer {
                renderer.destroy();
            }
            if let Some(fbo) = framebuffer {
                gl.delete_framebuffer(fbo);
            }
            if let Some(texture) = target {
                gl.delete_texture(texture);
            }
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, draw);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, read);
            gl.viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
            if scissor {
                gl.enable(glow::SCISSOR_TEST);
            }
            if dither {
                gl.enable(glow::DITHER);
            }
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, pack);
            gl.pixel_store_i32(glow::PACK_ROW_LENGTH, row_length);
            gl.pixel_store_i32(glow::PACK_SKIP_ROWS, skip_rows);
            gl.pixel_store_i32(glow::PACK_SKIP_PIXELS, skip_pixels);
            result
        }
    }
}

fn dimensions(width: u32, height: u32, crop: Rect, rotation: u8) -> Result<(u32, u32)> {
    ensure!(crop.valid_for(width, height), "Invalid export dimensions");
    let x = (crop.x * width as f32) as u32;
    let y = (crop.y * height as f32) as u32;
    ensure!(x < width && y < height, "Export crop has no source pixels");
    let w = ((crop.w * width as f32) as u32).max(1).min(width - x);
    let h = ((crop.h * height as f32) as u32).max(1).min(height - y);
    Ok(if rotation.is_multiple_of(2) {
        (w, h)
    } else {
        (h, w)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_dimensions_match_integer_crop_and_rotation() {
        let crop = Rect {
            x: 0.127,
            y: 0.137,
            w: 0.753,
            h: 0.751,
            angle: 0.0,
        };
        assert_eq!(dimensions(128, 96, crop, 0).unwrap(), (96, 72));
        assert_eq!(dimensions(128, 96, crop, 1).unwrap(), (72, 96));
        assert!(dimensions(0, 96, crop, 0).is_err());
    }
}
