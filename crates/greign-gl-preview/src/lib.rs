//! Native GL adapter. Unsafe operations are restricted to current-context FFI;
//! source slices are sized/validated, and every call checks the owning thread.
use anyhow::{Result, ensure};
use glow::HasContext;
use greign_film::{Adjustments, Material, Rect};
use greign_pipeline::LinearRgbImage;
use std::{
    sync::{Arc, Mutex},
    thread::ThreadId,
};

pub mod benchmark;
pub mod export;

pub struct Preview {
    gl: Arc<glow::Context>,
    owner: ThreadId,
    program: glow::Program,
    vao: glow::VertexArray,
    texture: glow::Texture,
    pub key: String,
    pub width: u32,
    pub height: u32,
    alive: bool,
}
#[derive(Clone, Copy)]
pub struct Settings {
    pub edits: Adjustments,
    pub material: Material,
    pub crop: Rect,
    pub rotation: u8,
}
impl Preview {
    /// Called by eframe while its GL context is current. All later operations
    /// must occur on that same render thread with that context current.
    pub fn new(gl: Arc<glow::Context>) -> Result<Self> {
        // SAFETY: caller is the native creation callback; GL is current here.
        unsafe {
            let program = gl.create_program().map_err(anyhow::Error::msg)?;
            let mut shaders = Vec::new();
            for (kind, source) in [
                (
                    glow::VERTEX_SHADER,
                    "#version 330 core\nout vec2 uv;void main(){vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);uv=p;gl_Position=vec4(p*2.-1.,0.,1.);}",
                ),
                (glow::FRAGMENT_SHADER, include_str!("preview.frag")),
            ] {
                let shader = gl.create_shader(kind).map_err(anyhow::Error::msg)?;
                gl.shader_source(shader, source);
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    let error = gl.get_shader_info_log(shader);
                    gl.delete_shader(shader);
                    for shader in shaders {
                        gl.delete_shader(shader);
                    }
                    gl.delete_program(program);
                    anyhow::bail!("GPU preview shader: {error}");
                }
                gl.attach_shader(program, shader);
                shaders.push(shader);
            }
            gl.link_program(program);
            for shader in shaders {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }
            if !gl.get_program_link_status(program) {
                let error = gl.get_program_info_log(program);
                gl.delete_program(program);
                anyhow::bail!("GPU preview link: {error}");
            }
            let vao = gl.create_vertex_array().map_err(anyhow::Error::msg)?;
            let texture = gl.create_texture().map_err(anyhow::Error::msg)?;
            Ok(Self {
                gl,
                owner: std::thread::current().id(),
                program,
                vao,
                texture,
                key: String::new(),
                width: 0,
                height: 0,
                alive: true,
            })
        }
    }
    fn check_thread(&self) {
        assert_eq!(
            self.owner,
            std::thread::current().id(),
            "GL preview called off render thread"
        );
    }
    pub fn upload(&mut self, key: String, image: &LinearRgbImage) -> Result<()> {
        self.upload_inner(key, image, true)
    }
    fn upload_inner(&mut self, key: String, image: &LinearRgbImage, mipmaps: bool) -> Result<()> {
        self.check_thread();
        ensure!(
            image.width > 0
                && image.height > 0
                && u64::from(image.width) * u64::from(image.height) == image.pixels.len() as u64,
            "Invalid GPU image dimensions"
        );
        ensure!(
            image.pixels.iter().flatten().all(|v| v.is_finite()),
            "Non-finite GPU image samples"
        );
        let width = i32::try_from(image.width)?;
        let height = i32::try_from(image.height)?;
        // SAFETY: validated RGB32F slice, current owning-thread context.
        unsafe {
            let gl = &self.gl;
            let maximum = gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE);
            ensure!(
                width <= maximum && height <= maximum,
                "Image exceeds GPU texture limit ({maximum})"
            );
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
            gl.pixel_store_i32(glow::UNPACK_SKIP_ROWS, 0);
            gl.pixel_store_i32(glow::UNPACK_SKIP_PIXELS, 0);
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                if mipmaps {
                    glow::LINEAR_MIPMAP_LINEAR
                } else {
                    glow::NEAREST
                } as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGB32F as i32,
                width,
                height,
                0,
                glow::RGB,
                glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(bytemuck::cast_slice(&image.pixels))),
            );
            if mipmaps {
                gl.generate_mipmap(glow::TEXTURE_2D);
            }
            let error = gl.get_error();
            gl.bind_texture(glow::TEXTURE_2D, None);
            ensure!(error == glow::NO_ERROR, "GPU upload failed: GL {error:#x}");
        }
        self.key = key;
        self.width = image.width;
        self.height = image.height;
        Ok(())
    }
    pub fn draw(&self, settings: Settings) {
        self.draw_window(settings, [0., 0., 1., 1.]);
    }
    fn draw_window(&self, settings: Settings, window: [f32; 4]) {
        self.draw_mode(settings, window, 0);
    }
    fn draw_mode(&self, settings: Settings, window: [f32; 4], export: i32) {
        self.check_thread();
        if !self.alive || !settings.crop.valid() {
            return;
        }
        // SAFETY: GL resources were created on this thread, remain alive until
        // explicit destroy at eframe shutdown; uniforms reference valid arrays.
        unsafe {
            let gl = &self.gl;
            gl.use_program(Some(self.program));
            gl.uniform_1_i32(
                gl.get_uniform_location(self.program, "export_linear")
                    .as_ref(),
                export,
            );
            gl.bind_vertex_array(Some(self.vao));
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::BLEND);
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            gl.uniform_1_i32(gl.get_uniform_location(self.program, "source").as_ref(), 0);
            gl.uniform_4_f32(
                gl.get_uniform_location(self.program, "view_window")
                    .as_ref(),
                window[0],
                window[1],
                window[2],
                window[3],
            );
            let e = settings.edits;
            gl.uniform_4_f32(
                gl.get_uniform_location(self.program, "lighting").as_ref(),
                e.brightness,
                e.contrast,
                e.shadows,
                e.highlights,
            );
            gl.uniform_4_f32(
                gl.get_uniform_location(self.program, "balance").as_ref(),
                e.warmth,
                e.tint,
                e.saturation,
                match settings.material {
                    Material::ColorNegative => 0.,
                    Material::BlackAndWhite => 1.,
                    Material::Positive => 2.,
                },
            );
            let bands: Vec<f32> = e
                .colors
                .iter()
                .flat_map(|b| [b.hue, b.saturation, b.lightness])
                .collect();
            gl.uniform_3_f32_slice(
                gl.get_uniform_location(self.program, "bands[0]").as_ref(),
                &bands,
            );
            let mut crop = settings.crop;
            let width = self.width as f32;
            let height = self.height as f32;
            crop.x = (crop.x * width).floor() / width;
            crop.y = (crop.y * height).floor() / height;
            crop.w = (crop.w * width).floor().max(1.).min(width - crop.x * width) / width;
            crop.h = (crop.h * height)
                .floor()
                .max(1.)
                .min(height - crop.y * height)
                / height;
            gl.uniform_4_f32(
                gl.get_uniform_location(self.program, "crop").as_ref(),
                crop.x,
                crop.y,
                crop.w,
                crop.h,
            );
            gl.uniform_1_i32(
                gl.get_uniform_location(self.program, "rotation").as_ref(),
                i32::from(settings.rotation % 4),
            );
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_vertex_array(None);
            gl.use_program(None);
        }
    }
    pub fn callback(
        preview: Arc<Mutex<Self>>,
        rect: egui::Rect,
        settings: Settings,
    ) -> egui::PaintCallback {
        egui::PaintCallback {
            rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, _| {
                if let Some((viewport, window)) = visible_window(&info) {
                    let preview = preview.lock().unwrap();
                    preview.check_thread();
                    // SAFETY: current render context and screen-bounded viewport.
                    unsafe {
                        preview.gl.viewport(
                            viewport.left_px,
                            viewport.from_bottom_px,
                            viewport.width_px,
                            viewport.height_px,
                        );
                    }
                    preview.draw_window(settings, window);
                }
            })),
        }
    }
    pub fn destroy(&mut self) {
        self.check_thread();
        if !self.alive {
            return;
        }
        // SAFETY: called during native on_exit with the same current context.
        unsafe {
            self.gl.delete_texture(self.texture);
            self.gl.delete_vertex_array(self.vao);
            self.gl.delete_program(self.program);
        }
        self.alive = false;
    }
}

fn visible_window(
    info: &egui::PaintCallbackInfo,
) -> Option<(egui::epaint::ViewportInPixels, [f32; 4])> {
    let visible = info.viewport.intersect(info.clip_rect);
    if !visible.is_positive() {
        return None;
    }
    let viewport = egui::PaintCallbackInfo {
        viewport: visible,
        clip_rect: info.clip_rect,
        pixels_per_point: info.pixels_per_point,
        screen_size_px: info.screen_size_px,
    }
    .viewport_in_pixels();
    let left = (info.viewport.min.x * info.pixels_per_point).round();
    let top = (info.viewport.min.y * info.pixels_per_point).round();
    let width = (info.viewport.max.x * info.pixels_per_point).round() - left;
    let height = (info.viewport.max.y * info.pixels_per_point).round() - top;
    if width <= 0. || height <= 0. || viewport.width_px <= 0 || viewport.height_px <= 0 {
        return None;
    }
    let window = [
        (viewport.left_px as f32 - left) / width,
        (viewport.top_px as f32 - top) / height,
        viewport.width_px as f32 / width,
        viewport.height_px as f32 / height,
    ];
    Some((viewport, window))
}

#[cfg(test)]
mod tests {
    #[test]
    fn clipped_zoom_uses_unclipped_image_coordinates() {
        let info = egui::PaintCallbackInfo {
            viewport: egui::Rect::from_min_size(egui::pos2(-100., -50.), egui::vec2(1000., 800.)),
            clip_rect: egui::Rect::from_min_size(egui::pos2(200., 100.), egui::vec2(500., 400.)),
            pixels_per_point: 1.3,
            screen_size_px: [1872, 1248],
        };
        let (viewport, window) = super::visible_window(&info).unwrap();
        assert_eq!(
            (
                viewport.left_px,
                viewport.top_px,
                viewport.width_px,
                viewport.height_px
            ),
            (260, 130, 650, 520)
        );
        for (actual, expected) in window.into_iter().zip([0.3, 0.1875, 0.5, 0.5]) {
            assert!((actual - expected).abs() < 1e-6);
        }
    }
}
