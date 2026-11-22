use crate::model::Roll;
use greign_film::{self as film, Analysis, Scan};
use rayon::prelude::*;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, mpsc::Sender},
    time::Instant,
};

pub enum Event {
    Neutral {
        key: String,
        result: Result<[f32; 2], String>,
    },
    Histogram {
        key: String,
        result: Result<Box<crate::diagnostics::Histogram>, String>,
    },
    JpegFloatRender {
        prepared: Arc<film::Prepared>,
        settings: greign_gl_preview::Settings,
        reply: std::sync::mpsc::SyncSender<Result<greign_pipeline::SrgbImage, String>>,
    },
    JpegRender {
        prepared: Arc<film::Prepared>,
        settings: greign_gl_preview::Settings,
        reply: std::sync::mpsc::SyncSender<Result<image::RgbImage, String>>,
    },
    ExportProgress {
        completed: usize,
        total: usize,
        bytes: u64,
        elapsed: f64,
    },
    ExportRender {
        prepared: Arc<film::Prepared>,
        settings: greign_gl_preview::Settings,
        reply: std::sync::mpsc::SyncSender<Result<greign_pipeline::LinearRgbImage, String>>,
    },
    GpuScene(crate::preview::GpuScene),
    PreviewReady(Box<crate::preview::Output>),
    Progress(String),
    Loaded(String, Arc<Scan>),
    Failed(String, String),
    Analysis(String, Result<Analysis, String>),
    Finished,
    Exported(PathBuf, Vec<ExportFailure>, Option<Box<ExportRetry>>),
}

#[derive(Debug)]
pub struct ExportFailure {
    pub name: String,
    pub error: String,
}
pub struct ExportRetry {
    pub roll: Roll,
    pub directory: PathBuf,
    pub ids: HashSet<String>,
}

pub fn load(
    roll: Roll,
    analyze: bool,
    tx: Sender<Event>,
    ctx: eframe::egui::Context,
    profile: PathBuf,
) {
    rayon::spawn(move || {
        let start = Instant::now();
        let mut scans = Vec::new();
        // RAW decoders already use several Rayon workers per image. Bound the
        // number of live full-size mosaics while filling the remaining cores.
        let batch_size = rayon::current_num_threads().div_ceil(4).clamp(1, 4);
        for (batch_index, frames) in roll.frames.chunks(batch_size).enumerate() {
            let first = batch_index * batch_size + 1;
            let last = first + frames.len() - 1;
            let _ = tx.send(Event::Progress(format!(
                "Reading frames {first}–{last} of {}",
                roll.frames.len()
            )));
            ctx.request_repaint();
            let results: Vec<_> = frames
                .par_iter()
                .map(|frame| film::load_scan(&frame.path, Some(960), roll.demo))
                .collect();
            // Keep presentation, failures and calibration inputs in roll order.
            for (frame, result) in frames.iter().zip(results) {
                match result {
                    Ok(scan) => {
                        let scan = Arc::new(scan);
                        if !frame.rejected {
                            scans.push(scan.clone());
                        }
                        let _ = tx.send(Event::Loaded(frame.id.clone(), scan));
                    }
                    Err(e) => {
                        let _ = tx.send(Event::Failed(frame.id.clone(), e.to_string()));
                    }
                }
            }
            ctx.request_repaint();
        }
        let load_ms = start.elapsed().as_secs_f64() * 1000.0;
        if analyze {
            let now = Instant::now();
            let refs: Vec<_> = scans.iter().map(AsRef::as_ref).collect();
            let result =
                film::analyze(&refs, roll.base, roll.analysis_area).map_err(|e| e.to_string());
            let _ = tx.send(Event::Analysis(roll.id.clone(), result));
            record(
                &profile,
                &serde_json::json!({"stage":"roll-analysis","frames":scans.len(),"load_ms":load_ms,"analysis_ms":now.elapsed().as_secs_f64()*1000.0}),
            );
        }
        let _ = tx.send(Event::Finished);
        ctx.request_repaint();
    });
}
#[cfg(test)]
pub fn export(
    roll: Roll,
    directory: PathBuf,
    only: Option<HashSet<String>>,
    tx: Sender<Event>,
    ctx: eframe::egui::Context,
    profile: PathBuf,
) {
    export_with_mode(roll, directory, only, tx, ctx, profile, ExportMode::Cpu);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExportMode {
    Cpu,
    Gpu,
    /// Reference mode for measuring the previous sequential export scheduling.
    SerialCpu,
}

pub fn export_with_mode(
    roll: Roll,
    directory: PathBuf,
    only: Option<HashSet<String>>,
    tx: Sender<Event>,
    ctx: eframe::egui::Context,
    profile: PathBuf,
    mode: ExportMode,
) {
    rayon::spawn(move || {
        let start = Instant::now();
        let mut failures = Vec::new();
        let mut failed_ids = HashSet::new();
        let mut outcomes = Vec::new();
        let total = roll
            .frames
            .iter()
            .filter(|f| !f.rejected && only.as_ref().is_none_or(|ids| ids.contains(&f.id)))
            .count();
        let _ = tx.send(Event::ExportProgress {
            completed: 0,
            total,
            bytes: 0,
            elapsed: 0.,
        });
        // One encoder plus at most one queued image bounds memory while decode/
        // preparation/GPU work proceeds independently of compression and disk I/O.
        let (encode_tx, encode_rx) = std::sync::mpsc::sync_channel::<EncodeJob>(1);
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let encode_profile = profile.clone();
        let progress_tx = tx.clone();
        let progress_ctx = ctx.clone();
        let encoder = std::thread::spawn(move || {
            let mut completed = 0;
            let mut bytes = 0;
            while let Ok(job) = encode_rx.recv() {
                let encode_start = Instant::now();
                let id = job.id.clone();
                let name = job.name.clone();
                let path = job.path.clone();
                let result = encode_image(job);
                completed += 1;
                if result.is_ok() {
                    bytes += fs::metadata(path).map_or(0, |m| m.len());
                }
                record(
                    &encode_profile,
                    &serde_json::json!({"stage":"export-encode-write", "frame":id,
                    "elapsed_ms":encode_start.elapsed().as_secs_f64()*1000.0}),
                );
                let _ = result_tx.send((id, name, result.map_err(|e| e.to_string())));
                let _ = progress_tx.send(Event::ExportProgress {
                    completed,
                    total,
                    bytes,
                    elapsed: start.elapsed().as_secs_f64(),
                });
                progress_ctx.request_repaint();
            }
            bytes
        });
        let frames: Vec<_> = roll
            .frames
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.rejected && only.as_ref().is_none_or(|ids| ids.contains(&f.id)))
            .collect();
        // Decode/prepare the next source while the current frame is rendered or
        // encoded. A bounded queue prevents a whole roll accumulating in RAM.
        let source_frames: Vec<_> = frames.iter().map(|(_, frame)| (*frame).clone()).collect();
        let (source_tx, source_rx) = std::sync::mpsc::sync_channel(1);
        let (advance_tx, advance_rx) = std::sync::mpsc::channel();
        let source_profile = profile.clone();
        let material = roll.material;
        let analysis = roll.analysis.clone();
        let demo = roll.demo;
        let loader = std::thread::spawn(move || {
            for frame in source_frames {
                let prepared = prepare_export_source(
                    &frame,
                    material,
                    analysis.as_ref(),
                    demo,
                    &source_profile,
                );
                if source_tx.send(prepared).is_err() {
                    break;
                }
                if mode == ExportMode::SerialCpu && advance_rx.recv().is_err() {
                    break;
                }
            }
        });
        for (completed, &(i, frame)) in frames.iter().enumerate() {
            let _ = tx.send(Event::Progress(format!(
                "Exporting frame {} of {}",
                completed + 1,
                frames.len()
            )));
            ctx.request_repaint();
            let result = (|| -> anyhow::Result<()> {
                let stem = frame.path.file_stem().unwrap_or_default().to_string_lossy();
                let prefix: String = roll
                    .export
                    .prefix
                    .chars()
                    .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
                    .collect();
                let prefix = if prefix.is_empty() {
                    prefix
                } else {
                    format!("{prefix}-")
                };
                let ext = if roll.export.dng {
                    "dng"
                } else if roll.export.tiff {
                    "tif"
                } else {
                    "jpg"
                };
                let mut path =
                    directory.join(format!("{prefix}{:02}-{stem}-positive.{ext}", i + 1));
                let mut suffix = 2;
                while path.exists() {
                    path = directory.join(format!(
                        "{prefix}{:02}-{stem}-positive-{suffix}.{ext}",
                        i + 1
                    ));
                    suffix += 1;
                }
                // Write beside the destination, then publish only a completed image.
                let pending = path.with_file_name(format!(".{}-pending.{ext}", frame.id));
                let prepared = source_rx.recv()??;
                let settings = greign_gl_preview::Settings {
                    edits: roll.adjustments.combined(frame.adjustments),
                    material: roll.material,
                    crop: frame.crop,
                    rotation: frame.rotation,
                };
                let now = Instant::now();
                let jpeg = !roll.export.dng && !roll.export.tiff;
                let jpeg_image = if jpeg && mode == ExportMode::Gpu && frame.heals.is_empty() {
                    match request_jpeg(&prepared, settings, roll.export.long_edge > 0, &tx, &ctx) {
                        Ok(image) => Some(image),
                        Err(error) => {
                            record(
                                &profile,
                                &serde_json::json!({"stage":"jpeg-gpu-fallback", "reason":format!("{error:?}")}),
                            );
                            None
                        }
                    }
                } else {
                    None
                };
                let (pixels, backend) = if let Some(image) = jpeg_image {
                    (
                        image,
                        if roll.export.long_edge > 0 {
                            "gpu-srgb-float"
                        } else {
                            "gpu-rgb8"
                        }
                        .into(),
                    )
                } else {
                    let (linear, backend) = render_export(
                        &prepared,
                        settings,
                        &frame.heals,
                        mode == ExportMode::Gpu && !jpeg,
                        &tx,
                        &ctx,
                    )?;
                    (ExportPixels::Linear(linear), backend)
                };
                record(
                    &profile,
                    &serde_json::json!({"stage":"export-render", "frame":frame.id,
                    "backend":backend, "elapsed_ms":now.elapsed().as_secs_f64()*1000.0}),
                );
                encode_tx.send(EncodeJob {
                    id: frame.id.clone(),
                    name: frame.name.clone(),
                    pixels,
                    pending,
                    path,
                    dng: roll.export.dng,
                    tiff: roll.export.tiff,
                    long_edge: roll.export.long_edge,
                    jpeg_quality: roll.export.jpeg_quality.clamp(1, 100),
                    date: greign_export::CaptureDate::parse(&roll.date)
                        .map_err(anyhow::Error::msg)?,
                })?;
                if mode == ExportMode::SerialCpu {
                    outcomes.push(result_rx.recv()?);
                }
                Ok(())
            })();
            if let Err(e) = result {
                failed_ids.insert(frame.id.clone());
                failures.push(ExportFailure {
                    name: frame.name.clone(),
                    error: e.to_string(),
                });
            }
            if mode == ExportMode::SerialCpu {
                let _ = advance_tx.send(());
            }
        }
        drop(source_rx);
        drop(advance_tx);
        let _ = loader.join(); // Channel errors already become per-frame failures.
        drop(encode_tx);
        let compression_result = encoder.join();
        let encoder_ok = compression_result.is_ok();
        let bytes = compression_result.unwrap_or(0);
        let mut finished = HashSet::new();
        for (id, name, result) in outcomes.into_iter().chain(result_rx) {
            finished.insert(id.clone());
            if let Err(error) = result {
                failed_ids.insert(id);
                failures.push(ExportFailure { name, error });
            }
        }
        if !encoder_ok {
            for (_, frame) in &frames {
                if !finished.contains(&frame.id) && failed_ids.insert(frame.id.clone()) {
                    failures.push(ExportFailure {
                        name: frame.name.clone(),
                        error: "Export encoder stopped unexpectedly".into(),
                    });
                }
            }
        }
        record(
            &profile,
            &serde_json::json!({"stage":"roll-export","frames":roll.frames.len(),"elapsed_ms":start.elapsed().as_secs_f64()*1000.0,"failures":failures.len()}),
        );
        let retry = (!failed_ids.is_empty()).then(|| {
            Box::new(ExportRetry {
                roll,
                directory: directory.clone(),
                ids: failed_ids,
            })
        });
        let _ = tx.send(Event::ExportProgress {
            completed: total,
            total,
            bytes,
            elapsed: start.elapsed().as_secs_f64(),
        });
        let _ = tx.send(Event::Exported(directory, failures, retry));
        let _ = tx.send(Event::Finished);
        ctx.request_repaint();
    });
}
fn request_jpeg(
    prepared: &Arc<film::Prepared>,
    settings: greign_gl_preview::Settings,
    float: bool,
    tx: &Sender<Event>,
    ctx: &eframe::egui::Context,
) -> anyhow::Result<ExportPixels> {
    if float {
        let (reply, receive) = std::sync::mpsc::sync_channel(1);
        tx.send(Event::JpegFloatRender {
            prepared: prepared.clone(),
            settings,
            reply,
        })?;
        ctx.request_repaint();
        receive
            .recv_timeout(std::time::Duration::from_secs(30))?
            .map(ExportPixels::Srgb)
            .map_err(anyhow::Error::msg)
    } else {
        let (reply, receive) = std::sync::mpsc::sync_channel(1);
        tx.send(Event::JpegRender {
            prepared: prepared.clone(),
            settings,
            reply,
        })?;
        ctx.request_repaint();
        receive
            .recv_timeout(std::time::Duration::from_secs(30))?
            .map(ExportPixels::Jpeg)
            .map_err(anyhow::Error::msg)
    }
}

fn render_export(
    prepared: &Arc<film::Prepared>,
    settings: greign_gl_preview::Settings,
    heals: &[film::Heal],
    gpu: bool,
    tx: &Sender<Event>,
    ctx: &eframe::egui::Context,
) -> anyhow::Result<(greign_pipeline::LinearRgbImage, String)> {
    let mut backend = if heals.is_empty() {
        "cpu"
    } else {
        "cpu-healing"
    }
    .to_owned();
    if gpu && heals.is_empty() {
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        tx.send(Event::ExportRender {
            prepared: prepared.clone(),
            settings,
            reply,
        })?;
        ctx.request_repaint();
        match receiver.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(Ok(image)) => return Ok((image, "gpu-float32".into())),
            result => {
                backend = format!(
                    "cpu-fallback: {}",
                    match result {
                        Ok(Err(error)) => error,
                        Err(error) => error.to_string(),
                        Ok(Ok(_)) => unreachable!(),
                    }
                );
            }
        }
    }
    Ok((
        film::finish(
            prepared,
            settings.edits,
            settings.crop,
            settings.rotation,
            heals,
            &|| false,
        )?,
        backend,
    ))
}

fn prepare_export_source(
    frame: &crate::model::Frame,
    material: film::Material,
    analysis: Option<&Analysis>,
    demo: bool,
    profile: &Path,
) -> anyhow::Result<Arc<film::Prepared>> {
    let now = Instant::now();
    let scan = film::load_scan(&frame.path, None, demo)?;
    record(
        profile,
        &serde_json::json!({"stage":"export-decode","frame":frame.id,
        "elapsed_ms":now.elapsed().as_secs_f64()*1000.}),
    );
    let now = Instant::now();
    let prepared = Arc::new(film::prepare_owned(
        scan,
        material,
        analysis,
        false,
        &|| false,
    )?);
    record(
        profile,
        &serde_json::json!({"stage":"export-prepare","frame":frame.id,
        "elapsed_ms":now.elapsed().as_secs_f64()*1000.}),
    );
    Ok(prepared)
}

struct EncodeJob {
    id: String,
    name: String,
    pixels: ExportPixels,
    pending: PathBuf,
    path: PathBuf,
    dng: bool,
    tiff: bool,
    long_edge: u32,
    jpeg_quality: u8,
    date: Option<greign_export::CaptureDate>,
}

enum ExportPixels {
    Linear(greign_pipeline::LinearRgbImage),
    Srgb(greign_pipeline::SrgbImage),
    Jpeg(image::RgbImage),
}

fn encode_image(job: EncodeJob) -> anyhow::Result<()> {
    let (linear, srgb) = match job.pixels {
        ExportPixels::Linear(linear) => (Some(linear), None),
        ExportPixels::Srgb(image) => (None, Some(image)),
        ExportPixels::Jpeg(image) => {
            anyhow::ensure!(!job.dng && !job.tiff, "RGB8 is only valid for JPEG export");
            anyhow::ensure!(
                job.long_edge == 0,
                "Resized JPEGs require floating-point pixels"
            );
            greign_export::export_rgb8_jpeg_fast_dated(
                image.width(),
                image.height(),
                image.as_raw(),
                &job.pending,
                job.jpeg_quality,
                job.date.as_ref(),
            )?;
            fs::rename(job.pending, job.path)?;
            return Ok(());
        }
    };
    if job.dng {
        greign_export::export_linear_dng_dated(
            &linear.ok_or_else(|| anyhow::anyhow!("DNG requires linear pixels"))?,
            &job.pending,
            job.date.as_ref(),
        )?;
    } else {
        let mut rgb = srgb.unwrap_or_else(|| greign_pipeline::rec2020_to_srgb(linear.unwrap()));
        if job.long_edge > 0 && rgb.width.max(rgb.height) > job.long_edge {
            let image = image::Rgb32FImage::from_raw(
                rgb.width,
                rgb.height,
                rgb.pixels.iter().flatten().copied().collect(),
            )
            .ok_or_else(|| anyhow::anyhow!("Invalid rendered image dimensions"))?;
            let scale = f64::from(job.long_edge) / f64::from(rgb.width.max(rgb.height));
            let resized = image::imageops::resize(
                &image,
                (f64::from(rgb.width) * scale).round().max(1.0) as u32,
                (f64::from(rgb.height) * scale).round().max(1.0) as u32,
                image::imageops::FilterType::Lanczos3,
            );
            rgb.width = resized.width();
            rgb.height = resized.height();
            rgb.pixels = resized.pixels().map(|p| p.0).collect();
        }
        if job.tiff {
            greign_export::export_srgb_dated(
                &rgb,
                &job.pending,
                greign_export::ExportOptions::default(),
                job.date.as_ref(),
            )?;
        } else {
            greign_export::export_rgb8_jpeg_fast_dated(
                rgb.width,
                rgb.height,
                &rgb.to_rgb8(),
                &job.pending,
                job.jpeg_quality,
                job.date.as_ref(),
            )?;
        }
    }
    fs::rename(job.pending, job.path)?;
    Ok(())
}

pub fn record(path: &Path, value: &serde_json::Value) {
    use std::io::Write;
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(format!("{value}\n").as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Frame;
    use std::sync::mpsc;
    use std::time::Duration;

    fn fixture(image: greign_pipeline::LinearRgbImage) -> Arc<film::Prepared> {
        Arc::new(
            film::prepare(
                &film::Scan {
                    image,
                    camera_matrix: None,
                    white_balance: [1.; 3],
                    capture: "export test".into(),
                },
                film::Material::Positive,
                None,
                false,
                &|| false,
            )
            .unwrap(),
        )
    }

    #[test]
    fn jpeg_gpu_dispatch_fallback_quality_resize_and_completed_progress() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.png");
        image::RgbImage::from_fn(64, 48, |x, y| {
            image::Rgb([(x * 4) as u8, (y * 5) as u8, 90])
        })
        .save(&path)
        .unwrap();
        let mut roll = Roll::new("JPEG test");
        roll.material = film::Material::Positive;
        roll.frames.push(Frame::new(path));
        roll.export.jpeg_quality = 73;
        for (edge, fail_gpu) in [(0, false), (32, false), (32, true)] {
            roll.export.long_edge = edge;
            let output = dir.path().join(format!("{edge}-{fail_gpu}"));
            fs::create_dir_all(&output).unwrap();
            let (tx, rx) = mpsc::channel();
            export_with_mode(
                roll.clone(),
                output.clone(),
                None,
                tx,
                eframe::egui::Context::default(),
                dir.path().join("profile.jsonl"),
                ExportMode::Gpu,
            );
            let mut requests = 0;
            let mut saved_bytes = 0;
            loop {
                match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
                    Event::JpegFloatRender {
                        prepared,
                        settings,
                        reply,
                    } => {
                        requests += 1;
                        if fail_gpu {
                            reply.send(Err("test GPU unavailable".into())).unwrap();
                        } else {
                            let linear = film::finish(
                                &prepared,
                                settings.edits,
                                settings.crop,
                                settings.rotation,
                                &[],
                                &|| false,
                            )
                            .unwrap();
                            let rgb = greign_pipeline::rec2020_to_srgb(linear);
                            let floats = image::Rgb32FImage::from_raw(
                                rgb.width,
                                rgb.height,
                                rgb.pixels.iter().flatten().copied().collect(),
                            )
                            .unwrap();
                            let resized = image::imageops::resize(
                                &floats,
                                32,
                                24,
                                image::imageops::FilterType::Lanczos3,
                            );
                            let expected = greign_pipeline::SrgbImage {
                                width: 32,
                                height: 24,
                                pixels: resized.pixels().map(|p| p.0).collect(),
                            };
                            greign_export::export_rgb8_jpeg_fast(
                                32,
                                24,
                                &expected.to_rgb8(),
                                &dir.path().join("expected.jpg"),
                                73,
                            )
                            .unwrap();
                            reply.send(Ok(rgb)).unwrap();
                        }
                    }
                    Event::JpegRender {
                        prepared,
                        settings,
                        reply,
                    } => {
                        requests += 1;
                        if fail_gpu {
                            reply.send(Err("test GPU unavailable".into())).unwrap();
                        } else {
                            let linear = film::finish(
                                &prepared,
                                settings.edits,
                                settings.crop,
                                settings.rotation,
                                &[],
                                &|| false,
                            )
                            .unwrap();
                            let rgb = greign_pipeline::rec2020_to_srgb(linear);
                            let image =
                                image::RgbImage::from_raw(rgb.width, rgb.height, rgb.to_rgb8())
                                    .unwrap();
                            greign_export::export_rgb8_jpeg_fast(
                                image.width(),
                                image.height(),
                                image.as_raw(),
                                &dir.path().join("expected.jpg"),
                                73,
                            )
                            .unwrap();
                            reply.send(Ok(image)).unwrap();
                        }
                    }
                    Event::ExportProgress {
                        completed,
                        total,
                        bytes,
                        ..
                    } => {
                        assert_eq!(total, 1);
                        assert!(completed <= total);
                        saved_bytes = bytes;
                    }
                    Event::Exported(_, errors, _) => assert!(errors.is_empty()),
                    Event::ExportRender { .. } => {
                        panic!("JPEG fallback should use CPU, not another GPU request")
                    }
                    Event::Finished => break,
                    _ => {}
                }
            }
            assert_eq!(requests, 1);
            let actual = output.join("01-source-positive.jpg");
            let decoded = image::open(&actual).unwrap();
            assert_eq!(
                (decoded.width(), decoded.height()),
                if edge == 0 { (64, 48) } else { (32, 24) }
            );
            assert_eq!(saved_bytes, fs::metadata(&actual).unwrap().len());
            if !fail_gpu {
                assert_eq!(
                    fs::read(actual).unwrap(),
                    fs::read(dir.path().join("expected.jpg")).unwrap()
                );
            }
        }
    }

    #[test]
    fn gpu_export_failure_uses_identical_cpu_reference() {
        let prepared = fixture(greign_pipeline::LinearRgbImage {
            width: 2,
            height: 2,
            pixels: vec![[0.2, 0.3, 0.4]; 4],
        });
        let settings = greign_gl_preview::Settings {
            edits: film::Adjustments::default(),
            material: film::Material::Positive,
            crop: film::Rect::default(),
            rotation: 1,
        };
        let expected = film::finish(
            &prepared,
            settings.edits,
            settings.crop,
            settings.rotation,
            &[],
            &|| false,
        )
        .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            render_export(
                &prepared,
                settings,
                &[],
                true,
                &tx,
                &eframe::egui::Context::default(),
            )
            .unwrap()
        });
        let Event::ExportRender { reply, .. } = rx.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("Missing GPU request")
        };
        reply
            .send(Err("simulated GPU allocation failure".into()))
            .unwrap();
        let (actual, backend) = worker.join().unwrap();
        assert_eq!(actual.pixels, expected.pixels);
        assert_eq!(
            (actual.width, actual.height),
            (expected.width, expected.height)
        );
        assert!(backend.contains("simulated GPU allocation failure"));
    }

    #[test]
    fn healed_export_never_requests_gpu_or_drops_repairs() {
        let prepared = fixture(greign_pipeline::LinearRgbImage {
            width: 32,
            height: 32,
            pixels: (0..1024).map(|i| [i as f32 / 1024.; 3]).collect(),
        });
        let settings = greign_gl_preview::Settings {
            edits: film::Adjustments::default(),
            material: film::Material::Positive,
            crop: film::Rect::default(),
            rotation: 0,
        };
        let heals = [film::Heal {
            target: [0.3, 0.3],
            source: [0.7, 0.7],
            radius: 0.05,
        }];
        let expected = film::finish(&prepared, settings.edits, settings.crop, 0, &heals, &|| {
            false
        })
        .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (actual, backend) = render_export(
            &prepared,
            settings,
            &heals,
            true,
            &tx,
            &eframe::egui::Context::default(),
        )
        .unwrap();
        assert_eq!(actual.pixels, expected.pixels);
        assert_eq!(backend, "cpu-healing");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn selected_dng_export_skips_other_sources_and_ignores_proof_resize() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.png");
        image::RgbImage::from_pixel(64, 48, image::Rgb([100, 120, 140]))
            .save(&path)
            .unwrap();
        let original = fs::read(&path).unwrap();
        let mut roll = Roll::new("DNG subset");
        roll.date = "1998-07-03".into();
        roll.material = film::Material::Positive;
        roll.frames = vec![
            Frame::new(dir.path().join("missing.png")),
            Frame::new(path.clone()),
        ];
        roll.export.dng = true;
        roll.export.long_edge = 8;
        let ids = HashSet::from([roll.frames[1].id.clone()]);
        let out = dir.path().join("out");
        fs::create_dir(&out).unwrap();
        let (tx, rx) = mpsc::channel();
        export(
            roll,
            out.clone(),
            Some(ids),
            tx,
            eframe::egui::Context::default(),
            dir.path().join("timings.jsonl"),
        );
        loop {
            match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
                Event::Exported(_, errors, _) => assert!(errors.is_empty(), "{errors:?}"),
                Event::Finished => break,
                _ => {}
            }
        }
        let files: Vec<_> = fs::read_dir(out)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name().unwrap(), "02-source-positive.dng");
        assert!(fs::metadata(&files[0]).unwrap().len() > 64 * 48 * 3 * 4);
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn failed_only_retry_uses_frozen_settings_and_does_not_duplicate_successes() {
        let dir = tempfile::tempdir().unwrap();
        let one = dir.path().join("one.png");
        let two = dir.path().join("two.png");
        image::RgbImage::from_pixel(64, 48, image::Rgb([80, 120, 160]))
            .save(&one)
            .unwrap();
        let mut roll = Roll::new("Retry");
        roll.material = film::Material::Positive;
        roll.frames = vec![Frame::new(one.clone()), Frame::new(two.clone())];
        roll.export.long_edge = 32;
        roll.export.prefix = "proof".into();
        roll.frames[0].adjustments.highlights = -40.0;
        roll.frames[0].adjustments.colors[5].saturation = -50.0;
        let out = dir.path().join("export");
        fs::create_dir(&out).unwrap();
        let (tx, rx) = mpsc::channel();
        let ctx = eframe::egui::Context::default();
        export(
            roll,
            out.clone(),
            None,
            tx.clone(),
            ctx.clone(),
            dir.path().join("timings.jsonl"),
        );
        let mut retry = None;
        loop {
            match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
                Event::Exported(_, errors, state) => {
                    assert_eq!(errors.len(), 1);
                    retry = state;
                }
                Event::Finished => break,
                _ => {}
            }
        }
        let saved = out.join("proof-01-one-positive.jpg");
        let bytes = fs::read(&saved).unwrap();
        let image = image::open(&saved).unwrap();
        assert_eq!((image.width(), image.height()), (32, 24));
        fs::copy(one, two).unwrap();
        let retry = retry.unwrap();
        assert_eq!(retry.ids.len(), 1);
        export(
            retry.roll,
            retry.directory,
            Some(retry.ids),
            tx,
            ctx,
            dir.path().join("timings.jsonl"),
        );
        loop {
            match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
                Event::Exported(_, errors, state) => {
                    assert!(errors.is_empty());
                    assert!(state.is_none());
                }
                Event::Finished => break,
                _ => {}
            }
        }
        assert_eq!(fs::read(saved).unwrap(), bytes);
        assert_eq!(fs::read_dir(out).unwrap().count(), 2);
    }

    #[test]
    fn background_analysis_and_exports_preserve_sources_and_skip_rejected_frames() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("negative.png");
        image::RgbImage::from_fn(64, 48, |x, y| {
            let offset = if x < 4 || y < 4 || x > 59 || y > 43 {
                0
            } else {
                (x * 2) as u8
            };
            image::Rgb([220 - offset, 180 - offset, 150 - offset])
        })
        .save(&path)
        .unwrap();
        let original = fs::read(&path).unwrap();
        let mut roll = Roll::new("Integration test");
        roll.date = "1998-07-03".into();
        roll.frames.push(Frame::new(path.clone()));
        let mut rejected = Frame::new(dir.path().join("does-not-exist.CR3"));
        rejected.rejected = true;
        roll.frames.push(rejected);
        let (tx, rx) = mpsc::channel();
        let ctx = eframe::egui::Context::default();
        load(
            roll.clone(),
            true,
            tx.clone(),
            ctx.clone(),
            dir.path().join("timings.jsonl"),
        );
        loop {
            match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
                Event::Analysis(_, result) => roll.analysis = Some(result.unwrap()),
                Event::Finished => break,
                _ => {}
            }
        }
        assert!(roll.analysis.is_some());
        for tiff in [false, true] {
            roll.export.tiff = tiff;
            let out = dir.path().join(if tiff { "tiff" } else { "jpeg" });
            fs::create_dir_all(&out).unwrap();
            export(
                roll.clone(),
                out.clone(),
                None,
                tx.clone(),
                ctx.clone(),
                dir.path().join("timings.jsonl"),
            );
            let mut completed = false;
            loop {
                match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
                    Event::Exported(_, errors, _) => {
                        assert!(errors.is_empty(), "{errors:?}");
                        completed = true;
                    }
                    Event::Finished => break,
                    _ => {}
                }
            }
            assert!(completed);
            let files: Vec<_> = fs::read_dir(out)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            assert_eq!(files.len(), 1);
            assert!(
                fs::read(&files[0])
                    .unwrap()
                    .windows(19)
                    .any(|b| b == b"1998:07:03 12:00:00")
            );
            let image = image::ImageReader::open(&files[0])
                .unwrap()
                .decode()
                .unwrap();
            assert_eq!((image.width(), image.height()), (64, 48));
            if tiff {
                assert_eq!(image.color(), image::ColorType::Rgb16);
            }
        }
        assert_eq!(fs::read(path).unwrap(), original);
    }
}
