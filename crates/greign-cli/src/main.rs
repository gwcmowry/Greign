//! Headless inspection, corpus, rendering, and benchmark commands.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use greign_core::VersionInfo;
use greign_raw::corpus;
use greign_raw::{PreviewEncoding, RawDecoder, RawlerDecoder};
use serde_json::json;
use sha2::{Digest, Sha256};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "greign", version, about = "Headless Greign tools")]
struct Cli {
    /// Emit structured JSON logs.
    #[arg(long, global = true, env = "GREIGN_LOG_JSON")]
    log_json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Report build and persisted-schema versions.
    Version {
        /// Print only fields that must agree across build profiles.
        #[arg(long)]
        schema_only: bool,
    },
    /// Manage the declared public RAW corpus.
    Corpus {
        #[command(subcommand)]
        command: CorpusCommand,
    },
    /// Inspect and extract data from a RAW file.
    Raw {
        #[command(subcommand)]
        command: RawCommand,
    },
}

#[derive(Debug, Subcommand)]
enum CorpusCommand {
    /// Verify manifest invariants, file hashes, and optional decoded metadata.
    Verify {
        /// Corpus manifest JSON file.
        manifest: PathBuf,
        /// Directory containing files named `<id>.<lowercase format>`.
        #[arg(long, default_value = "fixtures/downloads")]
        root: PathBuf,
        /// Report absent data as skipped instead of failing.
        #[arg(long)]
        allow_missing: bool,
        /// Also check camera identity and dimensions through the primary decoder.
        #[arg(long)]
        decode: bool,
        /// Fully decode mosaics and compare declared sample digests.
        #[arg(long)]
        mosaic: bool,
    },
}

#[derive(Debug, Subcommand)]
enum RawCommand {
    /// Extract normalized metadata without decoding the mosaic.
    Inspect {
        /// Source RAW file.
        input: PathBuf,
    },
    /// Extract an embedded preview without decoding the mosaic.
    Preview {
        /// Source RAW file.
        input: PathBuf,
        /// Destination JPEG, PNG, or TIFF.
        output: PathBuf,
    },
    /// Decode the mosaic and compute a stable SHA-256 sample digest.
    Digest {
        /// Source RAW file.
        input: PathBuf,
    },
    /// Render through the CPU reference pipeline to JPEG or 16-bit TIFF.
    Render {
        /// Source RAW file.
        input: PathBuf,
        /// Destination `.jpg`, `.jpeg`, `.tif`, or `.tiff`.
        output: PathBuf,
        /// Exposure compensation in stops.
        #[arg(long, default_value_t = 0.0)]
        exposure: f32,
        /// JPEG quality from 1 through 100.
        #[arg(long, default_value_t = greign_export::DEFAULT_JPEG_QUALITY)]
        jpeg_quality: u8,
        /// Use edge-aware demosaic, highlight repair, and capture sharpening.
        #[arg(long)]
        quality: bool,
        /// Chroma-noise reduction strength, zero (off) through one.
        #[arg(long, default_value_t = 0.5, requires = "quality")]
        chroma_denoise: f32,
        /// Luminance sharpening strength, zero (off) through two.
        #[arg(long, default_value_t = 0.2, requires = "quality")]
        sharpening: f32,
        /// Run the full quality pipeline on Vulkan; currently JPEG-only.
        #[arg(long, requires = "quality", conflicts_with = "preview_dimension")]
        gpu: bool,
        /// Render the cached progressive path with this maximum dimension.
        #[arg(long, requires = "quality")]
        preview_dimension: Option<u32>,
    },
    /// Measure metadata, preview, and optional mosaic latency distributions.
    Benchmark {
        /// Source RAW file.
        input: PathBuf,
        /// Repetitions for each enabled path.
        #[arg(long, default_value_t = 20)]
        iterations: u32,
        /// Include full mosaic decoding.
        #[arg(long)]
        mosaic: bool,
        /// Prepare and repeatedly finish a cached progressive preview.
        #[arg(long)]
        preview_render: bool,
        /// Longest edge for the progressive preview benchmark.
        #[arg(long, default_value_t = 960)]
        preview_dimension: u32,
    },
    /// Benchmark cached full-resolution GPU edit readiness.
    GpuBenchmark {
        /// Source RAW file.
        input: PathBuf,
        /// Repeated exposure updates after one GPU preparation.
        #[arg(long, default_value_t = 20)]
        iterations: u32,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.log_json)?;
    info!(version = %VersionInfo::current(), "greign started");

    match cli.command {
        Command::Version { schema_only } => {
            let _ = schema_only;
            println!("{}", VersionInfo::current());
        }
        Command::Corpus { command } => run_corpus(command)?,
        Command::Raw { command } => run_raw(command, &RawlerDecoder)?,
    }
    Ok(())
}

fn run_corpus(command: CorpusCommand) -> Result<()> {
    match command {
        CorpusCommand::Verify {
            manifest,
            root,
            allow_missing,
            decode,
            mosaic,
        } => {
            let decoder = RawlerDecoder;
            let backend: Option<&dyn RawDecoder> = (decode || mosaic).then_some(&decoder);
            let report = corpus::verify(&manifest, &root, allow_missing, backend, mosaic)
                .with_context(|| format!("failed to verify {}", manifest.display()))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    Ok(())
}

fn run_raw(command: RawCommand, decoder: &dyn RawDecoder) -> Result<()> {
    match command {
        RawCommand::Inspect { input } => {
            let start = Instant::now();
            let metadata = decoder.metadata(&input)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "metadata": metadata,
                    "elapsed_ms": start.elapsed().as_secs_f64() * 1_000.0,
                    "peak_rss_kib": peak_rss_kib(),
                    "mosaic_decoded": false
                }))?
            );
        }
        RawCommand::Preview { input, output } => {
            let start = Instant::now();
            let preview = decoder.embedded_preview(&input)?;
            write_preview(&preview, &output)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "output": output,
                    "width": preview.width,
                    "height": preview.height,
                    "elapsed_ms": start.elapsed().as_secs_f64() * 1_000.0,
                    "peak_rss_kib": peak_rss_kib(),
                    "mosaic_decoded": false
                }))?
            );
        }
        RawCommand::Digest { input } => {
            let start = Instant::now();
            let mosaic = decoder.mosaic(&input)?;
            let mut hasher = Sha256::new();
            for sample in &mosaic.samples {
                hasher.update(sample.to_le_bytes());
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "backend": decoder.name(),
                    "width": mosaic.metadata.width,
                    "height": mosaic.metadata.height,
                    "components_per_pixel": mosaic.metadata.components_per_pixel,
                    "samples": mosaic.samples.len(),
                    "sha256_le_u16": format!("{:x}", hasher.finalize()),
                    "elapsed_ms": start.elapsed().as_secs_f64() * 1_000.0,
                    "peak_rss_kib": peak_rss_kib()
                }))?
            );
        }
        RawCommand::Render {
            input,
            output,
            exposure,
            jpeg_quality,
            quality,
            chroma_denoise,
            sharpening,
            gpu,
            preview_dimension,
        } => render_raw(
            decoder,
            &input,
            &output,
            exposure,
            jpeg_quality,
            quality,
            chroma_denoise,
            sharpening,
            gpu,
            preview_dimension,
        )?,
        RawCommand::Benchmark {
            input,
            iterations,
            mosaic,
            preview_render,
            preview_dimension,
        } => benchmark_raw(
            decoder,
            &input,
            iterations,
            mosaic,
            preview_render,
            preview_dimension,
        )?,
        RawCommand::GpuBenchmark { input, iterations } => {
            benchmark_gpu_edits(decoder, &input, iterations)?;
        }
    }
    Ok(())
}

fn benchmark_gpu_edits(decoder: &dyn RawDecoder, input: &Path, iterations: u32) -> Result<()> {
    if iterations == 0 {
        return Err(anyhow!("benchmark iterations must be greater than zero"));
    }
    let total_start = Instant::now();
    let mosaic = decoder.mosaic(input)?;
    let decode_ms = total_start.elapsed().as_secs_f64() * 1_000.0;
    let init_start = Instant::now();
    let mut runtime = greign_gpu::GpuRuntime::initialize(2 * 1024 * 1024 * 1024)?;
    let gpu_init_ms = init_start.elapsed().as_secs_f64() * 1_000.0;
    let prepare_start = Instant::now();
    let (prepared, prepare_submit_ms) =
        runtime.prepare_quality_resident(&mosaic, greign_pipeline::QualitySettings::default())?;
    runtime.wait_idle()?;
    let prepare_ready_ms = prepare_start.elapsed().as_secs_f64() * 1_000.0;
    let mut host_submit_ms = Vec::with_capacity(usize::try_from(iterations)?);
    let mut ready_ms = Vec::with_capacity(usize::try_from(iterations)?);
    for iteration in 0..iterations {
        let start = Instant::now();
        let exposure_ev = f32::from(u8::try_from(iteration % 9).unwrap_or(0)) * 0.125 - 0.5;
        let (texture, submit_ms) = runtime.render_prepared_quality(
            &prepared,
            greign_pipeline::QualitySettings {
                exposure_ev,
                ..greign_pipeline::QualitySettings::default()
            },
        )?;
        host_submit_ms.push(submit_ms);
        runtime.wait_idle()?;
        ready_ms.push(start.elapsed().as_secs_f64() * 1_000.0);
        drop(texture);
    }
    let memory = runtime.memory_report();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "backend": decoder.name(),
            "input": input,
            "iterations": iterations,
            "decode_ms": decode_ms,
            "gpu_init_ms": gpu_init_ms,
            "prepare_submit_ms": prepare_submit_ms,
            "prepare_ready_ms": prepare_ready_ms,
            "edit_host_submit_ms": distribution(&mut host_submit_ms),
            "edit_ready_ms": distribution(&mut ready_ms),
            "prepared_width": prepared.width(),
            "prepared_height": prepared.height(),
            "gpu_peak_bytes": memory.peak_bytes,
            "peak_rss_kib": peak_rss_kib(),
        }))?
    );
    Ok(())
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn render_raw(
    decoder: &dyn RawDecoder,
    input: &Path,
    output: &Path,
    exposure: f32,
    jpeg_quality: u8,
    quality: bool,
    chroma_denoise: f32,
    sharpening: f32,
    gpu: bool,
    preview_dimension: Option<u32>,
) -> Result<()> {
    let start = Instant::now();
    let mosaic = decoder.mosaic(input)?;
    let decode_ms = start.elapsed().as_secs_f64() * 1_000.0;
    let settings = greign_pipeline::QualitySettings {
        exposure_ev: exposure,
        chroma_denoise,
        capture_sharpening: sharpening,
        ..greign_pipeline::QualitySettings::default()
    };
    if gpu {
        let extension = output.extension().and_then(|value| value.to_str());
        if !extension.is_some_and(|value| {
            value.eq_ignore_ascii_case("jpg") || value.eq_ignore_ascii_case("jpeg")
        }) {
            return Err(anyhow!("GPU export currently supports JPEG output only"));
        }
        let init_start = Instant::now();
        let mut runtime = greign_gpu::GpuRuntime::initialize(2 * 1024 * 1024 * 1024)?;
        let gpu_init_ms = init_start.elapsed().as_secs_f64() * 1_000.0;
        let render_start = Instant::now();
        let (image, timing) = runtime.develop_quality_readback(&mosaic, settings)?;
        let gpu_render_readback_pack_ms = render_start.elapsed().as_secs_f64() * 1_000.0;
        let encode_start = Instant::now();
        greign_export::export_rgb8_jpeg_fast(
            image.width,
            image.height,
            &image.pixels,
            output,
            jpeg_quality,
        )?;
        let encode_ms = encode_start.elapsed().as_secs_f64() * 1_000.0;
        let capabilities = runtime.capability_report();
        let memory = runtime.memory_report();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "backend": decoder.name(),
                "gpu": {
                    "adapter": capabilities.adapter_name,
                    "backend": capabilities.backend,
                    "timestamp_queries": capabilities.timestamp_queries,
                },
                "output": output,
                "width": image.width,
                "height": image.height,
                "decode_ms": decode_ms,
                "gpu_init_ms": gpu_init_ms,
                "gpu_render_readback_pack_ms": gpu_render_readback_pack_ms,
                "gpu_timing": {
                    "highlight_analysis_ms": timing.highlight_analysis_ms,
                    "raw_pack_ms": timing.raw_pack_ms,
                    "resource_setup_ms": timing.resource_setup_ms,
                    "sensor_ms": timing.sensor_ms,
                    "demosaic_ms": timing.demosaic_ms,
                    "color_ms": timing.color_ms,
                    "output_ms": timing.output_ms,
                    "submission_wall_ms": timing.submission_wall_ms,
                    "rgb_pack_ms": timing.rgb_pack_ms,
                },
                "jpeg_encode_ms": encode_ms,
                "elapsed_ms": start.elapsed().as_secs_f64() * 1_000.0,
                "peak_rss_kib": peak_rss_kib(),
                "gpu_memory": {
                    "limit_bytes": memory.limit_bytes,
                    "used_bytes": memory.used_bytes,
                    "peak_bytes": memory.peak_bytes,
                    "allocations": memory.allocations,
                },
            }))?
        );
        return Ok(());
    }
    let image = if let Some(max_dimension) = preview_dimension {
        let prepared = greign_pipeline::prepare_quality_preview(
            &mosaic,
            max_dimension,
            settings.highlight_threshold,
        )?;
        greign_pipeline::finish_quality_preview(&prepared, settings)?
    } else if quality {
        greign_pipeline::develop_quality(&mosaic, settings)?
    } else {
        greign_pipeline::develop_reference(
            &mosaic,
            greign_pipeline::DevelopSettings {
                exposure_ev: exposure,
            },
        )?
    };
    greign_export::export_srgb(
        &image,
        output,
        greign_export::ExportOptions { jpeg_quality },
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "backend": decoder.name(),
            "reference_cpu": !quality,
            "quality_cpu": quality,
            "progressive_max_dimension": preview_dimension,
            "output": output,
            "width": image.width,
            "height": image.height,
            "elapsed_ms": start.elapsed().as_secs_f64() * 1_000.0,
            "peak_rss_kib": peak_rss_kib()
        }))?
    );
    Ok(())
}

fn benchmark_raw(
    decoder: &dyn RawDecoder,
    input: &Path,
    iterations: u32,
    include_mosaic: bool,
    preview_render: bool,
    preview_dimension: u32,
) -> Result<()> {
    if iterations == 0 {
        return Err(anyhow!("benchmark iterations must be greater than zero"));
    }
    let mut metadata_ms = Vec::with_capacity(usize::try_from(iterations)?);
    let mut preview_ms = Vec::with_capacity(usize::try_from(iterations)?);
    let mut mosaic_ms =
        include_mosaic.then(|| Vec::with_capacity(usize::try_from(iterations).unwrap_or(0)));
    let metadata = decoder.metadata(input)?;
    let (prepared_preview, preview_prepare_ms) = if preview_render {
        let start = Instant::now();
        let mosaic = decoder.mosaic(input)?;
        let preview = greign_pipeline::prepare_quality_preview(
            &mosaic,
            preview_dimension,
            greign_pipeline::QualitySettings::default().highlight_threshold,
        )?;
        (Some(preview), Some(start.elapsed().as_secs_f64() * 1_000.0))
    } else {
        (None, None)
    };
    let mut preview_render_ms =
        preview_render.then(|| Vec::with_capacity(usize::try_from(iterations).unwrap_or(0)));
    for iteration in 0..iterations {
        let start = Instant::now();
        std::hint::black_box(decoder.metadata(input)?);
        metadata_ms.push(start.elapsed().as_secs_f64() * 1_000.0);

        let start = Instant::now();
        std::hint::black_box(decoder.embedded_preview(input)?);
        preview_ms.push(start.elapsed().as_secs_f64() * 1_000.0);

        if let Some(mosaic_ms) = &mut mosaic_ms {
            let start = Instant::now();
            std::hint::black_box(decoder.mosaic(input)?);
            mosaic_ms.push(start.elapsed().as_secs_f64() * 1_000.0);
        }
        if let (Some(prepared), Some(samples)) = (&prepared_preview, &mut preview_render_ms) {
            let start = Instant::now();
            let exposure_ev = f32::from(u8::try_from(iteration % 5).unwrap_or(0)) * 0.1 - 0.2;
            std::hint::black_box(greign_pipeline::finish_quality_preview(
                prepared,
                greign_pipeline::QualitySettings {
                    exposure_ev,
                    ..greign_pipeline::QualitySettings::default()
                },
            )?);
            samples.push(start.elapsed().as_secs_f64() * 1_000.0);
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "backend": decoder.name(),
            "input": input,
            "width": metadata.width,
            "height": metadata.height,
            "iterations": iterations,
            "metadata_ms": distribution(&mut metadata_ms),
            "preview_ms": distribution(&mut preview_ms),
            "mosaic_ms": mosaic_ms.as_mut().map(|samples| distribution(samples)),
            "preview_prepare_ms": preview_prepare_ms,
            "preview_render_ms": preview_render_ms.as_mut().map(|samples| distribution(samples)),
            "prepared_preview": prepared_preview.as_ref().map(|preview| json!({
                "width": preview.width(),
                "height": preview.height(),
                "bytes": preview.byte_len(),
            })),
            "peak_rss_kib": peak_rss_kib(),
            "build_profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        }))?
    );
    Ok(())
}

fn distribution(samples: &mut [f64]) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    let last = samples.len() - 1;
    let p95_index = (last * 95).div_ceil(100);
    json!({
        "p50": samples[last / 2],
        "p95": samples[p95_index],
        "min": samples[0],
        "max": samples[last]
    })
}

fn write_preview(preview: &greign_raw::EmbeddedPreview, output: &Path) -> Result<()> {
    let is_jpeg_output = output
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg")
        });
    match preview.encoding {
        PreviewEncoding::Jpeg if is_jpeg_output => std::fs::write(output, &preview.bytes)
            .with_context(|| format!("could not write preview {}", output.display())),
        PreviewEncoding::Jpeg => image::load_from_memory(&preview.bytes)
            .context("could not decode embedded JPEG")?
            .save(output)
            .with_context(|| format!("could not write preview {}", output.display())),
        PreviewEncoding::Rgb8 => {
            let image =
                image::RgbImage::from_raw(preview.width, preview.height, preview.bytes.clone())
                    .ok_or_else(|| anyhow!("decoder returned an invalid RGB preview length"))?;
            image
                .save(output)
                .with_context(|| format!("could not write preview {}", output.display()))
        }
    }
}

fn peak_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string(Path::new("/proc/self/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
    line.split_ascii_whitespace().nth(1)?.parse().ok()
}

fn init_logging(json: bool) -> Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("error"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if json {
        builder
            .json()
            .try_init()
            .map_err(|error| anyhow!(error.to_string()))?;
    } else {
        builder
            .try_init()
            .map_err(|error| anyhow!(error.to_string()))?;
    }
    Ok(())
}
