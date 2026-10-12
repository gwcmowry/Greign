//! Native `wgpu` compute runtime: Metal on macOS, Vulkan elsewhere.

use std::borrow::Cow;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Instant;

use wgpu::util::DeviceExt;

use crate::{
    BayerPattern, CapabilityReport, DEMOSAIC_SHADER, DemosaicParams, DispatchGeometry,
    MemoryBudget, MemoryError, PRIMITIVE_SHADER, Primitive, ShaderParams, TEXTURE_SHADER,
};

/// Initialization, validation, allocation, or execution failure.
#[derive(Debug)]
pub enum GpuRuntimeError {
    /// No compatible adapter or device could be created.
    Initialization(String),
    /// WGSL or pipeline validation failed.
    Shader(String),
    /// Dimensions and input length disagree.
    InvalidInput,
    /// Explicit resource budget refused an allocation.
    Memory(MemoryError),
    /// Staging-buffer mapping failed.
    Map(String),
    /// Device polling failed.
    Poll(String),
    /// Primitive parameters are invalid.
    Primitive(crate::PrimitiveError),
    /// CPU-side color or parameter validation failed.
    Pipeline(greign_pipeline::PipelineError),
    /// Adapter cannot report device timestamps.
    TimestampUnsupported,
}

impl fmt::Display for GpuRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initialization(message) => {
                write!(formatter, "GPU initialization failed: {message}")
            }
            Self::Shader(message) => write!(formatter, "GPU shader validation failed: {message}"),
            Self::InvalidInput => formatter.write_str("GPU input length does not match dimensions"),
            Self::Memory(error) => error.fmt(formatter),
            Self::Map(message) => write!(formatter, "GPU buffer mapping failed: {message}"),
            Self::Poll(message) => write!(formatter, "GPU polling failed: {message}"),
            Self::Primitive(error) => error.fmt(formatter),
            Self::Pipeline(error) => error.fmt(formatter),
            Self::TimestampUnsupported => {
                formatter.write_str("GPU timestamp queries are unsupported")
            }
        }
    }
}

/// GPU-resident timestamp samples plus end-to-end submission wall time.
#[derive(Clone, Debug, PartialEq)]
pub struct GpuTiming {
    /// One timestamp-derived kernel duration per dispatch.
    pub kernel_ms: Vec<f64>,
    /// Host wall time for encoding, submission, execution, and timestamp readback.
    pub wall_ms: f64,
}

/// GPU-resident RGBA32F output suitable for direct downstream presentation.
pub struct ResidentTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    allocation: crate::AllocationId,
    memory: Arc<Mutex<MemoryBudget>>,
}

impl fmt::Debug for ResidentTexture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResidentTexture")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl ResidentTexture {
    /// Pixel width.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Pixel height.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Texture handle for downstream GPU operations.
    #[must_use]
    pub const fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// Default view for direct presentation or later compute stages.
    #[must_use]
    pub const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
}

impl Drop for ResidentTexture {
    fn drop(&mut self) {
        let _ = lock_budget(&self.memory).release(self.allocation);
    }
}

impl std::error::Error for GpuRuntimeError {}

impl From<MemoryError> for GpuRuntimeError {
    fn from(error: MemoryError) -> Self {
        Self::Memory(error)
    }
}

impl From<crate::PrimitiveError> for GpuRuntimeError {
    fn from(error: crate::PrimitiveError) -> Self {
        Self::Primitive(error)
    }
}

impl From<greign_pipeline::PipelineError> for GpuRuntimeError {
    fn from(error: greign_pipeline::PipelineError) -> Self {
        Self::Pipeline(error)
    }
}

/// Ready compute device, queue, primitive pipeline, and memory accounting.
pub struct GpuRuntime {
    _instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    texture_pipeline: wgpu::ComputePipeline,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    demosaic_pipeline: wgpu::ComputePipeline,
    demosaic_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) quality: crate::quality::QualityPipelines,
    pub(crate) memory: Arc<Mutex<MemoryBudget>>,
}

impl fmt::Debug for GpuRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GpuRuntime")
            .field("capabilities", &self.capability_report())
            .field("memory", &self.memory_report())
            .finish_non_exhaustive()
    }
}

impl GpuRuntime {
    /// Initializes a native adapter, validates WGSL, and creates the primitive pipeline.
    ///
    /// # Errors
    ///
    /// Returns [`GpuRuntimeError`] when no native device can be created or the
    /// checked-in shader fails validation.
    pub fn initialize(memory_budget_bytes: u64) -> Result<Self, GpuRuntimeError> {
        pollster::block_on(Self::initialize_async(memory_budget_bytes))
    }

    async fn initialize_async(memory_budget_bytes: u64) -> Result<Self, GpuRuntimeError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: if cfg!(target_os = "macos") {
                wgpu::Backends::METAL
            } else {
                wgpu::Backends::VULKAN
            },
            ..wgpu::InstanceDescriptor::default()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .map_err(|error| GpuRuntimeError::Initialization(error.to_string()))?;
        let adapter_features = adapter.features();
        let requested_features = adapter_features & wgpu::Features::TIMESTAMP_QUERY;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("greign-device"),
                required_features: requested_features,
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|error| GpuRuntimeError::Initialization(error.to_string()))?;

        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("greign-primitives"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(PRIMITIVE_SHADER)),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("greign-primitives"),
            layout: None,
            module: &shader,
            entry_point: Some("process"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let texture_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("greign-texture-primitives"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(TEXTURE_SHADER)),
        });
        let texture_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("greign-texture-primitives"),
            layout: None,
            module: &texture_shader,
            entry_point: Some("process"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let demosaic_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("greign-bilinear-demosaic"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(DEMOSAIC_SHADER)),
        });
        let demosaic_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("greign-bilinear-demosaic"),
            layout: None,
            module: &demosaic_shader,
            entry_point: Some("process"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let quality = crate::quality::QualityPipelines::new(&device);
        if let Some(error) = device.pop_error_scope().await {
            return Err(GpuRuntimeError::Shader(error.to_string()));
        }
        let bind_group_layout = pipeline.get_bind_group_layout(0);
        let texture_bind_group_layout = texture_pipeline.get_bind_group_layout(0);
        let demosaic_bind_group_layout = demosaic_pipeline.get_bind_group_layout(0);
        Ok(Self {
            _instance: instance,
            adapter,
            device,
            queue,
            pipeline,
            bind_group_layout,
            texture_pipeline,
            texture_bind_group_layout,
            demosaic_pipeline,
            demosaic_bind_group_layout,
            quality,
            memory: Arc::new(Mutex::new(MemoryBudget::new(memory_budget_bytes))),
        })
    }

    /// Reports adapter identity, relevant limits, and timestamp support.
    #[must_use]
    pub fn capability_report(&self) -> CapabilityReport {
        let info = self.adapter.get_info();
        let limits = self.adapter.limits();
        CapabilityReport {
            adapter_name: info.name,
            backend: format!("{:?}", info.backend),
            max_texture_dimension_2d: limits.max_texture_dimension_2d,
            max_storage_buffer_binding_size: u64::from(limits.max_storage_buffer_binding_size),
            timestamp_queries: self
                .device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY),
        }
    }

    /// Current and peak explicit resource accounting.
    #[must_use]
    pub fn memory_report(&self) -> crate::MemoryReport {
        lock_budget(&self.memory).report()
    }

    /// Waits until all previously submitted GPU work has completed.
    ///
    /// # Errors
    ///
    /// Returns [`GpuRuntimeError::Poll`] if the device reports a polling
    /// failure.
    pub fn wait_idle(&self) -> Result<(), GpuRuntimeError> {
        let submission = self.queue.submit([]);
        self.device
            .poll(wgpu::PollType::wait_for(submission))
            .map_err(|error| GpuRuntimeError::Poll(error.to_string()))?;
        Ok(())
    }

    /// Executes one primitive and reads the result back for parity testing.
    ///
    /// Production display code must use a GPU-resident output instead of this
    /// diagnostic readback path.
    ///
    /// # Errors
    ///
    /// Returns [`GpuRuntimeError`] for invalid inputs, budget exhaustion,
    /// device polling, or staging-map failures.
    pub fn execute_readback(
        &mut self,
        width: u32,
        height: u32,
        primitive: Primitive,
        input: &[[f32; 4]],
    ) -> Result<Vec<[f32; 4]>, GpuRuntimeError> {
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or(GpuRuntimeError::InvalidInput)?;
        if expected != input.len() {
            return Err(GpuRuntimeError::InvalidInput);
        }
        let geometry =
            DispatchGeometry::new(width, height, 16, 16).ok_or(GpuRuntimeError::InvalidInput)?;
        let params = ShaderParams::new(width, height, primitive)?;
        let byte_size = u64::try_from(std::mem::size_of_val(input))
            .map_err(|_| GpuRuntimeError::InvalidInput)?;
        let source_allocation = lock_budget(&self.memory).reserve(byte_size)?;
        let output_allocation = match lock_budget(&self.memory).reserve(byte_size) {
            Ok(allocation) => allocation,
            Err(error) => {
                lock_budget(&self.memory).release(source_allocation)?;
                return Err(error.into());
            }
        };
        let staging_allocation = match lock_budget(&self.memory).reserve(byte_size) {
            Ok(allocation) => allocation,
            Err(error) => {
                let mut memory = lock_budget(&self.memory);
                memory.release(source_allocation)?;
                memory.release(output_allocation)?;
                return Err(error.into());
            }
        };
        let result = self.execute_allocated(geometry, params, input, byte_size);
        let mut memory = lock_budget(&self.memory);
        memory.release(source_allocation)?;
        memory.release(output_allocation)?;
        memory.release(staging_allocation)?;
        result
    }

    /// Bilinearly demosaics a Bayer plane and reads RGBA32F pixels back.
    ///
    /// This is the deterministic parity path. Production rendering will keep
    /// the output resident and process bounded tiles.
    ///
    /// # Errors
    ///
    /// Returns [`GpuRuntimeError`] for invalid dimensions, budget exhaustion,
    /// device polling, or staging-map failures.
    pub fn demosaic_readback(
        &mut self,
        width: u32,
        height: u32,
        pattern: BayerPattern,
        input: &[f32],
    ) -> Result<Vec<[f32; 4]>, GpuRuntimeError> {
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or(GpuRuntimeError::InvalidInput)?;
        if expected != input.len() {
            return Err(GpuRuntimeError::InvalidInput);
        }
        let geometry =
            DispatchGeometry::new(width, height, 16, 16).ok_or(GpuRuntimeError::InvalidInput)?;
        let params =
            DemosaicParams::new(width, height, pattern).ok_or(GpuRuntimeError::InvalidInput)?;
        let source_bytes = u64::try_from(std::mem::size_of_val(input))
            .map_err(|_| GpuRuntimeError::InvalidInput)?;
        let output_bytes = u64::try_from(expected)
            .ok()
            .and_then(|count| count.checked_mul(size_of::<[f32; 4]>() as u64))
            .ok_or(GpuRuntimeError::InvalidInput)?;
        let reserved_bytes = source_bytes
            .checked_add(
                output_bytes
                    .checked_mul(2)
                    .ok_or(GpuRuntimeError::InvalidInput)?,
            )
            .and_then(|bytes| bytes.checked_add(size_of::<DemosaicParams>() as u64))
            .ok_or(GpuRuntimeError::InvalidInput)?;
        let allocation = lock_budget(&self.memory).reserve(reserved_bytes)?;
        let result = self.demosaic_allocated(geometry, params, input, source_bytes, output_bytes);
        lock_budget(&self.memory).release(allocation)?;
        result
    }

    /// Executes a primitive into a GPU-resident RGBA32F texture.
    ///
    /// The returned texture owns an allocation lease and releases it on drop.
    /// Pixel data is never copied back to CPU memory.
    ///
    /// # Errors
    ///
    /// Returns [`GpuRuntimeError`] for invalid inputs, parameters, or budget
    /// exhaustion.
    pub fn execute_resident(
        &mut self,
        width: u32,
        height: u32,
        primitive: Primitive,
        input: &[[f32; 4]],
    ) -> Result<ResidentTexture, GpuRuntimeError> {
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or(GpuRuntimeError::InvalidInput)?;
        if expected != input.len() {
            return Err(GpuRuntimeError::InvalidInput);
        }
        let geometry =
            DispatchGeometry::new(width, height, 16, 16).ok_or(GpuRuntimeError::InvalidInput)?;
        let params = ShaderParams::new(width, height, primitive)?;
        let byte_size = u64::try_from(std::mem::size_of_val(input))
            .map_err(|_| GpuRuntimeError::InvalidInput)?;
        let source_allocation = lock_budget(&self.memory).reserve(byte_size)?;
        let texture_allocation = match lock_budget(&self.memory).reserve(byte_size) {
            Ok(allocation) => allocation,
            Err(error) => {
                lock_budget(&self.memory).release(source_allocation)?;
                return Err(error.into());
            }
        };

        let source = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("greign-resident-source"),
                contents: bytemuck::cast_slice(input),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("greign-resident-uniform"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("greign-resident-output"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("greign-resident-bind-group"),
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: source.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("greign-resident-commands"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("greign-resident-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.texture_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(geometry.groups_x, geometry.groups_y, 1);
        }
        self.queue.submit([encoder.finish()]);
        lock_budget(&self.memory).release(source_allocation)?;
        Ok(ResidentTexture {
            texture,
            view,
            width,
            height,
            allocation: texture_allocation,
            memory: Arc::clone(&self.memory),
        })
    }

    /// Benchmarks repeated GPU-resident dispatches using device timestamps.
    ///
    /// Input is uploaded once, output remains GPU-resident, and only the small
    /// timestamp buffer is read back. This models an interactive preview stage
    /// without `PCIe` image readback.
    ///
    /// # Errors
    ///
    /// Returns [`GpuRuntimeError`] for unsupported timestamps, invalid inputs,
    /// budget exhaustion, polling, or staging-map failures.
    pub fn benchmark_resident(
        &mut self,
        width: u32,
        height: u32,
        primitive: Primitive,
        input: &[[f32; 4]],
        iterations: u32,
    ) -> Result<GpuTiming, GpuRuntimeError> {
        if !self
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            return Err(GpuRuntimeError::TimestampUnsupported);
        }
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or(GpuRuntimeError::InvalidInput)?;
        if expected != input.len() || iterations == 0 {
            return Err(GpuRuntimeError::InvalidInput);
        }
        let query_count = iterations
            .checked_mul(2)
            .ok_or(GpuRuntimeError::InvalidInput)?;
        let geometry =
            DispatchGeometry::new(width, height, 16, 16).ok_or(GpuRuntimeError::InvalidInput)?;
        let params = ShaderParams::new(width, height, primitive)?;
        let byte_size = u64::try_from(std::mem::size_of_val(input))
            .map_err(|_| GpuRuntimeError::InvalidInput)?;
        let query_bytes = u64::from(query_count) * size_of::<u64>() as u64;
        let reserved_bytes = byte_size
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(query_bytes * 2))
            .and_then(|bytes| bytes.checked_add(size_of::<ShaderParams>() as u64))
            .ok_or(GpuRuntimeError::InvalidInput)?;
        let allocation = lock_budget(&self.memory).reserve(reserved_bytes)?;
        let result = self.benchmark_resident_allocated(
            geometry,
            params,
            input,
            iterations,
            query_count,
            byte_size,
            query_bytes,
        );
        lock_budget(&self.memory).release(allocation)?;
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn benchmark_resident_allocated(
        &self,
        geometry: DispatchGeometry,
        params: ShaderParams,
        input: &[[f32; 4]],
        iterations: u32,
        query_count: u32,
        byte_size: u64,
        query_bytes: u64,
    ) -> Result<GpuTiming, GpuRuntimeError> {
        let source = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("greign-benchmark-source"),
                contents: bytemuck::cast_slice(input),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("greign-benchmark-output"),
            size: byte_size,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("greign-benchmark-uniform"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("greign-benchmark-bind-group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: source.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let queries = self.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("greign-benchmark-timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: query_count,
        });
        let resolve = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("greign-benchmark-resolve"),
            size: query_bytes,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("greign-benchmark-timestamp-staging"),
            size: query_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let start = Instant::now();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("greign-benchmark-commands"),
            });
        for iteration in 0..iterations {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("greign-benchmark-pass"),
                timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                    query_set: &queries,
                    beginning_of_pass_write_index: Some(iteration * 2),
                    end_of_pass_write_index: Some(iteration * 2 + 1),
                }),
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(geometry.groups_x, geometry.groups_y, 1);
        }
        encoder.resolve_query_set(&queries, 0..query_count, &resolve, 0);
        encoder.copy_buffer_to_buffer(&resolve, 0, &staging, 0, query_bytes);
        let submission = self.queue.submit([encoder.finish()]);
        let slice = staging.slice(..);
        let (sender, receiver) = mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_for(submission))
            .map_err(|error| GpuRuntimeError::Poll(error.to_string()))?;
        receiver
            .recv()
            .map_err(|error| GpuRuntimeError::Map(error.to_string()))?
            .map_err(|error| GpuRuntimeError::Map(error.to_string()))?;
        let wall_ms = start.elapsed().as_secs_f64() * 1_000.0;
        let mapped = slice.get_mapped_range();
        let timestamps = bytemuck::cast_slice::<u8, u64>(&mapped);
        let period = f64::from(self.queue.get_timestamp_period());
        let kernel_ms = timestamps
            .chunks_exact(2)
            .map(|pair| ticks_to_ms(pair[1] - pair[0], period))
            .collect();
        drop(mapped);
        staging.unmap();
        Ok(GpuTiming { kernel_ms, wall_ms })
    }

    fn execute_allocated(
        &self,
        geometry: DispatchGeometry,
        params: ShaderParams,
        input: &[[f32; 4]],
        byte_size: u64,
    ) -> Result<Vec<[f32; 4]>, GpuRuntimeError> {
        let source = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("greign-primitive-source"),
                contents: bytemuck::cast_slice(input),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("greign-primitive-output"),
            size: byte_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("greign-primitive-staging"),
            size: byte_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("greign-primitive-uniform"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("greign-primitive-bind-group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: source.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("greign-primitive-commands"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("greign-primitive-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(geometry.groups_x, geometry.groups_y, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &staging, 0, byte_size);
        let submission = self.queue.submit([encoder.finish()]);
        let slice = staging.slice(..);
        let (sender, receiver) = mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_for(submission))
            .map_err(|error| GpuRuntimeError::Poll(error.to_string()))?;
        receiver
            .recv()
            .map_err(|error| GpuRuntimeError::Map(error.to_string()))?
            .map_err(|error| GpuRuntimeError::Map(error.to_string()))?;
        let mapped = slice.get_mapped_range();
        let result = bytemuck::cast_slice::<u8, [f32; 4]>(&mapped).to_vec();
        drop(mapped);
        staging.unmap();
        Ok(result)
    }

    fn demosaic_allocated(
        &self,
        geometry: DispatchGeometry,
        params: DemosaicParams,
        input: &[f32],
        source_bytes: u64,
        output_bytes: u64,
    ) -> Result<Vec<[f32; 4]>, GpuRuntimeError> {
        let source = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("greign-demosaic-source"),
                contents: bytemuck::cast_slice(input),
                usage: wgpu::BufferUsages::STORAGE,
            });
        debug_assert_eq!(source.size(), source_bytes);
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("greign-demosaic-output"),
            size: output_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("greign-demosaic-staging"),
            size: output_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("greign-demosaic-uniform"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("greign-demosaic-bind-group"),
            layout: &self.demosaic_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: source.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("greign-demosaic-commands"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("greign-demosaic-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.demosaic_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(geometry.groups_x, geometry.groups_y, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &staging, 0, output_bytes);
        let submission = self.queue.submit([encoder.finish()]);
        let slice = staging.slice(..);
        let (sender, receiver) = mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_for(submission))
            .map_err(|error| GpuRuntimeError::Poll(error.to_string()))?;
        receiver
            .recv()
            .map_err(|error| GpuRuntimeError::Map(error.to_string()))?
            .map_err(|error| GpuRuntimeError::Map(error.to_string()))?;
        let mapped = slice.get_mapped_range();
        let result = bytemuck::cast_slice::<u8, [f32; 4]>(&mapped).to_vec();
        drop(mapped);
        staging.unmap();
        Ok(result)
    }
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn ticks_to_ms(ticks: u64, period_ns: f64) -> f64 {
    ticks as f64 * period_ns / 1_000_000.0
}

fn lock_budget(memory: &Mutex<MemoryBudget>) -> MutexGuard<'_, MemoryBudget> {
    memory
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
