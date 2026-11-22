//! Latest-request-wins preview lanes. Full-resolution I/O never blocks drafts.
use crate::{jobs, model::Frame};
use eframe::egui;
use greign_film::{self as film, Analysis, Material, Scan};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Draft,
    Preview,
    Full,
}
#[derive(Clone)]
pub struct Request {
    pub key: String,
    pub scene_key: String,
    pub frame: Frame,
    pub scan: Arc<Scan>,
    pub material: Material,
    pub analysis: Option<Analysis>,
    pub edits: film::Adjustments,
    pub negative: bool,
    pub demo: bool,
    pub quality: Quality,
    pub gpu: bool,
}
pub struct GpuScene {
    pub key: String,
    pub prepared: Arc<film::Prepared>,
}
pub struct Output {
    pub id: String,
    pub key: String,
    pub quality: Quality,
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
    pub error: Option<String>,
}
struct Mailbox {
    pending: Option<(u64, Vec<Request>)>,
    stop: bool,
}
struct Lane {
    state: Arc<(Mutex<Mailbox>, Condvar)>,
    epoch: Arc<AtomicU64>,
}
pub struct Renderer {
    fast: Lane,
    full: Lane,
}
impl Renderer {
    pub fn new(tx: mpsc::Sender<jobs::Event>, ctx: egui::Context, profile: PathBuf) -> Self {
        Self {
            fast: Lane::new(tx.clone(), ctx.clone(), profile.clone(), false),
            full: Lane::new(tx, ctx, profile, true),
        }
    }
    pub fn submit(&self, requests: Vec<Request>, full: bool) {
        if full { &self.full } else { &self.fast }.submit(requests);
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        for lane in [&self.fast, &self.full] {
            lane.epoch.fetch_add(1, Ordering::Relaxed);
            lane.state.0.lock().unwrap().stop = true;
            lane.state.1.notify_one();
        }
    }
}
impl Lane {
    fn submit(&self, requests: Vec<Request>) {
        let generation = self.epoch.fetch_add(1, Ordering::Relaxed) + 1;
        self.state.0.lock().unwrap().pending = Some((generation, requests));
        self.state.1.notify_one();
    }
    fn new(
        tx: mpsc::Sender<jobs::Event>,
        ctx: egui::Context,
        profile: PathBuf,
        full: bool,
    ) -> Self {
        let state = Arc::new((
            Mutex::new(Mailbox {
                pending: None,
                stop: false,
            }),
            Condvar::new(),
        ));
        let epoch = Arc::new(AtomicU64::new(0));
        let shared = state.clone();
        let latest = epoch.clone();
        std::thread::spawn(move || {
            // Bounded conversion caches; full lane retains only one source/scene.
            let mut prepared: VecDeque<(String, Arc<film::Prepared>)> = VecDeque::new();
            let mut source: Option<(String, Scan)> = None;
            loop {
                let (generation, requests) = {
                    let mut mailbox = shared.0.lock().unwrap();
                    while mailbox.pending.is_none() && !mailbox.stop {
                        mailbox = shared.1.wait(mailbox).unwrap();
                    }
                    if mailbox.stop {
                        return;
                    }
                    mailbox.pending.take().unwrap()
                };
                let cancelled = || latest.load(Ordering::Relaxed) != generation;
                for request in requests {
                    if cancelled() {
                        break;
                    }
                    let start = Instant::now();
                    let cached = prepared.iter().any(|(key, _)| *key == request.scene_key);
                    let result = (|| -> anyhow::Result<_> {
                        if !cached {
                            if full {
                                let source_key = format!(
                                    "{}:{:p}",
                                    request.frame.id,
                                    Arc::as_ptr(&request.scan)
                                );
                                if source.as_ref().is_none_or(|(key, _)| *key != source_key) {
                                    source = None;
                                    prepared.clear();
                                    let loaded =
                                        film::load_scan(&request.frame.path, None, request.demo)?;
                                    source = Some((source_key, loaded));
                                }
                            }
                            anyhow::ensure!(!cancelled(), "Preview superseded");
                            let small;
                            let scan = if full {
                                &source.as_ref().unwrap().1
                            } else if request.quality == Quality::Draft {
                                small = downsample(&request.scan, 480)?;
                                &small
                            } else {
                                &request.scan
                            };
                            let image = film::prepare(
                                scan,
                                request.material,
                                request.analysis.as_ref(),
                                request.negative,
                                &cancelled,
                            )?;
                            while prepared.len() >= if full { 1 } else { 24 } {
                                prepared.pop_front();
                            }
                            prepared.push_back((request.scene_key.clone(), Arc::new(image)));
                        }
                        let index = prepared
                            .iter()
                            .position(|(key, _)| *key == request.scene_key)
                            .unwrap();
                        let item = prepared.remove(index).unwrap();
                        prepared.push_back(item);
                        let base = &prepared.back().unwrap().1;
                        if request.gpu {
                            anyhow::ensure!(
                                request.frame.heals.is_empty() && !request.negative,
                                "GPU preparation does not support healing or original-negative display"
                            );
                            anyhow::ensure!(!cancelled(), "Preview superseded");
                            let _ = tx.send(jobs::Event::GpuScene(GpuScene {
                                key: request.key.clone(),
                                prepared: base.clone(),
                            }));
                            ctx.request_repaint();
                            return Ok(None);
                        }
                        let rgb = film::finish(
                            base,
                            request.edits,
                            if request.negative {
                                film::Rect::default()
                            } else {
                                request.frame.crop
                            },
                            if request.negative {
                                0
                            } else {
                                request.frame.rotation
                            },
                            !request.negative && request.frame.mirrored,
                            &request.frame.heals,
                            &cancelled,
                        )?;
                        anyhow::ensure!(!cancelled(), "Preview superseded");
                        let output = greign_pipeline::rec2020_to_srgb(rgb);
                        anyhow::ensure!(!cancelled(), "Preview superseded");
                        Ok(Some((output.width, output.height, output.to_rgb8())))
                    })();
                    jobs::record(
                        &profile,
                        &serde_json::json!({"stage":"interactive-render","quality":format!("{:?}",request.quality),"gpu_source_only":request.gpu,"cached_conversion":cached,"cancelled":cancelled(),"elapsed_ms":start.elapsed().as_secs_f64()*1000.0}),
                    );
                    if cancelled() {
                        break;
                    }
                    let (width, height, bytes, error) = match result {
                        Ok(Some((w, h, b))) => (w, h, b, None),
                        Ok(None) => continue,
                        Err(e) => (0, 0, vec![], Some(e.to_string())),
                    };
                    if tx
                        .send(jobs::Event::PreviewReady(Box::new(Output {
                            id: request.frame.id,
                            key: request.key,
                            quality: request.quality,
                            width,
                            height,
                            bytes,
                            error,
                        })))
                        .is_err()
                    {
                        return;
                    }
                    ctx.request_repaint();
                }
            }
        });
        Self { state, epoch }
    }
}
fn downsample(scan: &Scan, size: u32) -> anyhow::Result<Scan> {
    let ratio = f64::from(size) / f64::from(scan.image.width.max(scan.image.height));
    if ratio >= 1.0 {
        return Ok(scan.clone());
    }
    let image = image::Rgb32FImage::from_raw(
        scan.image.width,
        scan.image.height,
        scan.image.pixels.iter().flatten().copied().collect(),
    )
    .ok_or_else(|| anyhow::anyhow!("Invalid preview dimensions"))?;
    let resized = image::imageops::resize(
        &image,
        (f64::from(scan.image.width) * ratio).round().max(1.0) as u32,
        (f64::from(scan.image.height) * ratio).round().max(1.0) as u32,
        image::imageops::FilterType::Triangle,
    );
    Ok(Scan {
        image: greign_pipeline::LinearRgbImage {
            width: resized.width(),
            height: resized.height(),
            pixels: resized.pixels().map(|p| p.0).collect(),
        },
        camera_matrix: scan.camera_matrix,
        white_balance: scan.white_balance,
        capture: scan.capture.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn gpu_scene_delivery_reuses_conversion_without_baking_slider_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.png");
        image::RgbImage::from_pixel(64, 48, image::Rgb([90, 130, 170]))
            .save(&path)
            .unwrap();
        let scan = Arc::new(film::load_scan(&path, Some(960), false).unwrap());
        let (tx, rx) = mpsc::channel();
        let renderer = Renderer::new(
            tx,
            egui::Context::default(),
            dir.path().join("profile.jsonl"),
        );
        let mut request = Request {
            key: "scene".into(),
            scene_key: "scene".into(),
            frame: Frame::new(path),
            scan,
            material: Material::Positive,
            analysis: None,
            edits: film::Adjustments::default(),
            negative: false,
            demo: false,
            quality: Quality::Full,
            gpu: true,
        };
        renderer.submit(vec![request.clone()], true);
        let jobs::Event::GpuScene(first) = rx.recv_timeout(Duration::from_secs(10)).unwrap() else {
            panic!("Expected GPU source");
        };
        request.edits.highlights = -80.;
        request.edits.brightness = 2.;
        renderer.submit(vec![request], true);
        let jobs::Event::GpuScene(second) = rx.recv_timeout(Duration::from_secs(10)).unwrap()
        else {
            panic!("Expected GPU source");
        };
        assert!(Arc::ptr_eq(&first.prepared, &second.prepared));
        assert_eq!(
            (second.prepared.image.width, second.prepared.image.height),
            (64, 48)
        );
    }

    fn receive(rx: &mpsc::Receiver<jobs::Event>, key: &str) -> Output {
        loop {
            if let jobs::Event::PreviewReady(output) =
                rx.recv_timeout(Duration::from_secs(30)).unwrap()
            {
                if output.key == key {
                    assert!(output.error.is_none(), "{:?}", output.error);
                    return *output;
                }
            }
        }
    }
    #[test]
    fn latest_draft_and_full_resolution_use_separate_lanes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frame.png");
        image::RgbImage::from_pixel(1200, 800, image::Rgb([70, 100, 130]))
            .save(&path)
            .unwrap();
        let scan = Arc::new(film::load_scan(&path, Some(960), false).unwrap());
        let (tx, rx) = mpsc::channel();
        let renderer = Renderer::new(
            tx,
            egui::Context::default(),
            dir.path().join("timings.jsonl"),
        );
        let request = Request {
            key: "old-full".into(),
            scene_key: "full".into(),
            frame: Frame::new(path),
            scan: scan.clone(),
            material: Material::Positive,
            analysis: None,
            edits: film::Adjustments::default(),
            negative: false,
            demo: false,
            quality: Quality::Full,
            gpu: false,
        };
        renderer.submit(vec![request.clone()], true);
        renderer.submit(vec![], true);
        let mut draft = request.clone();
        draft.quality = Quality::Draft;
        draft.scene_key = "draft".into();
        for i in 0..20 {
            draft.key = format!("draft-{i}");
            draft.edits.shadows = i as f32;
            renderer.submit(vec![draft.clone()], false);
        }
        let output = receive(&rx, "draft-19");
        assert_eq!((output.width, output.height), (480, 320));
        let mut final_request = request;
        final_request.key = "final".into();
        renderer.submit(vec![final_request], true);
        let output = receive(&rx, "final");
        assert_eq!((output.width, output.height), (1200, 800));
        assert_eq!((scan.image.width, scan.image.height), (960, 640));
    }

    #[test]
    #[ignore = "CPU performance run using the private demo; saves dumps and visible proof images"]
    fn profile_interactive_lanes() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let roll = crate::model::seed_demo(&root);
        assert!(!roll.frames.is_empty(), "Local demo images required");
        let scans: Vec<_> = roll
            .frames
            .iter()
            .map(|frame| Arc::new(film::load_scan(&frame.path, Some(960), true).unwrap()))
            .collect();
        let refs: Vec<_> = scans.iter().map(AsRef::as_ref).collect();
        let analysis = film::analyze(&refs, roll.base, roll.analysis_area).unwrap();
        let (tx, rx) = mpsc::channel();
        let directory = root.join("exports/interactive-preview");
        std::fs::create_dir_all(&directory).unwrap();
        let profile = root.join("artifacts/interactive-worker-profile.jsonl");
        let renderer = Renderer::new(tx, egui::Context::default(), profile);
        let mut requests: Vec<_> = roll
            .frames
            .iter()
            .zip(&scans)
            .map(|(frame, scan)| Request {
                key: format!("normal-{}", frame.id),
                scene_key: format!("normal-{}", frame.id),
                frame: frame.clone(),
                scan: scan.clone(),
                material: roll.material,
                analysis: Some(analysis.clone()),
                edits: film::Adjustments::default(),
                negative: false,
                demo: true,
                quality: Quality::Preview,
                gpu: false,
            })
            .collect();
        let start = Instant::now();
        renderer.submit(requests.clone(), false);
        for request in &requests {
            receive(&rx, &request.key);
        }
        let raw = root.join("artifacts/interactive-latency.jsonl");
        jobs::record(
            &raw,
            &serde_json::json!({"stage":"initial-roll","elapsed_ms":start.elapsed().as_secs_f64()*1000.0}),
        );
        let mut draft = requests[0].clone();
        draft.scene_key = "active-draft".into();
        draft.quality = Quality::Draft;
        // Continuous edits; no wait for obsolete requests. Poll on a 16 ms UI cadence.
        for i in 0..60 {
            draft.key = format!("drag-{i}");
            draft.edits.shadows = i as f32;
            let start = Instant::now();
            renderer.submit(vec![draft.clone()], false);
            std::thread::sleep(Duration::from_millis(16));
            let mut shown = false;
            while let Ok(event) = rx.try_recv() {
                if let jobs::Event::PreviewReady(output) = event {
                    if output.key == draft.key {
                        shown = true;
                    }
                }
            }
            jobs::record(
                &raw,
                &serde_json::json!({"stage":"drag-tick","current_preview_ready":shown,"elapsed_ms":start.elapsed().as_secs_f64()*1000.0}),
            );
        }
        draft.key = "final-draft".into();
        renderer.submit(vec![draft.clone()], false);
        let proof = receive(&rx, &draft.key);
        image::save_buffer(
            directory.join("draft.png"),
            &proof.bytes,
            proof.width,
            proof.height,
            image::ColorType::Rgb8,
        )
        .unwrap();
        let mut full = draft;
        full.quality = Quality::Full;
        full.scene_key = "active-full".into();
        full.key = "released-full".into();
        let start = Instant::now();
        renderer.submit(vec![full.clone()], true);
        for request in &mut requests {
            request.edits.shadows = 59.0;
            request.key.push_str("-released");
        }
        renderer.submit(requests.clone(), false);
        let mut received = 0;
        loop {
            if let jobs::Event::PreviewReady(output) =
                rx.recv_timeout(Duration::from_secs(60)).unwrap()
            {
                assert!(output.error.is_none(), "{:?}", output.error);
                jobs::record(
                    &raw,
                    &serde_json::json!({"stage":"release-result","quality":format!("{:?}",output.quality),"elapsed_ms":start.elapsed().as_secs_f64()*1000.0}),
                );
                if output.quality == Quality::Full {
                    image::save_buffer(
                        directory.join("full-resolution.png"),
                        &output.bytes,
                        output.width,
                        output.height,
                        image::ColorType::Rgb8,
                    )
                    .unwrap();
                }
                received += 1;
                if received == requests.len() + 1 {
                    break;
                }
            }
        }
        full.key = "warm-full".into();
        full.edits.shadows = 30.0;
        let mut concurrent = full.clone();
        concurrent.quality = Quality::Draft;
        concurrent.scene_key = "active-draft".into();
        concurrent.key = "draft-during-full".into();
        let start = Instant::now();
        renderer.submit(vec![full], true);
        renderer.submit(vec![concurrent], false);
        let mut received = 0;
        while received < 2 {
            if let jobs::Event::PreviewReady(output) =
                rx.recv_timeout(Duration::from_secs(30)).unwrap()
            {
                assert!(output.error.is_none(), "{:?}", output.error);
                jobs::record(
                    &raw,
                    &serde_json::json!({"stage":"warm-concurrent","quality":format!("{:?}",output.quality),"elapsed_ms":start.elapsed().as_secs_f64()*1000.0}),
                );
                received += 1;
            }
        }
    }
}
