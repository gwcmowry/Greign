//! Fused full-resolution quality rendering on GPU textures.

use std::borrow::Cow;
use std::sync::mpsc;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use greign_pipeline::{HighlightContext, QualitySettings, camera_to_rec2020_matrix};
use greign_raw::{Mosaic, Orientation};
use rayon::prelude::*;
use wgpu::util::DeviceExt;

use crate::{DispatchGeometry, GpuRuntime, GpuRuntimeError};

/// Compact color-managed GPU result ready for CPU encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuRgb8Image {
    /// Display-oriented width.
    pub width: u32,
    /// Display-oriented height.
    pub height: u32,
    /// Interleaved row-major RGB bytes.
    pub pixels: Vec<u8>,
}

/// GPU-resident color-managed quality result for direct presentation.
pub struct GpuQualityTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    allocation: crate::AllocationId,
    memory: std::sync::Arc<std::sync::Mutex<crate::MemoryBudget>>,
}

impl GpuQualityTexture {
    /// Display-oriented width.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }
    /// Display-oriented height.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }
    /// Color-managed RGBA8 texture.
    #[must_use]
    pub const fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
    /// Default texture view for presentation.
    #[must_use]
    pub const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
}

impl Drop for GpuQualityTexture {
    fn drop(&mut self) {
        let _ = self
            .memory
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .release(self.allocation);
    }
}

/// Cached full-resolution scene-linear texture before interactive exposure,
/// sharpening, tone, and display conversion.
pub struct GpuPreparedQuality {
    _working: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    output_width: u32,
    output_height: u32,
    orientation: u32,
    allocation: crate::AllocationId,
    memory: std::sync::Arc<std::sync::Mutex<crate::MemoryBudget>>,
}

impl GpuPreparedQuality {
    /// Scene-linear source width.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }
    /// Scene-linear source height.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }
}

impl Drop for GpuPreparedQuality {
    fn drop(&mut self) {
        let _ = self
            .memory
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .release(self.allocation);
    }
}

/// Device timestamps and host wall time for one full render submission.
#[derive(Clone, Debug, PartialEq)]
pub struct GpuQualityTiming {
    /// Host time estimating color at clipped-region boundaries before upload.
    pub highlight_analysis_ms: f64,
    /// CPU time packing two `u16` samples per upload word.
    pub raw_pack_ms: f64,
    /// Host resource, texture, uniform, and bind-group creation.
    pub resource_setup_ms: f64,
    /// Normalization/crop/WB/highlight sensor kernel.
    pub sensor_ms: f64,
    /// Fused edge-aware demosaic kernel.
    pub demosaic_ms: f64,
    /// Camera-matrix kernel.
    pub color_ms: f64,
    /// Sharpen/tone/sRGB/orientation kernel.
    pub output_ms: f64,
    /// Encoding, submission, GPU execution, and image/timestamp mapping.
    pub submission_wall_ms: f64,
    /// CPU time removing row padding and alpha after readback.
    pub rgb_pack_ms: f64,
}

pub(crate) struct QualityPipelines {
    sensor: wgpu::ComputePipeline,
    sensor_layout: wgpu::BindGroupLayout,
    demosaic: wgpu::ComputePipeline,
    demosaic_layout: wgpu::BindGroupLayout,
    color: wgpu::ComputePipeline,
    color_layout: wgpu::BindGroupLayout,
    output: wgpu::ComputePipeline,
    output_layout: wgpu::BindGroupLayout,
}

impl QualityPipelines {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let pipeline = |label: &'static str, source: &'static str| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(source)),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: None,
                module: &shader,
                entry_point: Some("process"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        };
        let sensor = pipeline("greign-quality-sensor", crate::QUALITY_SENSOR_SHADER);
        let demosaic = pipeline("greign-quality-demosaic", crate::QUALITY_DEMOSAIC_SHADER);
        let color = pipeline("greign-quality-color", crate::QUALITY_COLOR_SHADER);
        let output = pipeline("greign-quality-output", crate::QUALITY_OUTPUT_SHADER);
        let sensor_layout = sensor.get_bind_group_layout(0);
        let demosaic_layout = demosaic.get_bind_group_layout(0);
        let color_layout = color.get_bind_group_layout(0);
        let output_layout = output.get_bind_group_layout(0);
        Self {
            sensor,
            sensor_layout,
            demosaic,
            demosaic_layout,
            color,
            color_layout,
            output,
            output_layout,
        }
    }
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct SensorParams {
    source: [u32; 4],
    output: [u32; 4],
    cfa: [u32; 4],
    black: [f32; 4],
    white: [f32; 4],
    wb_threshold: [f32; 4],
    level_counts: [u32; 4],
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct DemosaicTextureParams {
    dimensions: [u32; 4],
    cfa: [u32; 4],
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct ColorParams {
    row0: [f32; 4],
    row1: [f32; 4],
    row2: [f32; 4],
    misc: [f32; 4],
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct OutputParams {
    dimensions: [u32; 4],
    settings: [f32; 4],
}

impl GpuRuntime {
    /// Prepares and caches the edit-independent full-resolution working image
    /// on the GPU, using host-computed clipped-region color context.
    ///
    /// # Errors
    ///
    /// Returns [`GpuRuntimeError`] for invalid metadata, unsupported CFA,
    /// device limits, or memory budget exhaustion.
    #[allow(clippy::too_many_lines)]
    pub fn prepare_quality_resident(
        &mut self,
        mosaic: &Mosaic,
        settings: QualitySettings,
    ) -> Result<(GpuPreparedQuality, f64), GpuRuntimeError> {
        let started = Instant::now();
        let prepared = PreparedQuality::new(mosaic, settings)?;
        let allocation = self
            .memory
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reserve(prepared.reserved_bytes)?;
        let packed = pack_u16(&mosaic.samples);
        let raw = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("greign-quality-cache-raw"),
                contents: bytemuck::cast_slice(&packed),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let sensor = texture(
            &self.device,
            "greign-quality-cache-sensor",
            prepared.width,
            prepared.height,
            wgpu::TextureFormat::R32Float,
        );
        let demosaic = texture(
            &self.device,
            "greign-quality-cache-demosaic",
            prepared.width,
            prepared.height,
            wgpu::TextureFormat::Rgba32Float,
        );
        let working = texture(
            &self.device,
            "greign-quality-cache-working",
            prepared.width,
            prepared.height,
            wgpu::TextureFormat::Rgba32Float,
        );
        let sensor_view = sensor.create_view(&wgpu::TextureViewDescriptor::default());
        let demosaic_view = demosaic.create_view(&wgpu::TextureViewDescriptor::default());
        let working_view = working.create_view(&wgpu::TextureViewDescriptor::default());
        let presentation_view = working.create_view(&wgpu::TextureViewDescriptor::default());
        let sensor_uniform = uniform(
            &self.device,
            "greign-quality-cache-sensor-params",
            &prepared.sensor,
        );
        let demosaic_uniform = uniform(
            &self.device,
            "greign-quality-cache-demosaic-params",
            &prepared.demosaic,
        );
        let color_uniform = uniform(
            &self.device,
            "greign-quality-cache-color-params",
            &prepared.color,
        );
        let sensor_group = sensor_bind_group(
            &self.device,
            "greign-quality-cache-sensor-group",
            &self.quality.sensor_layout,
            raw.as_entire_binding(),
            &sensor_view,
            sensor_uniform.as_entire_binding(),
            &prepared.highlights,
        );
        let demosaic_group = texture_bind_group(
            &self.device,
            "greign-quality-cache-demosaic-group",
            &self.quality.demosaic_layout,
            &sensor_view,
            &demosaic_view,
            demosaic_uniform.as_entire_binding(),
        );
        let color_group = texture_bind_group(
            &self.device,
            "greign-quality-cache-color-group",
            &self.quality.color_layout,
            &demosaic_view,
            &working_view,
            color_uniform.as_entire_binding(),
        );
        let geometry = DispatchGeometry::new(prepared.width, prepared.height, 16, 16)
            .ok_or(GpuRuntimeError::InvalidInput)?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("greign-quality-cache-commands"),
            });
        untimed_quality_pass(
            &mut encoder,
            "sensor",
            &self.quality.sensor,
            &sensor_group,
            geometry,
        );
        untimed_quality_pass(
            &mut encoder,
            "demosaic",
            &self.quality.demosaic,
            &demosaic_group,
            geometry,
        );
        untimed_quality_pass(
            &mut encoder,
            "color",
            &self.quality.color,
            &color_group,
            geometry,
        );
        self.queue.submit([encoder.finish()]);
        let host_submit_ms = started.elapsed().as_secs_f64() * 1_000.0;
        Ok((
            GpuPreparedQuality {
                _working: working,
                view: presentation_view,
                width: prepared.width,
                height: prepared.height,
                output_width: prepared.output_width,
                output_height: prepared.output_height,
                orientation: prepared.output.dimensions[2],
                allocation,
                memory: std::sync::Arc::clone(&self.memory),
            },
            host_submit_ms,
        ))
    }

    /// Applies interactive exposure, sharpening, tone, output color, and
    /// orientation to a cached working texture in one GPU kernel.
    ///
    /// # Errors
    ///
    /// Returns [`GpuRuntimeError`] for invalid settings or memory exhaustion.
    #[allow(clippy::cast_precision_loss)]
    pub fn render_prepared_quality(
        &mut self,
        prepared: &GpuPreparedQuality,
        settings: QualitySettings,
    ) -> Result<(GpuQualityTexture, f64), GpuRuntimeError> {
        if !settings.exposure_ev.is_finite()
            || !settings.exposure_ev.exp2().is_finite()
            || !settings.chroma_denoise.is_finite()
            || !(0.0..=1.0).contains(&settings.chroma_denoise)
            || !settings.capture_sharpening.is_finite()
            || !(0.0..=2.0).contains(&settings.capture_sharpening)
        {
            return Err(GpuRuntimeError::InvalidInput);
        }
        let started = Instant::now();
        let bytes = u64::from(prepared.output_width) * u64::from(prepared.output_height) * 4;
        let allocation = self
            .memory
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reserve(bytes)?;
        let output = texture(
            &self.device,
            "greign-quality-edit-output",
            prepared.output_width,
            prepared.output_height,
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let presentation_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let params = OutputParams {
            dimensions: [
                prepared.width,
                prepared.height,
                prepared.orientation,
                prepared.output_width,
            ],
            settings: [
                settings.capture_sharpening,
                prepared.output_height as f32,
                settings.exposure_ev.exp2(),
                settings.chroma_denoise,
            ],
        };
        let uniform = uniform(&self.device, "greign-quality-edit-params", &params);
        let group = texture_bind_group(
            &self.device,
            "greign-quality-edit-group",
            &self.quality.output_layout,
            &prepared.view,
            &output_view,
            uniform.as_entire_binding(),
        );
        let geometry = DispatchGeometry::new(prepared.output_width, prepared.output_height, 16, 16)
            .ok_or(GpuRuntimeError::InvalidInput)?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("greign-quality-edit-commands"),
            });
        untimed_quality_pass(
            &mut encoder,
            "output",
            &self.quality.output,
            &group,
            geometry,
        );
        self.queue.submit([encoder.finish()]);
        let host_submit_ms = started.elapsed().as_secs_f64() * 1_000.0;
        Ok((
            GpuQualityTexture {
                texture: output,
                view: presentation_view,
                width: prepared.output_width,
                height: prepared.output_height,
                allocation,
                memory: std::sync::Arc::clone(&self.memory),
            },
            host_submit_ms,
        ))
    }

    /// Submits the complete quality path into a directly presentable RGBA8
    /// texture without CPU image readback.
    ///
    /// The returned host time covers packing, resource creation, command
    /// encoding, and queue submission. GPU work remains asynchronous.
    ///
    /// # Errors
    ///
    /// Returns [`GpuRuntimeError`] for invalid RAW metadata, unsupported CFA,
    /// device limits, or memory budget exhaustion.
    #[allow(clippy::too_many_lines)]
    pub fn develop_quality_resident(
        &mut self,
        mosaic: &Mosaic,
        settings: QualitySettings,
    ) -> Result<(GpuQualityTexture, f64), GpuRuntimeError> {
        let started = Instant::now();
        let prepared = PreparedQuality::new(mosaic, settings)?;
        let allocation = self
            .memory
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reserve(prepared.reserved_bytes)?;
        let packed = pack_u16(&mosaic.samples);
        let raw = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("greign-quality-resident-raw"),
                contents: bytemuck::cast_slice(&packed),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let sensor = texture(
            &self.device,
            "greign-quality-resident-sensor",
            prepared.width,
            prepared.height,
            wgpu::TextureFormat::R32Float,
        );
        let demosaic = texture(
            &self.device,
            "greign-quality-resident-demosaic",
            prepared.width,
            prepared.height,
            wgpu::TextureFormat::Rgba32Float,
        );
        let working = texture(
            &self.device,
            "greign-quality-resident-working",
            prepared.width,
            prepared.height,
            wgpu::TextureFormat::Rgba32Float,
        );
        let output = texture(
            &self.device,
            "greign-quality-resident-output",
            prepared.output_width,
            prepared.output_height,
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let sensor_view = sensor.create_view(&wgpu::TextureViewDescriptor::default());
        let demosaic_view = demosaic.create_view(&wgpu::TextureViewDescriptor::default());
        let working_view = working.create_view(&wgpu::TextureViewDescriptor::default());
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let presentation_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let sensor_uniform = uniform(
            &self.device,
            "greign-quality-resident-sensor-params",
            &prepared.sensor,
        );
        let demosaic_uniform = uniform(
            &self.device,
            "greign-quality-resident-demosaic-params",
            &prepared.demosaic,
        );
        let color_uniform = uniform(
            &self.device,
            "greign-quality-resident-color-params",
            &prepared.color,
        );
        let output_uniform = uniform(
            &self.device,
            "greign-quality-resident-output-params",
            &prepared.output,
        );
        let sensor_group = sensor_bind_group(
            &self.device,
            "greign-quality-resident-sensor-group",
            &self.quality.sensor_layout,
            raw.as_entire_binding(),
            &sensor_view,
            sensor_uniform.as_entire_binding(),
            &prepared.highlights,
        );
        let demosaic_group = texture_bind_group(
            &self.device,
            "greign-quality-resident-demosaic-group",
            &self.quality.demosaic_layout,
            &sensor_view,
            &demosaic_view,
            demosaic_uniform.as_entire_binding(),
        );
        let color_group = texture_bind_group(
            &self.device,
            "greign-quality-resident-color-group",
            &self.quality.color_layout,
            &demosaic_view,
            &working_view,
            color_uniform.as_entire_binding(),
        );
        let output_group = texture_bind_group(
            &self.device,
            "greign-quality-resident-output-group",
            &self.quality.output_layout,
            &working_view,
            &output_view,
            output_uniform.as_entire_binding(),
        );
        let geometry = DispatchGeometry::new(prepared.width, prepared.height, 16, 16)
            .ok_or(GpuRuntimeError::InvalidInput)?;
        let output_geometry =
            DispatchGeometry::new(prepared.output_width, prepared.output_height, 16, 16)
                .ok_or(GpuRuntimeError::InvalidInput)?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("greign-quality-resident-commands"),
            });
        untimed_quality_pass(
            &mut encoder,
            "sensor",
            &self.quality.sensor,
            &sensor_group,
            geometry,
        );
        untimed_quality_pass(
            &mut encoder,
            "demosaic",
            &self.quality.demosaic,
            &demosaic_group,
            geometry,
        );
        untimed_quality_pass(
            &mut encoder,
            "color",
            &self.quality.color,
            &color_group,
            geometry,
        );
        untimed_quality_pass(
            &mut encoder,
            "output",
            &self.quality.output,
            &output_group,
            output_geometry,
        );
        self.queue.submit([encoder.finish()]);
        let host_submit_ms = started.elapsed().as_secs_f64() * 1_000.0;
        Ok((
            GpuQualityTexture {
                texture: output,
                view: presentation_view,
                width: prepared.output_width,
                height: prepared.output_height,
                allocation,
                memory: std::sync::Arc::clone(&self.memory),
            },
            host_submit_ms,
        ))
    }

    /// Runs the complete quality path on the GPU and reads back one compact
    /// RGB8 image for encoding.
    ///
    /// # Errors
    ///
    /// Returns [`GpuRuntimeError`] for invalid RAW metadata, unsupported CFA,
    /// device limits, memory budget, shader execution, polling, or mapping.
    pub fn develop_quality_readback(
        &mut self,
        mosaic: &Mosaic,
        settings: QualitySettings,
    ) -> Result<(GpuRgb8Image, GpuQualityTiming), GpuRuntimeError> {
        if !self
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            return Err(GpuRuntimeError::TimestampUnsupported);
        }
        let prepared = PreparedQuality::new(mosaic, settings)?;
        let allocation = self
            .memory
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reserve(prepared.reserved_bytes)?;
        let result = self.develop_quality_allocated(mosaic, &prepared);
        self.memory
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .release(allocation)?;
        result
    }

    #[allow(clippy::too_many_lines)]
    fn develop_quality_allocated(
        &self,
        mosaic: &Mosaic,
        prepared: &PreparedQuality,
    ) -> Result<(GpuRgb8Image, GpuQualityTiming), GpuRuntimeError> {
        let raw_pack_start = Instant::now();
        let packed = pack_u16(&mosaic.samples);
        let raw_pack_ms = raw_pack_start.elapsed().as_secs_f64() * 1_000.0;
        let resource_start = Instant::now();
        let raw = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("greign-quality-packed-raw"),
                contents: bytemuck::cast_slice(&packed),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let sensor = texture(
            &self.device,
            "greign-quality-sensor",
            prepared.width,
            prepared.height,
            wgpu::TextureFormat::R32Float,
        );
        let demosaic = texture(
            &self.device,
            "greign-quality-demosaic",
            prepared.width,
            prepared.height,
            wgpu::TextureFormat::Rgba32Float,
        );
        let working = texture(
            &self.device,
            "greign-quality-working",
            prepared.width,
            prepared.height,
            wgpu::TextureFormat::Rgba32Float,
        );
        let output = texture(
            &self.device,
            "greign-quality-output",
            prepared.output_width,
            prepared.output_height,
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let sensor_view = sensor.create_view(&wgpu::TextureViewDescriptor::default());
        let demosaic_view = demosaic.create_view(&wgpu::TextureViewDescriptor::default());
        let working_view = working.create_view(&wgpu::TextureViewDescriptor::default());
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let sensor_uniform = uniform(
            &self.device,
            "greign-quality-sensor-params",
            &prepared.sensor,
        );
        let demosaic_uniform = uniform(
            &self.device,
            "greign-quality-demosaic-params",
            &prepared.demosaic,
        );
        let color_uniform = uniform(&self.device, "greign-quality-color-params", &prepared.color);
        let output_uniform = uniform(
            &self.device,
            "greign-quality-output-params",
            &prepared.output,
        );
        let sensor_group = sensor_bind_group(
            &self.device,
            "greign-quality-sensor-group",
            &self.quality.sensor_layout,
            raw.as_entire_binding(),
            &sensor_view,
            sensor_uniform.as_entire_binding(),
            &prepared.highlights,
        );
        let demosaic_group = texture_bind_group(
            &self.device,
            "greign-quality-demosaic-group",
            &self.quality.demosaic_layout,
            &sensor_view,
            &demosaic_view,
            demosaic_uniform.as_entire_binding(),
        );
        let color_group = texture_bind_group(
            &self.device,
            "greign-quality-color-group",
            &self.quality.color_layout,
            &demosaic_view,
            &working_view,
            color_uniform.as_entire_binding(),
        );
        let output_group = texture_bind_group(
            &self.device,
            "greign-quality-output-group",
            &self.quality.output_layout,
            &working_view,
            &output_view,
            output_uniform.as_entire_binding(),
        );
        let bytes_per_row = prepared.output_width * 4;
        let padded_bytes_per_row = bytes_per_row.div_ceil(256) * 256;
        let staging_bytes = u64::from(padded_bytes_per_row) * u64::from(prepared.output_height);
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("greign-quality-readback"),
            size: staging_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let queries = self.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("greign-quality-timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: 8,
        });
        let query_bytes = 8 * size_of::<u64>() as u64;
        let query_resolve = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("greign-quality-query-resolve"),
            size: query_bytes,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let query_staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("greign-quality-query-staging"),
            size: query_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let geometry = DispatchGeometry::new(prepared.width, prepared.height, 16, 16)
            .ok_or(GpuRuntimeError::InvalidInput)?;
        let output_geometry =
            DispatchGeometry::new(prepared.output_width, prepared.output_height, 16, 16)
                .ok_or(GpuRuntimeError::InvalidInput)?;
        let resource_setup_ms = resource_start.elapsed().as_secs_f64() * 1_000.0;
        let start = Instant::now();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("greign-quality-commands"),
            });
        quality_pass(
            &mut encoder,
            "sensor",
            &self.quality.sensor,
            &sensor_group,
            geometry,
            &queries,
            0,
        );
        quality_pass(
            &mut encoder,
            "demosaic",
            &self.quality.demosaic,
            &demosaic_group,
            geometry,
            &queries,
            2,
        );
        quality_pass(
            &mut encoder,
            "color",
            &self.quality.color,
            &color_group,
            geometry,
            &queries,
            4,
        );
        quality_pass(
            &mut encoder,
            "output",
            &self.quality.output,
            &output_group,
            output_geometry,
            &queries,
            6,
        );
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &output,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(prepared.output_height),
                },
            },
            wgpu::Extent3d {
                width: prepared.output_width,
                height: prepared.output_height,
                depth_or_array_layers: 1,
            },
        );
        encoder.resolve_query_set(&queries, 0..8, &query_resolve, 0);
        encoder.copy_buffer_to_buffer(&query_resolve, 0, &query_staging, 0, query_bytes);
        let submission = self.queue.submit([encoder.finish()]);
        let image_slice = staging.slice(..);
        let query_slice = query_staging.slice(..);
        let (image_sender, image_receiver) = mpsc::sync_channel(1);
        let (query_sender, query_receiver) = mpsc::sync_channel(1);
        image_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = image_sender.send(result);
        });
        query_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = query_sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_for(submission))
            .map_err(|error| GpuRuntimeError::Poll(error.to_string()))?;
        image_receiver
            .recv()
            .map_err(|error| GpuRuntimeError::Map(error.to_string()))?
            .map_err(|error| GpuRuntimeError::Map(error.to_string()))?;
        query_receiver
            .recv()
            .map_err(|error| GpuRuntimeError::Map(error.to_string()))?
            .map_err(|error| GpuRuntimeError::Map(error.to_string()))?;
        let submission_wall_ms = start.elapsed().as_secs_f64() * 1_000.0;
        let mapped = image_slice.get_mapped_range();
        let rgb_pack_start = Instant::now();
        let rgb = strip_alpha_and_padding(
            &mapped,
            prepared.output_width,
            prepared.output_height,
            padded_bytes_per_row,
        );
        let rgb_pack_ms = rgb_pack_start.elapsed().as_secs_f64() * 1_000.0;
        drop(mapped);
        staging.unmap();
        let mapped_queries = query_slice.get_mapped_range();
        let stamps = bytemuck::cast_slice::<u8, u64>(&mapped_queries);
        let period = f64::from(self.queue.get_timestamp_period());
        let elapsed = |index: usize| {
            super::wgpu_runtime::ticks_to_ms(stamps[index + 1] - stamps[index], period)
        };
        let timing = GpuQualityTiming {
            highlight_analysis_ms: prepared.highlight_analysis_ms,
            raw_pack_ms,
            resource_setup_ms,
            sensor_ms: elapsed(0),
            demosaic_ms: elapsed(2),
            color_ms: elapsed(4),
            output_ms: elapsed(6),
            submission_wall_ms,
            rgb_pack_ms,
        };
        drop(mapped_queries);
        query_staging.unmap();
        Ok((
            GpuRgb8Image {
                width: prepared.output_width,
                height: prepared.output_height,
                pixels: rgb,
            },
            timing,
        ))
    }
}

struct PreparedQuality {
    width: u32,
    height: u32,
    output_width: u32,
    output_height: u32,
    sensor: SensorParams,
    demosaic: DemosaicTextureParams,
    color: ColorParams,
    output: OutputParams,
    reserved_bytes: u64,
    highlights: HighlightContext,
    highlight_analysis_ms: f64,
}

impl PreparedQuality {
    #[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
    fn new(mosaic: &Mosaic, settings: QualitySettings) -> Result<Self, GpuRuntimeError> {
        if !settings.chroma_denoise.is_finite()
            || !(0.0..=1.0).contains(&settings.chroma_denoise)
            || !settings.exposure_ev.is_finite()
            || !settings.exposure_ev.exp2().is_finite()
            || !settings.capture_sharpening.is_finite()
            || !(0.0..=2.0).contains(&settings.capture_sharpening)
        {
            return Err(GpuRuntimeError::InvalidInput);
        }
        let metadata = &mosaic.metadata;
        let expected = usize::try_from(metadata.width)
            .ok()
            .and_then(|w| {
                usize::try_from(metadata.height)
                    .ok()
                    .and_then(|h| w.checked_mul(h))
            })
            .ok_or(GpuRuntimeError::InvalidInput)?;
        if expected != mosaic.samples.len() || metadata.components_per_pixel != 1 {
            return Err(GpuRuntimeError::InvalidInput);
        }
        let cfa_string = metadata
            .cfa_pattern
            .as_deref()
            .ok_or(GpuRuntimeError::InvalidInput)?;
        let cfa = cfa_codes(cfa_string).ok_or(GpuRuntimeError::InvalidInput)?;
        if !matches!(metadata.black_levels.len(), 1 | 4)
            || !matches!(metadata.white_levels.len(), 1 | 4)
        {
            return Err(GpuRuntimeError::InvalidInput);
        }
        let crop = metadata
            .crop_area
            .or(metadata.active_area)
            .unwrap_or(greign_raw::ActiveArea {
                x: 0,
                y: 0,
                width: metadata.width,
                height: metadata.height,
            });
        if crop.width == 0
            || crop.height == 0
            || crop
                .x
                .checked_add(crop.width)
                .is_none_or(|x| x > metadata.width)
            || crop
                .y
                .checked_add(crop.height)
                .is_none_or(|y| y > metadata.height)
        {
            return Err(GpuRuntimeError::InvalidInput);
        }
        let shifted = [
            cfa[usize::try_from((crop.y % 2) * 2 + (crop.x % 2)).unwrap_or(0)],
            cfa[usize::try_from((crop.y % 2) * 2 + ((crop.x + 1) % 2)).unwrap_or(0)],
            cfa[usize::try_from(((crop.y + 1) % 2) * 2 + (crop.x % 2)).unwrap_or(0)],
            cfa[usize::try_from(((crop.y + 1) % 2) * 2 + ((crop.x + 1) % 2)).unwrap_or(0)],
        ];
        let matrix = camera_to_rec2020_matrix(metadata.xyz_to_camera_d65)?;
        let (orientation, output_width, output_height) =
            orientation(metadata.orientation, crop.width, crop.height);
        let black = four_levels(&metadata.black_levels);
        let white = four_levels(&metadata.white_levels);
        let highlight_start = Instant::now();
        let highlights = HighlightContext::from_raw(mosaic, settings.highlight_threshold)?;
        let highlight_analysis_ms = highlight_start.elapsed().as_secs_f64() * 1_000.0;
        let sensor = SensorParams {
            source: [metadata.width, metadata.height, crop.x, crop.y],
            output: [
                crop.width,
                crop.height,
                highlights.width,
                greign_pipeline::HIGHLIGHT_TILE,
            ],
            cfa,
            black,
            white,
            wb_threshold: [
                metadata.white_balance[0],
                metadata.white_balance[1],
                metadata.white_balance[2],
                settings.highlight_threshold,
            ],
            level_counts: [
                u32::try_from(metadata.black_levels.len())
                    .map_err(|_| GpuRuntimeError::InvalidInput)?,
                u32::try_from(metadata.white_levels.len())
                    .map_err(|_| GpuRuntimeError::InvalidInput)?,
                0,
                0,
            ],
        };
        let demosaic = DemosaicTextureParams {
            dimensions: [crop.width, crop.height, 0, 0],
            cfa: shifted,
        };
        let color = ColorParams {
            row0: [matrix[0][0], matrix[0][1], matrix[0][2], 0.0],
            row1: [matrix[1][0], matrix[1][1], matrix[1][2], 0.0],
            row2: [matrix[2][0], matrix[2][1], matrix[2][2], 0.0],
            misc: [1.0, crop.width as f32, crop.height as f32, 0.0],
        };
        let output = OutputParams {
            dimensions: [crop.width, crop.height, orientation, output_width],
            settings: [
                settings.capture_sharpening,
                output_height as f32,
                settings.exposure_ev.exp2(),
                settings.chroma_denoise,
            ],
        };
        let pixels = u64::from(crop.width) * u64::from(crop.height);
        let output_pixels = u64::from(output_width) * u64::from(output_height);
        let packed_raw = (u64::try_from(mosaic.samples.len())
            .map_err(|_| GpuRuntimeError::InvalidInput)?)
        .div_ceil(2)
            * 4;
        let padded_row = u64::from((output_width * 4).div_ceil(256) * 256);
        let reserved_bytes = packed_raw
            + (highlights.corrections.len() * size_of::<[f32; 4]>()) as u64
            + pixels * 4
            + pixels * 16 * 2
            + output_pixels * 4
            + padded_row * u64::from(output_height)
            + 4096;
        Ok(Self {
            width: crop.width,
            height: crop.height,
            output_width,
            output_height,
            sensor,
            demosaic,
            color,
            output,
            reserved_bytes,
            highlights,
            highlight_analysis_ms,
        })
    }
}

fn cfa_codes(pattern: &str) -> Option<[u32; 4]> {
    let bytes = pattern.as_bytes();
    if bytes.len() != 4 {
        return None;
    }
    let mut out = [0; 4];
    for (i, b) in bytes.iter().enumerate() {
        out[i] = match b {
            b'R' => 0,
            b'G' => 1,
            b'B' => 2,
            _ => return None,
        };
    }
    Some(out)
}
fn four_levels(levels: &[f32]) -> [f32; 4] {
    if levels.len() == 1 {
        [levels[0]; 4]
    } else {
        [levels[0], levels[1], levels[2], levels[3]]
    }
}
fn orientation(value: Orientation, w: u32, h: u32) -> (u32, u32, u32) {
    let code = match value {
        Orientation::Normal | Orientation::Unknown => 1,
        Orientation::HorizontalFlip => 2,
        Orientation::Rotate180 => 3,
        Orientation::VerticalFlip => 4,
        Orientation::Transpose => 5,
        Orientation::Rotate90 => 6,
        Orientation::Transverse => 7,
        Orientation::Rotate270 => 8,
    };
    if matches!(code, 5..=8) {
        (code, h, w)
    } else {
        (code, w, h)
    }
}
fn pack_u16(samples: &[u16]) -> Vec<u32> {
    samples
        .par_chunks(2)
        .map(|pair| u32::from(pair[0]) | (u32::from(*pair.get(1).unwrap_or(&0)) << 16))
        .collect()
}
fn texture(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
fn uniform<T: Pod>(device: &wgpu::Device, label: &str, value: &T) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::bytes_of(value),
        usage: wgpu::BufferUsages::UNIFORM,
    })
}
#[allow(clippy::too_many_arguments)]
fn sensor_bind_group<'a>(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    source: wgpu::BindingResource<'a>,
    output: &wgpu::TextureView,
    uniform: wgpu::BindingResource<'a>,
    context: &HighlightContext,
) -> wgpu::BindGroup {
    let corrections = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("greign-highlight-region-context"),
        contents: bytemuck::cast_slice(&context.corrections),
        usage: wgpu::BufferUsages::STORAGE,
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: source,
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(output),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform,
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: corrections.as_entire_binding(),
            },
        ],
    })
}

fn bind_group<'a>(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    source: wgpu::BindingResource<'a>,
    output: &wgpu::TextureView,
    uniform: wgpu::BindingResource<'a>,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: source,
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(output),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform,
            },
        ],
    })
}
fn texture_bind_group(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    source: &wgpu::TextureView,
    output: &wgpu::TextureView,
    uniform: wgpu::BindingResource<'_>,
) -> wgpu::BindGroup {
    bind_group(
        device,
        label,
        layout,
        wgpu::BindingResource::TextureView(source),
        output,
        uniform,
    )
}
fn quality_pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    pipeline: &wgpu::ComputePipeline,
    group: &wgpu::BindGroup,
    geometry: DispatchGeometry,
    queries: &wgpu::QuerySet,
    index: u32,
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some(label),
        timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
            query_set: queries,
            beginning_of_pass_write_index: Some(index),
            end_of_pass_write_index: Some(index + 1),
        }),
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.dispatch_workgroups(geometry.groups_x, geometry.groups_y, 1);
}

fn untimed_quality_pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    pipeline: &wgpu::ComputePipeline,
    group: &wgpu::BindGroup,
    geometry: DispatchGeometry,
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some(label),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.dispatch_workgroups(geometry.groups_x, geometry.groups_y, 1);
}
fn strip_alpha_and_padding(bytes: &[u8], width: u32, height: u32, stride: u32) -> Vec<u8> {
    let row_rgb = usize::try_from(width * 3).unwrap_or(0);
    let mut rgb = vec![0_u8; row_rgb * usize::try_from(height).unwrap_or(0)];
    rgb.par_chunks_mut(row_rgb)
        .enumerate()
        .for_each(|(y, output)| {
            let start = y * usize::try_from(stride).unwrap_or(0);
            let end = start + usize::try_from(width * 4).unwrap_or(0);
            for (source, destination) in bytes[start..end]
                .chunks_exact(4)
                .zip(output.chunks_exact_mut(3))
            {
                destination.copy_from_slice(&source[..3]);
            }
        });
    rgb
}
