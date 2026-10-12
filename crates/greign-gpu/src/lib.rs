//! GPU resource, dispatch, and fallback invariants independent of windowing.
//!
//! The `wgpu` adapter is intentionally added only when the dependency and a
//! Vulkan device can be validated. These types establish its accounting and
//! failure contract without requiring GPU access in CI.

mod demosaic;
mod dispatch;
mod fallback;
mod memory;
mod primitive;
mod quality;
mod runtime;
mod wgpu_runtime;

pub use demosaic::{BayerPattern, DemosaicParams};
pub use dispatch::DispatchGeometry;
pub use fallback::{ExecutionBackend, execute_with_cpu_fallback};
pub use memory::{AllocationId, MemoryBudget, MemoryError, MemoryReport};
pub use primitive::{Primitive, PrimitiveError, ShaderOperation, ShaderParams};
pub use quality::{GpuPreparedQuality, GpuQualityTexture, GpuQualityTiming, GpuRgb8Image};
pub use runtime::{DeviceGeneration, DeviceState, RecoveryAction, RuntimeLifecycle};
pub use wgpu_runtime::{GpuRuntime, GpuRuntimeError, GpuTiming, ResidentTexture};

/// WGSL kernels for normalization, white balance, exposure, tone, and matrix
/// primitives. The adapter compiles this source during device initialization.
pub const PRIMITIVE_SHADER: &str = include_str!("../shaders/primitives.wgsl");

/// Equivalent primitive kernels writing directly to a GPU-resident texture.
pub const TEXTURE_SHADER: &str = include_str!("../shaders/texture_output.wgsl");

/// Bilinear Bayer demosaic parity kernel.
pub const DEMOSAIC_SHADER: &str = include_str!("../shaders/bilinear_demosaic.wgsl");
pub(crate) const QUALITY_SENSOR_SHADER: &str = include_str!("../shaders/quality_sensor.wgsl");
pub(crate) const QUALITY_DEMOSAIC_SHADER: &str = include_str!("../shaders/quality_demosaic.wgsl");
pub(crate) const QUALITY_COLOR_SHADER: &str = include_str!("../shaders/quality_color.wgsl");
pub(crate) const QUALITY_OUTPUT_SHADER: &str = include_str!("../shaders/quality_output.wgsl");

/// Runtime capabilities persisted with benchmark results.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityReport {
    /// Human-readable adapter name.
    pub adapter_name: String,
    /// Backend name, expected to be Vulkan on Linux.
    pub backend: String,
    /// Maximum two-dimensional texture extent.
    pub max_texture_dimension_2d: u32,
    /// Maximum storage buffer binding size.
    pub max_storage_buffer_binding_size: u64,
    /// Whether timestamp queries are supported.
    pub timestamp_queries: bool,
}
