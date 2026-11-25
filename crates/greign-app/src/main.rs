//! Native desktop roll workspace. No web server or browser is involved.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::collapsible_if,
    clippy::too_many_lines
)]
mod diagnostics;
mod export_check;
mod jobs;
mod model;
mod platform;
mod preview;
mod smoke;
mod ui;
mod viewport;
mod workflow;
use eframe::egui::{self, TextureHandle};
use greign_film::{self as film, Adjustments, Material, Scan};
use jobs::Event;
use model::{Library, Roll};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Adjust,
    Conversion,
    Dust,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    View,
    Heal,
    Base,
    Crop,
    Neutral,
}
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ExportScope {
    #[default]
    Roll,
    Selected,
    Frame,
    AllReady,
}
#[derive(Clone, Copy)]
enum Action {
    Import,
    ImportFolder,
    Load,
    Analyze,
    AnalyzeFrame(usize),
    SampleBase,
    SetBase {
        frame: Option<usize>,
        base: Option<[f32; 3]>,
    },
    PreviewFaded,
    Crop,
    AutoCrop,
    AutoCropAll,
    AutoUpright,
    ResetCrop,
    SetIndividual(bool),
    Mirror,
    MirrorAll(bool),
    OrientationToRoll,
    Export,
    Undo,
    Redo,
    Reset,
    Rotate,
    RotateBack,
    Reject,
    Detail,
    RestoreAnalysis,
    OpenExport,
    CopyEdits,
    PasteEdits,
    SaveVersion,
    RestoreVersion,
    AcceptAnalysis,
    DiscardAnalysis,
    RetryExport,
}

// Independent view/tool/dialog toggles, not a single mutually exclusive state.
#[allow(clippy::struct_excessive_bools)]
struct App {
    root: PathBuf,
    library_path: PathBuf,
    library: Library,
    roll: usize,
    frame: usize,
    grid: bool,
    scope_roll: bool,
    scope_selected: bool,
    selected: HashSet<String>,
    clipboard: Option<model::Frame>,
    paste_options: model::PasteOptions,
    version_name: String,
    version_index: usize,
    comparing: bool,
    compare_button: bool,
    pending_analysis: Option<model::ProposedConversion>,
    requested_preview: Option<String>,
    preview_analysis: bool,
    neutral_analysis: bool,
    export_retry: VecDeque<Box<jobs::ExportRetry>>,
    export_batch: Option<workflow::ExportBatch>,
    panel: Panel,
    tool: Tool,
    show_negative: bool,
    histogram_key: String,
    histogram: Option<Box<diagnostics::Histogram>>,
    histogram_pending: bool,
    histogram_error: Option<String>,
    zoom: bool,
    reference: Option<String>,
    dust_radius: f32,
    selected_heal: Option<usize>,
    move_source: bool,
    drag_start: Option<egui::Pos2>,
    crop_return_negative: bool,
    cache: HashMap<String, Arc<Scan>>,
    renderer: preview::Renderer,
    gpu: Option<Arc<std::sync::Mutex<greign_gl_preview::Preview>>>,
    gpu_pending: Option<preview::GpuScene>,
    desired: HashMap<String, String>,
    completed: HashMap<String, String>,
    full_desired: Option<String>,
    detail_texture: Option<(String, String, TextureHandle)>,
    fast_plan: String,
    full_plan: String,
    scheduled: Option<(u64, usize, usize, bool, bool)>,
    dragging: bool,
    refine: bool,
    view: viewport::View,
    textures: HashMap<String, TextureHandle>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    busy: bool,
    status: String,
    revision: u64,
    rendering: bool,
    dirty: bool,
    history: Vec<Library>,
    redo: Vec<Library>,
    gesture: Option<Library>,
    actions: Vec<Action>,
    metadata_open: bool,
    new_roll_open: bool,
    new_name: String,
    new_paths: Vec<PathBuf>,
    new_folder: Option<PathBuf>,
    new_import_error: Option<String>,
    new_stock: String,
    new_material: Material,
    export_dir: PathBuf,
    export_dialog: bool,
    export_scope: ExportScope,
    last_export: Option<PathBuf>,
    export_progress: Option<(usize, usize, u64, f64)>,
    save_error: Option<String>,
    screenshot: Option<PathBuf>,
    screenshot_requested: bool,
    gpu_ui_check: bool,
    gpu_ui_stage: u8,
    start: Instant,
    smoke: Option<smoke::Replay>,
}
impl App {
    fn new(
        ctx: &egui::Context,
        root: PathBuf,
        mut library: Library,
        screenshot: Option<PathBuf>,
    ) -> Self {
        ui::style(ctx);
        for roll in &mut library.rolls {
            if roll.versions.is_empty() && roll.analysis.is_some() {
                roll.save_version("Opening look");
            }
        }
        let (tx, rx) = mpsc::channel();
        let renderer =
            preview::Renderer::new(tx.clone(), ctx.clone(), root.join(".greign/timings.jsonl"));
        let mut app = Self {
            library_path: std::env::var_os("GREIGN_LIBRARY_PATH")
                .map_or_else(|| root.join(".greign/rolls.json"), PathBuf::from),
            export_dir: platform::export_directory(&root),
            root,
            library,
            roll: 0,
            frame: 0,
            grid: true,
            scope_roll: true,
            scope_selected: false,
            selected: HashSet::new(),
            clipboard: None,
            paste_options: model::PasteOptions::default(),
            version_name: String::new(),
            version_index: 0,
            comparing: false,
            compare_button: false,
            pending_analysis: None,
            requested_preview: None,
            preview_analysis: false,
            neutral_analysis: true,
            export_retry: VecDeque::new(),
            export_batch: None,
            panel: Panel::Adjust,
            tool: Tool::View,
            show_negative: false,
            histogram_key: String::new(),
            histogram: None,
            histogram_pending: false,
            histogram_error: None,
            zoom: false,
            reference: None,
            dust_radius: 0.012,
            selected_heal: None,
            move_source: false,
            drag_start: None,
            crop_return_negative: false,
            cache: HashMap::new(),
            renderer,
            gpu: None,
            gpu_pending: None,
            desired: HashMap::new(),
            completed: HashMap::new(),
            full_desired: None,
            detail_texture: None,
            fast_plan: String::new(),
            full_plan: String::new(),
            scheduled: None,
            dragging: false,
            refine: false,
            view: viewport::View::default(),
            textures: HashMap::new(),
            tx,
            rx,
            busy: false,
            status: "Ready".into(),
            revision: 1,
            rendering: false,
            dirty: false,
            history: Vec::new(),
            redo: Vec::new(),
            gesture: None,
            actions: Vec::new(),
            metadata_open: false,
            new_roll_open: false,
            new_name: String::new(),
            new_paths: Vec::new(),
            new_folder: None,
            new_import_error: None,
            new_stock: String::new(),
            new_material: Material::default(),
            export_dialog: false,
            export_scope: ExportScope::Roll,
            last_export: None,
            export_progress: None,
            save_error: None,
            screenshot,
            screenshot_requested: false,
            gpu_ui_check: false,
            gpu_ui_stage: 0,
            start: Instant::now(),
            smoke: None,
        };
        if !app.library.rolls.is_empty() {
            app.begin_load(
                ctx,
                app.library.rolls[0].demo && app.library.rolls[0].analysis.is_none(),
            );
        }
        app
    }
    fn profile(&self) -> PathBuf {
        self.root.join(".greign/timings.jsonl")
    }
    fn begin_load(&mut self, ctx: &egui::Context, analyze: bool) {
        if self.busy {
            return;
        }
        if let Some(roll) = self.library.rolls.get(self.roll).cloned() {
            self.export_progress = None;
            self.busy = true;
            self.status = if analyze {
                "Analyzing roll…"
            } else {
                "Loading previews…"
            }
            .into();
            jobs::load(roll, analyze, self.tx.clone(), ctx.clone(), self.profile());
        }
    }
    fn invalidate(&mut self) {
        self.revision += 1;
    }
    fn save(&mut self) {
        self.save_error = model::save(&self.library_path, &self.library)
            .err()
            .map(|e| e.to_string());
    }
    fn remember(&mut self, before: Library) {
        self.history.push(before);
        if self.history.len() > 64 {
            self.history.remove(0);
        }
        self.redo.clear();
    }
    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::AutoOriented(id, result) => {
                    let batch = match result {
                        Ok(results) => results,
                        Err(error) => {
                            self.status = format!("Automatic upright detection failed: {error}");
                            continue;
                        }
                    };
                    let before = self.library.clone();
                    let Some(roll) = self.library.rolls.iter_mut().find(|r| r.id == id) else {
                        continue;
                    };
                    let (mut rotated, mut upright, mut changed) = (0, 0, false);
                    let mut review = Vec::new();
                    for result in batch.frames {
                        let Some(frame) = roll.frames.iter_mut().find(|f| f.id == result.id) else {
                            continue;
                        };
                        if frame.rotation != result.previous_rotation
                            || frame.crop != result.previous_crop
                            || frame.mirrored != result.previous_mirrored
                        {
                            review.push(format!("{} (edited during detection)", frame.name));
                            continue;
                        }
                        match result.guess {
                            Ok(guess) if guess.rotation.is_some() => {
                                let rotation = guess.rotation.unwrap();
                                if rotation == frame.rotation {
                                    upright += 1;
                                } else {
                                    frame.rotation = rotation;
                                    rotated += 1;
                                    changed = true;
                                }
                            }
                            outcome => {
                                changed |= !frame.review;
                                frame.review = true;
                                let reason = match outcome {
                                    Ok(guess) => guess.reason,
                                    Err(error) => error,
                                };
                                review.push(format!("{} ({reason})", frame.name));
                            }
                        }
                    }
                    if changed {
                        self.remember(before);
                        self.invalidate();
                        self.save();
                    }
                    self.status = format!(
                        "Auto upright · {} · {rotated} rotated · {upright} already upright · {} need review",
                        batch.backend,
                        review.len()
                    );
                    if changed {
                        self.status.push_str(" · Undo restores the batch");
                    }
                    if !review.is_empty() {
                        self.status.push_str(" · ");
                        self.status.push_str(&review.join(", "));
                    }
                }
                Event::AutoCropped(id, results) => {
                    let before = self.library.clone();
                    let Some(roll) = self.library.rolls.iter_mut().find(|r| r.id == id) else {
                        continue;
                    };
                    let mut cropped = 0;
                    let mut changed = false;
                    let mut skipped = Vec::new();
                    for result in &results {
                        let Some(frame) = roll.frames.iter_mut().find(|f| f.id == result.id) else {
                            continue;
                        };
                        match &result.crop {
                            Ok(Some(crop)) if frame.crop == result.previous => {
                                changed |= frame.crop != *crop;
                                frame.crop = *crop;
                                cropped += 1;
                            }
                            Ok(Some(_)) => {
                                skipped.push(format!("{} (edited during detection)", frame.name));
                            }
                            Ok(None) => skipped.push(format!("{} (no clear boundary)", frame.name)),
                            Err(_) => skipped.push(format!("{} (could not load)", frame.name)),
                        }
                    }
                    if changed {
                        self.remember(before);
                        self.invalidate();
                        self.save();
                    }
                    self.status = format!("Autocropped {cropped} of {} frames", results.len());
                    self.status.push_str(if changed {
                        " · Undo restores the batch"
                    } else {
                        " · No crops changed"
                    });
                    if !skipped.is_empty() {
                        self.status.push_str(" · Unchanged / review: ");
                        self.status.push_str(&skipped.join(", "));
                    }
                }
                Event::Histogram { key, result } => {
                    self.histogram_pending = false;
                    if key == self.histogram_key {
                        match result {
                            Ok(histogram) => self.histogram = Some(histogram),
                            Err(error) => self.histogram_error = Some(error),
                        }
                    }
                }
                Event::Neutral { key, result } => {
                    if key == self.neutral_key()
                        && !self.busy
                        && !self.comparing
                        && !self.preview_analysis
                    {
                        match result {
                            Ok([warmth, tint]) => {
                                let before = self.library.clone();
                                let roll = &self.library.rolls[self.roll];
                                let frame = &roll.frames[self.frame];
                                let (previous, other) = if self.scope_roll {
                                    (roll.adjustments, frame.adjustments)
                                } else {
                                    (frame.adjustments, roll.adjustments)
                                };
                                let mut next = previous;
                                next.warmth = (warmth - other.warmth).clamp(-50., 50.);
                                next.tint = (tint - other.tint).clamp(-50., 50.);
                                let limited = (warmth - other.warmth).abs() > 50.
                                    || (tint - other.tint).abs() > 50.;
                                self.change_adjustments(previous, next);
                                self.dirty = false;
                                self.remember(before);
                                self.invalidate();
                                self.save();
                                self.status=if limited {"Neutral correction reached the balance slider limit; choose a less strongly colored patch"}else{"Neutral patch applied · Undo restores the previous color balance"}.into();
                            }
                            Err(error) => self.status = error,
                        }
                    }
                }
                Event::JpegFloatRender {
                    prepared,
                    settings,
                    reply,
                } => {
                    let result = self
                        .gpu
                        .as_ref()
                        .ok_or_else(|| "GPU unavailable".to_owned())
                        .and_then(|gpu| {
                            gpu.lock()
                                .unwrap()
                                .export_srgb_source(&prepared.image, settings)
                                .map_err(|e| e.to_string())
                        });
                    let _ = reply.send(result);
                }
                Event::JpegRender {
                    prepared,
                    settings,
                    reply,
                } => {
                    let result = self
                        .gpu
                        .as_ref()
                        .ok_or_else(|| "GPU unavailable".to_owned())
                        .and_then(|gpu| {
                            gpu.lock()
                                .unwrap()
                                .export_jpeg_source(&prepared.image, settings)
                                .map_err(|e| e.to_string())
                        });
                    let _ = reply.send(result);
                }
                Event::ExportProgress {
                    completed,
                    total,
                    bytes,
                    elapsed,
                } => {
                    self.export_progress = Some((completed, total, bytes, elapsed));
                }
                Event::ExportRender {
                    prepared,
                    settings,
                    reply,
                } => {
                    let result = self
                        .gpu
                        .as_ref()
                        .ok_or_else(|| "GPU unavailable".to_owned())
                        .and_then(|gpu| {
                            gpu.lock()
                                .unwrap()
                                .export_source(&prepared.image, settings)
                                .map_err(|e| e.to_string())
                        });
                    let _ = reply.send(result);
                }
                Event::GpuScene(scene) => {
                    if self.full_desired.as_ref() == Some(&scene.key) {
                        self.gpu_pending = Some(scene);
                    }
                }
                Event::PreviewReady(output) => {
                    let full = output.quality == preview::Quality::Full;
                    let valid = if full {
                        self.full_desired.as_ref() == Some(&output.key)
                            && !ctx.input(|i| i.pointer.any_down())
                    } else {
                        self.desired.get(&output.id) == Some(&output.key)
                    };
                    if !valid {
                        continue;
                    }
                    for roll in &mut self.library.rolls {
                        if let Some(frame) =
                            roll.frames.iter_mut().find(|frame| frame.id == output.id)
                        {
                            frame.error.clone_from(&output.error);
                        }
                    }
                    if let Some(error) = output.error {
                        self.status = format!("Preview: {error}");
                        if full {
                            self.full_desired = None;
                        } else {
                            self.completed.insert(output.id, output.key);
                        }
                    } else {
                        let image = egui::ColorImage::from_rgb(
                            [output.width as usize, output.height as usize],
                            &output.bytes,
                        );
                        if full {
                            let texture = ctx.load_texture(
                                "active-detail",
                                image,
                                egui::TextureOptions::LINEAR,
                            );
                            self.detail_texture = Some((output.id, output.key, texture));
                        } else {
                            if let Some(texture) = self.textures.get_mut(&output.id) {
                                texture.set(image, egui::TextureOptions::LINEAR);
                            } else {
                                self.textures.insert(
                                    output.id.clone(),
                                    ctx.load_texture(
                                        &output.id,
                                        image,
                                        egui::TextureOptions::LINEAR,
                                    ),
                                );
                            }
                            self.completed.insert(output.id, output.key);
                        }
                    }
                    self.rendering = self
                        .desired
                        .iter()
                        .any(|(id, key)| self.completed.get(id) != Some(key));
                }
                Event::Progress(s) => self.status = s,
                Event::Loaded(id, scan) => {
                    let clipped = scan
                        .image
                        .pixels
                        .iter()
                        .filter(|p| p.iter().any(|v| !v.is_finite() || *v >= 0.995))
                        .count();
                    let warning = (clipped as f32 / scan.image.pixels.len().max(1) as f32 > 0.005)
                        .then(|| "Possible scan clipping · inspect the original; editing cannot recover missing capture detail".to_owned());
                    self.cache.insert(id.clone(), scan);
                    for r in &mut self.library.rolls {
                        if let Some(f) = r.frames.iter_mut().find(|f| f.id == id) {
                            f.error = None;
                            f.scan_warning.clone_from(&warning);
                        }
                    }
                    self.invalidate();
                }
                Event::Failed(id, error) => {
                    for r in &mut self.library.rolls {
                        if let Some(f) = r.frames.iter_mut().find(|f| f.id == id) {
                            f.error = Some(error.clone());
                        }
                    }
                    self.status = error;
                    self.save();
                }
                Event::Analysis(id, result) => match result {
                    Ok(a) => {
                        let before = self.library.clone();
                        if let Some(r) = self.library.rolls.iter_mut().find(|r| r.id == id) {
                            let requested_preview = self.requested_preview.as_ref() == Some(&id);
                            if requested_preview {
                                self.requested_preview = None;
                            }
                            if r.analysis.is_some() || r.conversion_locked || requested_preview {
                                self.neutral_analysis = a.individual;
                                let stale = a.is_stale(r);
                                self.pending_analysis = Some(model::ProposedConversion {
                                    roll_id: id,
                                    conversion: a,
                                    frame_id: None,
                                });
                                self.preview_analysis = true;
                                self.show_negative = false;
                                self.invalidate();
                                self.status = if stale { "Settings changed · update the preview before applying" } else { "Showing new preview · compare with Current conversion, then Apply conversion" }.into();
                                continue;
                            }
                            r.optimize_individually = a.individual;
                            r.calibration = a.shared.options;
                            r.analysis = Some(a);
                            if r.versions.is_empty() {
                                r.save_version("Initial conversion");
                            }
                        }
                        self.remember(before);
                        self.status = "Roll converted · ready to review".into();
                        self.invalidate();
                        self.save();
                    }
                    Err(e) => {
                        if self.requested_preview.as_ref() == Some(&id) {
                            self.requested_preview = None;
                        }
                        self.status = format!("Analysis needs attention: {e}");
                    }
                },
                Event::FrameAnalysis(roll_id, frame_id, result) => match result {
                    Ok(conversion) => {
                        self.pending_analysis = Some(model::ProposedConversion {
                            roll_id,
                            conversion,
                            frame_id: Some(frame_id),
                        });
                        self.neutral_analysis = false;
                        self.preview_analysis = true;
                        self.show_negative = false;
                        self.tool = Tool::View;
                        self.invalidate();
                        self.status = "Showing this frame's new conversion · other frames keep their applied looks".into();
                    }
                    Err(error) => {
                        self.status = format!("Frame conversion needs attention: {error}");
                    }
                },
                Event::Finished => {
                    self.busy = false;
                    if self.export_batch.is_some() {
                        self.next_export(ctx);
                        continue;
                    }
                    self.scheduled = None;
                    for roll in &mut self.library.rolls {
                        if roll.material == Material::Positive
                            && !roll.frames.is_empty()
                            && roll.versions.is_empty()
                        {
                            roll.save_version("Initial positive");
                        }
                    }
                    self.save();
                }
                Event::Exported(path, errors, retry) => {
                    self.last_export = Some(path.clone());
                    if let Some(retry) = retry {
                        self.export_retry.push_back(retry);
                    }
                    if let Some(batch) = &mut self.export_batch {
                        batch.completed += 1;
                        batch.bytes += self.export_progress.map_or(0, |(_, _, bytes, _)| bytes);
                        batch.last_directory = Some(path.clone());
                        batch.frames += self
                            .export_progress
                            .map_or(0, |(_, total, _, _)| total.saturating_sub(errors.len()));
                        batch.errors.extend(
                            errors
                                .iter()
                                .map(|e| format!("{} / {}: {}", path.display(), e.name, e.error)),
                        );
                    }
                    self.status = if errors.is_empty() {
                        self.export_progress.map_or_else(
                            || format!("Export complete · {}", path.display()),
                            |(_, total, bytes, elapsed)| {
                                format!(
                                    "Exported {total} frames · {:.1} MB · {elapsed:.1}s",
                                    bytes as f64 / 1_000_000.
                                )
                            },
                        )
                    } else {
                        format!(
                            "Export completed with {} errors: {}",
                            errors.len(),
                            errors
                                .iter()
                                .map(|e| format!("{}: {}", e.name, e.error))
                                .collect::<Vec<_>>()
                                .join("; ")
                        )
                    };
                }
            }
        }
    }
    fn schedule_render(&mut self, ctx: &egui::Context) {
        let state = (
            self.revision,
            self.roll,
            self.frame,
            self.grid,
            self.dragging,
        );
        if self.scheduled == Some(state) {
            return;
        }
        self.scheduled = Some(state);
        let Some(mut roll) = self.library.rolls.get(self.roll).cloned() else {
            self.renderer.submit(vec![], false);
            self.renderer.submit(vec![], true);
            self.rendering = false;
            return;
        };
        if self.comparing {
            if let Some(version) = roll.versions.get(self.version_index).cloned() {
                roll.restore_state(&version.state);
            }
        } else if self.preview_analysis {
            if let Some(proposal) = &self.pending_analysis {
                if proposal.roll_id == roll.id {
                    roll.analysis = Some(proposal.conversion.clone());
                }
            }
        }
        let active = roll.frames.get(self.frame).map(|f| f.id.clone());
        let mut requests = Vec::new();
        for frame in &roll.frames {
            if self.dragging && Some(&frame.id) != active.as_ref() {
                continue;
            }
            let Some(scan) = self.cache.get(&frame.id) else {
                continue;
            };
            let analysis = roll.analysis_for(frame).cloned();
            let negative = (!self.comparing && self.show_negative)
                || (analysis.is_none() && roll.material != Material::Positive);
            let scene = serde_json::to_string(&(roll.material, &analysis, negative)).unwrap();
            let quality = if self.dragging {
                preview::Quality::Draft
            } else {
                preview::Quality::Preview
            };
            let scene_key = format!("{}:{:p}:{quality:?}:{scene}", frame.id, Arc::as_ptr(scan));
            let edits = self.displayed_adjustments(&roll, frame);
            let key = serde_json::to_string(&(
                &scene_key,
                edits,
                frame.crop,
                frame.rotation,
                frame.mirrored,
                &frame.heals,
            ))
            .unwrap();
            requests.push(preview::Request {
                key,
                scene_key,
                frame: frame.clone(),
                scan: scan.clone(),
                material: roll.material,
                analysis,
                edits,
                negative,
                demo: roll.demo,
                quality,
                gpu: false,
            });
        }
        requests.sort_by_key(|r| Some(&r.frame.id) != active.as_ref());
        self.desired = requests
            .iter()
            .map(|r| (r.frame.id.clone(), r.key.clone()))
            .collect();
        let plan = requests
            .iter()
            .map(|r| r.key.as_str())
            .collect::<Vec<_>>()
            .join("|");
        let gpu_eligible = self.gpu.is_some()
            && requests.first().is_some_and(|r| !r.negative)
            && roll
                .frames
                .get(self.frame)
                .is_some_and(|f| f.heals.is_empty());
        let full = if (!self.dragging || gpu_eligible)
            && (!self.grid || self.refine || gpu_eligible)
            && !self.busy
        {
            requests
                .first()
                .filter(|r| Some(&r.frame.id) == active.as_ref())
                .cloned()
                .map(|mut r| {
                    r.quality = preview::Quality::Full;
                    r.gpu = gpu_eligible;
                    let scene =
                        serde_json::to_string(&(r.material, &r.analysis, r.negative)).unwrap();
                    r.scene_key = format!("{}:{:p}:Full:{scene}", r.frame.id, Arc::as_ptr(&r.scan));
                    r.key = serde_json::to_string(&(
                        &r.scene_key,
                        r.edits,
                        r.frame.crop,
                        r.frame.rotation,
                        r.frame.mirrored,
                        &r.frame.heals,
                    ))
                    .unwrap();
                    if r.gpu {
                        r.key.clone_from(&r.scene_key);
                    }
                    r
                })
        } else {
            None
        };
        if plan != self.fast_plan {
            self.fast_plan = plan;
            requests.retain(|r| self.completed.get(&r.frame.id) != Some(&r.key));
            if self.dragging
                && gpu_eligible
                && full.as_ref().is_some_and(|r| {
                    self.gpu
                        .as_ref()
                        .is_some_and(|g| g.lock().unwrap().key == r.key)
                })
            {
                for request in &requests {
                    self.completed
                        .insert(request.frame.id.clone(), request.key.clone());
                }
                requests.clear();
            }
            self.renderer.submit(requests, false);
        }
        let full_plan = full.as_ref().map_or_else(String::new, |r| r.key.clone());
        self.full_desired = full.as_ref().map(|r| r.key.clone());
        if full_plan != self.full_plan {
            self.full_plan = full_plan;
            let requests = full
                .into_iter()
                .filter(|r| {
                    if r.gpu {
                        return self
                            .gpu
                            .as_ref()
                            .is_none_or(|g| g.lock().unwrap().key != r.key);
                    }
                    self.detail_texture
                        .as_ref()
                        .is_none_or(|(_, key, _)| *key != r.key)
                })
                .collect();
            self.renderer.submit(requests, true);
        }
        self.rendering = self
            .desired
            .iter()
            .any(|(id, key)| self.completed.get(id) != Some(key));
        ctx.request_repaint();
    }
    fn apply_action(&mut self, action: Action, ctx: &egui::Context) {
        if self.comparing && !matches!(action, Action::Load | Action::Detail | Action::OpenExport) {
            return;
        }
        if self.library.rolls.get(self.roll).is_none()
            && !matches!(action, Action::Undo | Action::Redo | Action::OpenExport)
        {
            return;
        }
        match action {
            Action::AutoUpright => {
                if self.busy
                    || self.preview_analysis
                    || self.library.rolls[self.roll].frames.is_empty()
                {
                    return;
                }
                let roll = self.library.rolls[self.roll].clone();
                if roll.material != Material::Positive && roll.analysis.is_none() {
                    self.status = "Apply a conversion before automatic upright detection".into();
                    return;
                }
                let cached = roll
                    .frames
                    .iter()
                    .filter_map(|frame| {
                        self.cache
                            .get(&frame.id)
                            .map(|scan| (frame.id.clone(), scan.clone()))
                    })
                    .collect();
                self.busy = true;
                self.show_negative = false;
                self.tool = Tool::View;
                self.drag_start = None;
                self.status = "Checking upright orientation with the local face model…".into();
                jobs::auto_orient(
                    roll,
                    cached,
                    self.tx.clone(),
                    ctx.clone(),
                    self.profile(),
                    platform::upright_gpu_library(&self.root),
                );
            }
            Action::AutoCropAll => {
                if self.busy
                    || self.preview_analysis
                    || self.library.rolls[self.roll].frames.is_empty()
                {
                    return;
                }
                let roll = self.library.rolls[self.roll].clone();
                let cached = roll
                    .frames
                    .iter()
                    .filter_map(|f| {
                        self.cache
                            .get(&f.id)
                            .map(|scan| (f.id.clone(), scan.clone()))
                    })
                    .collect();
                self.busy = true;
                self.tool = Tool::View;
                self.drag_start = None;
                self.show_negative = false;
                self.status = "Autocropping all picture frames…".into();
                jobs::auto_crop(roll, cached, self.tx.clone(), ctx.clone());
            }
            Action::MirrorAll(mirrored) => {
                if self.busy || self.preview_analysis {
                    return;
                }
                if self.library.rolls[self.roll]
                    .frames
                    .iter()
                    .all(|f| f.mirrored == mirrored)
                {
                    return;
                }
                let before = self.library.clone();
                for frame in &mut self.library.rolls[self.roll].frames {
                    frame.mirrored = mirrored;
                }
                self.show_negative = false;
                self.tool = Tool::View;
                self.drag_start = None;
                self.remember(before);
                self.invalidate();
                self.save();
                self.status = if mirrored {
                    "All frames mirrored · Undo restores the previous orientations"
                } else {
                    "Mirroring cleared for all frames · Undo restores the previous orientations"
                }
                .into();
            }
            Action::SetIndividual(individual) => {
                if self.busy || self.library.rolls[self.roll].conversion_locked {
                    return;
                }
                let before = self.library.clone();
                self.library.rolls[self.roll].optimize_individually = individual;
                self.invalidate();
                self.remember(before);
                self.save();
                self.status = "Conversion mode changed · update the preview to use it".into();
            }
            Action::Mirror | Action::OrientationToRoll => {
                if self.busy || self.preview_analysis {
                    return;
                }
                let Some(active) = self.library.rolls[self.roll].frames.get(self.frame) else {
                    return;
                };
                let (rotation, mirrored) = (active.rotation, active.mirrored);
                let before = self.library.clone();
                let roll = &mut self.library.rolls[self.roll];
                if matches!(action, Action::Mirror) {
                    roll.frames[self.frame].mirrored = !mirrored;
                } else {
                    for frame in &mut roll.frames {
                        frame.rotation = rotation;
                        frame.mirrored = mirrored;
                    }
                }
                self.show_negative = false;
                self.tool = Tool::View;
                self.drag_start = None;
                self.remember(before);
                self.invalidate();
                self.save();
                self.status = "Orientation updated for positive previews and exports".into();
            }
            Action::Crop => {
                if self.busy
                    || self.preview_analysis
                    || self.library.rolls[self.roll].frames.is_empty()
                {
                    return;
                }
                if self.tool != Tool::Crop {
                    self.crop_return_negative = self.show_negative;
                }
                self.grid = false;
                self.tool = Tool::Crop;
                self.scope_roll = false;
                self.scope_selected = false;
                self.show_negative = true;
                self.drag_start = None;
                self.invalidate();
                self.status = "Drag a crop on the full original scan · Esc cancels".into();
            }
            Action::AutoCrop | Action::ResetCrop => {
                if self.busy || self.preview_analysis {
                    return;
                }
                let Some(frame) = self.library.rolls[self.roll].frames.get(self.frame) else {
                    return;
                };
                let crop = if matches!(action, Action::ResetCrop) {
                    film::Rect::default()
                } else {
                    let Some(scan) = self.cache.get(&frame.id) else {
                        self.status = "Wait for this scan to finish loading".into();
                        return;
                    };
                    let Some(crop) = film::detect_frame_crop(scan) else {
                        self.status = "No clear picture boundary on all four sides · use Crop to select the frame manually".into();
                        return;
                    };
                    crop
                };
                let before = self.library.clone();
                self.library.rolls[self.roll].frames[self.frame].crop = crop;
                self.grid = false;
                self.tool = Tool::View;
                self.drag_start = None;
                self.show_negative = false;
                self.remember(before);
                self.invalidate();
                self.save();
                self.status = if matches!(action, Action::ResetCrop) {
                    "Crop and straighten reset".into()
                } else {
                    format!(
                        "Picture cropped inside film base · straightened {:.2}° · Undo restores the previous crop",
                        crop.angle
                    )
                };
            }
            Action::Import | Action::ImportFolder => {
                if self.busy {
                    return;
                }
                let paths = if matches!(action, Action::ImportFolder) {
                    rfd::FileDialog::new()
                        .set_title("Add scans from a folder")
                        .pick_folder()
                        .and_then(|folder| match workflow::folder_scans(&folder) {
                            Ok(paths) => Some(paths),
                            Err(error) => {
                                self.status = error.to_string();
                                None
                            }
                        })
                } else {
                    rfd::FileDialog::new()
                        .add_filter(
                            "Scans",
                            &[
                                "cr3", "CR3", "cr2", "nef", "arw", "dng", "orf", "rw2", "raf",
                                "tif", "tiff", "jpg", "jpeg", "png",
                            ],
                        )
                        .pick_files()
                };
                if let Some(paths) = paths {
                    let before = self.library.clone();
                    let roll = &mut self.library.rolls[self.roll];
                    for path in paths {
                        if model::supported(&path) && !roll.frames.iter().any(|f| f.path == path) {
                            roll.frames.push(model::Frame::new(path));
                        }
                    }
                    roll.frames.sort_by(|a, b| a.name.cmp(&b.name));
                    // Adding scans must never discard an accepted conversion.
                    // Re-analysis is an explicit, previewable proposal.
                    self.remember(before);
                    self.save();
                    self.begin_load(ctx, false);
                }
            }
            Action::Load => {
                self.invalidate();
                self.begin_load(ctx, false);
            }
            Action::Analyze => {
                if self.busy || self.comparing {
                    return;
                }
                self.requested_preview = Some(self.library.rolls[self.roll].id.clone());
                self.show_negative = false;
                self.preview_analysis = false;
                self.pending_analysis = None;
                self.invalidate();
                self.begin_load(ctx, true);
            }
            Action::AnalyzeFrame(index) => {
                let roll = &self.library.rolls[self.roll];
                if self.busy
                    || self.comparing
                    || roll.conversion_locked
                    || !roll.optimize_individually
                    || roll.material == Material::Positive
                {
                    return;
                }
                let Some(frame) = roll.frames.get(index) else {
                    return;
                };
                let id = frame.id.clone();
                self.status = format!("Building conversion preview for {}…", frame.name);
                let cached = self.cache.get(&id).cloned();
                let roll = roll.clone();
                self.requested_preview = None;
                self.pending_analysis = None;
                self.preview_analysis = false;
                self.show_negative = false;
                self.tool = Tool::View;
                self.busy = true;
                self.scope_roll = false;
                self.scope_selected = false;
                self.invalidate();
                jobs::analyze_frame(roll, id, cached, self.tx.clone(), ctx.clone());
            }
            Action::SampleBase => {
                let roll = &self.library.rolls[self.roll];
                if self.busy
                    || self.comparing
                    || roll.conversion_locked
                    || roll.material == Material::Positive
                    || roll.frames.get(self.frame).is_none()
                {
                    return;
                }
                self.preview_analysis = false;
                self.show_negative = true;
                self.grid = false;
                self.tool = Tool::Base;
                self.scope_roll = false;
                self.scope_selected = false;
                self.drag_start = None;
                self.invalidate();
            }
            Action::SetBase { frame, base } => {
                let roll = &self.library.rolls[self.roll];
                if self.busy
                    || self.comparing
                    || roll.conversion_locked
                    || frame.is_some() != roll.optimize_individually
                {
                    return;
                }
                if base.is_some_and(|b| b.iter().any(|v| !v.is_finite() || *v <= 0. || *v >= 0.995))
                {
                    self.status = "Select an unclipped film-base patch".into();
                    return;
                }
                let before = self.library.clone();
                let roll = &mut self.library.rolls[self.roll];
                if let Some(index) = frame {
                    let Some(target) = roll.frames.get_mut(index) else {
                        return;
                    };
                    target.base = base;
                } else {
                    roll.base = base;
                }
                self.tool = Tool::View;
                self.remember(before);
                self.save();
                self.invalidate();
                if let Some(index) = frame {
                    self.apply_action(Action::AnalyzeFrame(index), ctx);
                } else {
                    self.status =
                        "Shared film base updated · build a new preview to apply it".into();
                }
            }
            Action::PreviewFaded => {
                let roll = &self.library.rolls[self.roll];
                if self.busy
                    || self.comparing
                    || roll.conversion_locked
                    || roll.frames.is_empty()
                    || roll.material != Material::ColorNegative
                {
                    return;
                }
                let before = self.library.clone();
                let roll = &mut self.library.rolls[self.roll];
                roll.optimize_individually = true;
                roll.calibration = film::CalibrationOptions {
                    illumination: true,
                    color_balance: 1.,
                };
                self.neutral_analysis = true;
                self.panel = Panel::Conversion;
                self.remember(before);
                self.save();
                self.apply_action(Action::Analyze, ctx);
            }
            Action::Export => self.start_export(ctx),
            Action::Undo => {
                if let Some(old) = self.history.pop() {
                    self.redo.push(self.library.clone());
                    self.library = old;
                    self.pending_analysis = None;
                    self.preview_analysis = false;
                    self.roll = self.roll.min(self.library.rolls.len().saturating_sub(1));
                    self.invalidate();
                    self.save();
                }
            }
            Action::Redo => {
                if let Some(new) = self.redo.pop() {
                    self.history.push(self.library.clone());
                    self.library = new;
                    self.pending_analysis = None;
                    self.preview_analysis = false;
                    self.invalidate();
                    self.save();
                }
            }
            Action::Reset => {
                if let Some(r) = self.library.rolls.get_mut(self.roll) {
                    if self.scope_roll {
                        r.adjustments = Adjustments::default();
                    } else if self.scope_selected {
                        for f in &mut r.frames {
                            if self.selected.contains(&f.id) {
                                f.adjustments = Adjustments::default();
                            }
                        }
                    } else if let Some(f) = r.frames.get_mut(self.frame) {
                        f.adjustments = Adjustments::default();
                    }
                    self.dirty = true;
                }
            }
            Action::Rotate | Action::RotateBack => {
                if self.busy || self.preview_analysis {
                    return;
                }
                let before = self.library.clone();
                let roll = &mut self.library.rolls[self.roll];
                let selected =
                    self.grid && roll.frames.iter().any(|f| self.selected.contains(&f.id));
                let mut count = 0;
                let backwards = matches!(action, Action::RotateBack);
                for (index, frame) in roll.frames.iter_mut().enumerate() {
                    if (selected && self.selected.contains(&frame.id))
                        || (!selected && index == self.frame)
                    {
                        // Mirroring is applied after rotation, so reverse the source
                        // turn to keep the displayed clockwise direction consistent.
                        frame.rotation =
                            (frame.rotation + if backwards ^ frame.mirrored { 3 } else { 1 }) % 4;
                        count += 1;
                    }
                }
                if count > 0 {
                    self.tool = Tool::View;
                    self.drag_start = None;
                    self.show_negative = false;
                    self.remember(before);
                    self.invalidate();
                    self.save();
                    self.status = format!(
                        "Rotated {count} frame{} {} · Undo restores the batch",
                        if count == 1 { "" } else { "s" },
                        if backwards {
                            "counterclockwise"
                        } else {
                            "clockwise"
                        }
                    );
                }
            }
            Action::Reject => {
                if let Some(f) = self.library.rolls[self.roll].frames.get_mut(self.frame) {
                    f.rejected = !f.rejected;
                    self.dirty = true;
                }
            }
            Action::Detail => {
                self.refine = true;
                self.zoom = true;
                self.view.native_pending = true;
                self.scheduled = None;
                self.status = "Refining active frame at full resolution…".into();
            }
            Action::RestoreAnalysis => {
                let r = &mut self.library.rolls[self.roll];
                if r.conversion_locked {
                    return;
                }
                if let Some(previous) = r.previous_analysis.take() {
                    r.optimize_individually = previous.individual;
                    r.calibration = previous.shared.options;
                    r.previous_analysis = r.analysis.take();
                    r.analysis = Some(previous);
                    self.dirty = true;
                }
            }
            Action::OpenExport => {
                if let Some(path) = &self.last_export {
                    if let Err(error) = platform::open_path(path) {
                        self.status = format!("Could not open export: {error}");
                    }
                }
            }
            Action::CopyEdits => {
                self.clipboard = self.library.rolls[self.roll]
                    .frames
                    .get(self.frame)
                    .cloned();
                if let Some(frame) = &self.clipboard {
                    ctx.copy_text(model::EditClipboard::encode(frame));
                }
                self.status =
                    "Copied active frame corrections (roll settings are not included)".into();
            }
            Action::PasteEdits => {
                if self.busy || self.preview_analysis {
                    return;
                }
                if let Some(source) = &self.clipboard {
                    let r = &mut self.library.rolls[self.roll];
                    let mut count = 0;
                    for (i, target) in r.frames.iter_mut().enumerate() {
                        if (self.scope_selected && self.selected.contains(&target.id))
                            || (!self.scope_selected && i == self.frame)
                        {
                            model::paste_edits(target, source, self.paste_options);
                            self.dirty = true;
                            count += 1;
                        }
                    }
                    self.status = format!(
                        "Pasted frame corrections to {count} frame(s) · roll settings unchanged"
                    );
                }
            }
            Action::SaveVersion => {
                let name = if self.version_name.trim().is_empty() {
                    "Saved look"
                } else {
                    self.version_name.trim()
                };
                let r = &mut self.library.rolls[self.roll];
                r.save_version(name);
                self.version_index = r.versions.len() - 1;
                self.version_name.clear();
                self.dirty = true;
            }
            Action::RestoreVersion => {
                let r = &mut self.library.rolls[self.roll];
                if r.conversion_locked {
                    self.status =
                        "Unlock conversion before restoring a complete saved version".into();
                    return;
                }
                if let Some(version) = r.versions.get(self.version_index).cloned() {
                    r.save_version("Before restoring version");
                    r.restore_state(&version.state);
                    self.pending_analysis = None;
                    self.preview_analysis = false;
                    self.dirty = true;
                }
            }
            Action::AcceptAnalysis => {
                let before = self.library.clone();
                let r = &mut self.library.rolls[self.roll];
                if r.conversion_locked {
                    self.status = "Unlock conversion before accepting new analysis".into();
                    return;
                }
                if self
                    .pending_analysis
                    .as_ref()
                    .is_some_and(|p| p.roll_id == r.id && p.conversion.is_stale(r))
                {
                    self.status = "Settings changed · update the preview before applying".into();
                    return;
                }
                if let Some(proposal) = self.pending_analysis.take() {
                    if proposal.roll_id == r.id {
                        let a = proposal.conversion;
                        let first_conversion = r.analysis.is_none();
                        if !first_conversion {
                            r.save_version("Before re-analysis");
                        }
                        r.optimize_individually = a.individual;
                        r.calibration = a.shared.options;
                        r.previous_analysis = r.analysis.replace(a);
                        if self.neutral_analysis {
                            if let Some(id) = &proposal.frame_id {
                                if let Some(frame) = r.frames.iter_mut().find(|f| f.id == *id) {
                                    frame.adjustments =
                                        Adjustments::default().difference(r.adjustments);
                                }
                            } else {
                                r.adjustments = Adjustments::default();
                                for frame in &mut r.frames {
                                    frame.adjustments = Adjustments::default();
                                }
                            }
                        }
                        if first_conversion {
                            r.save_version("Initial conversion");
                        }
                        self.preview_analysis = false;
                        self.dirty = false;
                        self.remember(before);
                        self.invalidate();
                        self.save();
                        self.status =
                            if proposal.frame_id.is_some() { "Frame conversion applied · other frames unchanged; prior look saved as a version" }
                            else { "Conversion applied · ready to edit or export; prior look saved as a version" }.into();
                    } else {
                        self.pending_analysis = Some(proposal);
                    }
                }
            }
            Action::DiscardAnalysis => {
                self.pending_analysis = None;
                self.preview_analysis = false;
                self.invalidate();
            }
            Action::RetryExport => {
                if !self.busy {
                    let tasks = self
                        .export_retry
                        .drain(..)
                        .map(|retry| workflow::ExportTask {
                            roll: retry.roll,
                            only: Some(retry.ids),
                            directory: Some(retry.directory),
                        })
                        .collect();
                    self.start_export_tasks(tasks, ctx);
                }
            }
        }
    }

    fn select_frame(&mut self, index: usize, toggle: bool, range: bool) {
        let r = &self.library.rolls[self.roll];
        if index >= r.frames.len() {
            return;
        }
        if range {
            for f in &r.frames[self.frame.min(index)..=self.frame.max(index)] {
                self.selected.insert(f.id.clone());
            }
        } else if toggle {
            let id = r.frames[index].id.clone();
            if !self.selected.remove(&id) {
                self.selected.insert(id);
            }
        } else {
            self.selected.clear();
            self.selected.insert(r.frames[index].id.clone());
        }
        self.frame = index;
        self.selected_heal = None;
    }

    fn displayed_adjustments(&self, roll: &Roll, frame: &model::Frame) -> Adjustments {
        if self.comparing {
            if let Some(version) = roll.versions.get(self.version_index) {
                let frame = version
                    .state
                    .frames
                    .iter()
                    .find(|f| f.id == frame.id)
                    .unwrap_or(frame);
                return version.state.adjustments.combined(frame.adjustments);
            }
        } else if self.preview_analysis
            && self.neutral_analysis
            && self
                .pending_analysis
                .as_ref()
                .is_some_and(|p| p.affects(roll, frame))
        {
            return Adjustments::default();
        }
        roll.adjustments.combined(frame.adjustments)
    }

    fn displayed_conversion<'a>(&'a self, roll: &'a Roll) -> Option<&'a model::Conversion> {
        if self.comparing {
            return roll
                .versions
                .get(self.version_index)?
                .state
                .analysis
                .as_ref();
        }
        if self.preview_analysis {
            if let Some(proposal) = &self.pending_analysis {
                if proposal.roll_id == roll.id {
                    return Some(&proposal.conversion);
                }
            }
        }
        roll.analysis.as_ref()
    }

    fn edit_shortcuts(&mut self, ctx: &egui::Context) {
        let text_focused = ctx
            .memory(egui::Memory::focused)
            .is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some());
        if text_focused
            || self.busy
            || self.comparing
            || self.preview_analysis
            || self.library.rolls.get(self.roll).is_none()
        {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        for event in &events {
            if let egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } = event
            {
                if !modifiers.command && !modifiers.ctrl && !modifiers.alt {
                    match key {
                        egui::Key::R => self.actions.push(if modifiers.shift {
                            Action::RotateBack
                        } else {
                            Action::Rotate
                        }),
                        egui::Key::X if !modifiers.shift => self.actions.push(Action::Reject),
                        _ => {}
                    }
                }
            }
        }
        if events.iter().any(|e| matches!(e,egui::Event::Key {key:egui::Key::E,pressed:true,modifiers,..} if modifiers.command||modifiers.ctrl)) {
            self.export_dialog = true;
        }
        let copy=events.iter().any(|e|matches!(e,egui::Event::Copy)||matches!(e,egui::Event::Key{key:egui::Key::C,pressed:true,modifiers,..} if modifiers.command||modifiers.ctrl));
        if copy {
            self.actions.push(Action::CopyEdits);
        }
        let pasted = events.iter().find_map(|e| {
            if let egui::Event::Paste(text) = e {
                Some(text)
            } else {
                None
            }
        });
        let paste = if let Some(text) = pasted {
            if let Some(frame) = model::EditClipboard::decode(text) {
                self.clipboard = Some(frame);
                true
            } else {
                false
            }
        } else {
            events.iter().any(|e|matches!(e,egui::Event::Key{key:egui::Key::V,pressed:true,modifiers,..} if modifiers.command||modifiers.ctrl))&&self.clipboard.is_some()
        };
        if paste {
            self.scope_roll = false;
            self.scope_selected = self.library.rolls[self.roll]
                .frames
                .iter()
                .filter(|f| self.selected.contains(&f.id))
                .count()
                > 1;
            self.actions.push(Action::PasteEdits);
        }
    }
    fn change_adjustments(&mut self, previous: Adjustments, next: Adjustments) {
        let r = &mut self.library.rolls[self.roll];
        if self.scope_roll {
            r.adjustments = next;
        } else if self.scope_selected {
            let delta = next.difference(previous);
            for f in &mut r.frames {
                if self.selected.contains(&f.id) {
                    f.adjustments = f.adjustments.combined(delta);
                }
            }
        } else if let Some(f) = r.frames.get_mut(self.frame) {
            f.adjustments = next;
        }
        self.dirty = true;
    }
}
impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        if let Some(mut replay) = self.smoke.take() {
            replay.inject(self, ctx, input);
            self.smoke = Some(replay);
        }
    }
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if std::env::var_os("GREIGN_TRACE_INPUT").is_some() {
            ctx.input(|i| {
                for event in &i.events {
                    if matches!(
                        event,
                        egui::Event::PointerButton { .. } | egui::Event::Key { .. }
                    ) {
                        eprintln!("native input {event:?}; scale={}", ctx.pixels_per_point());
                    }
                }
            });
        }
        self.drain(ctx);
        if !ctx.input(|i| i.pointer.any_down()) {
            if let Some(scene) = self
                .gpu_pending
                .take()
                .filter(|s| self.full_desired.as_ref() == Some(&s.key))
            {
                if let Some(gpu) = &self.gpu {
                    let start = Instant::now();
                    let result = gpu.lock().unwrap().upload(scene.key, &scene.prepared.image);
                    jobs::record(
                        &self.profile(),
                        &serde_json::json!({"stage":"gpu-upload-submit","elapsed_ms":start.elapsed().as_secs_f64()*1000.}),
                    );
                    if let Err(e) = result {
                        self.status = format!("GPU preview unavailable; using CPU: {e}");
                        if let Some(gpu) = self.gpu.take() {
                            gpu.lock().unwrap().destroy();
                        }
                        self.scheduled = None;
                    }
                }
            }
        }
        let before = if self.gesture.is_none() {
            Some(self.library.clone())
        } else {
            None
        };
        if !ctx.wants_keyboard_input() {
            ctx.input(|i| {
                if i.key_pressed(egui::Key::ArrowRight) {
                    if let Some(r) = self.library.rolls.get(self.roll) {
                        self.frame = (self.frame + 1).min(r.frames.len().saturating_sub(1));
                    }
                }
                if i.key_pressed(egui::Key::ArrowLeft) {
                    self.frame = self.frame.saturating_sub(1);
                }
                if i.key_pressed(egui::Key::Escape) {
                    if self.tool == Tool::Crop {
                        self.show_negative = self.crop_return_negative;
                        self.invalidate();
                    }
                    self.drag_start = None;
                    self.tool = Tool::View;
                    self.move_source = false;
                }
                if i.key_pressed(egui::Key::C) && !i.modifiers.command && !self.busy {
                    self.actions.push(Action::Crop);
                }
                if i.key_pressed(egui::Key::Z) && i.modifiers.command && !self.busy {
                    self.actions.push(if i.modifiers.shift {
                        Action::Redo
                    } else {
                        Action::Undo
                    });
                }
            });
        }
        ui::draw(self, ctx);
        self.edit_shortcuts(ctx);
        let comparing = (self.compare_button
            || (!ctx.wants_keyboard_input() && ctx.input(|i| i.key_down(egui::Key::Backslash))))
            && self
                .library
                .rolls
                .get(self.roll)
                .is_some_and(|r| !r.versions.is_empty());
        if comparing != self.comparing {
            self.comparing = comparing;
            self.invalidate();
        }
        for action in std::mem::take(&mut self.actions) {
            self.apply_action(action, ctx);
        }
        if self.dirty {
            if self.gesture.is_none() {
                self.gesture = before;
            }
            self.invalidate();
            self.dirty = false;
        }
        if !ctx.input(|i| i.pointer.any_down()) {
            if let Some(before) = self.gesture.take() {
                self.refine = true;
                self.scheduled = None;
                self.remember(before);
                self.save();
            }
        }
        self.dragging = self.gesture.is_some() && ctx.input(|i| i.pointer.any_down());
        self.schedule_render(ctx);
        if self.busy || self.rendering || self.smoke.as_ref().is_some_and(|s| !s.finished()) {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        let mut zoom_capture = false;
        if let Some(path) = &self.screenshot {
            if !self.screenshot_requested {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            if !self.screenshot_requested
                && !self.busy
                && !self.rendering
                && (!self.textures.is_empty()
                    || self
                        .library
                        .rolls
                        .get(self.roll)
                        .is_none_or(|r| r.frames.is_empty()))
                && self.start.elapsed() > Duration::from_secs(2)
                && (!self.gpu_ui_check
                    || self.gpu.as_ref().is_some_and(|gpu| {
                        self.full_desired.as_ref() == Some(&gpu.lock().unwrap().key)
                    }))
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.screenshot_requested = true;
            }
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let bytes: Vec<_> = image
                        .pixels
                        .iter()
                        .flat_map(egui::Color32::to_array)
                        .collect();
                    let _ =
                        std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
                    if let Err(e) = image::save_buffer(
                        path,
                        &bytes,
                        image.width() as u32,
                        image.height() as u32,
                        image::ColorType::Rgba8,
                    ) {
                        eprintln!("Screenshot: {e}");
                    }
                    if self.gpu_ui_check {
                        if self.gpu_ui_stage == 0 {
                            self.gpu_ui_stage = 1;
                            zoom_capture = true;
                        } else {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    } else if self.smoke.as_ref().is_some_and(smoke::Replay::finished) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }
        if zoom_capture {
            self.screenshot = Some(self.root.join("exports/gpu-preview/native-zoom.png"));
            self.screenshot_requested = false;
            self.view.scale = 3.;
            self.view.pan = egui::vec2(180., -90.);
            self.zoom = true;
            ctx.request_repaint();
        }
        if self.gpu_ui_check && self.start.elapsed() > Duration::from_secs(30) {
            eprintln!("GPU UI validation timed out: {}", self.status);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        if let Some(gpu) = self.gpu.take() {
            gpu.lock().unwrap().destroy();
        }
        self.save();
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let gpu_ui_check = args.iter().any(|a| a == "--gpu-ui-check");
    let root = platform::workspace_root()?;
    platform::prepare_upright_gpu_loader(&root);
    std::fs::create_dir_all(root.join(".greign"))?;
    if args.iter().any(|a| {
        a == "--gpu-benchmark"
            || a == "--gpu-export-check"
            || a == "--jpeg-export-check"
            || a == "--orientation-check"
    }) {
        struct Close;
        impl eframe::App for Close {
            fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        let synthetic_only = args.iter().any(|a| a == "--orientation-check");
        let jpeg_check = synthetic_only || args.iter().any(|a| a == "--jpeg-export-check");
        let long_edge = args
            .windows(2)
            .find(|a| a[0] == "--long-edge")
            .map(|a| a[1].parse::<u32>())
            .transpose()?
            .unwrap_or(0);
        let export_check = jpeg_check || args.iter().any(|a| a == "--gpu-export-check");
        let result = Arc::new(std::sync::Mutex::new(None));
        let out = result.clone();
        eframe::run_native(
            "Greign GPU validation",
            eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default().with_inner_size([400., 180.]),
                ..Default::default()
            },
            Box::new(move |cc| {
                *out.lock().unwrap() = Some(
                    cc.gl
                        .as_ref()
                        .ok_or_else(|| "OpenGL unavailable".to_owned())
                        .and_then(|gl| {
                            if export_check {
                                let mut renderer = greign_gl_preview::Preview::new(gl.clone())
                                    .map_err(|e| e.to_string())?;
                                let result = crate::export_check::run(
                                    &renderer,
                                    &cc.egui_ctx,
                                    &root,
                                    jpeg_check,
                                    long_edge,
                                    synthetic_only,
                                )
                                .map_err(|e| e.to_string());
                                renderer.destroy();
                                result
                            } else {
                                greign_gl_preview::benchmark::run(gl.clone(), &root)
                                    .map_err(|e| e.to_string())
                            }
                        }),
                );
                Ok(Box::new(Close))
            }),
        )?;
        result
            .lock()
            .unwrap()
            .take()
            .ok_or("GPU benchmark did not run")??;
        return Ok(());
    }
    let path = if gpu_ui_check {
        root.join("artifacts/gpu-ui-library.json")
    } else {
        args.iter()
            .position(|a| a == "--library-path")
            .and_then(|i| args.get(i + 1))
            .map_or_else(
                || {
                    std::env::var_os("GREIGN_LIBRARY_PATH")
                        .map_or_else(|| root.join(".greign/rolls.json"), PathBuf::from)
                },
                PathBuf::from,
            )
    };
    let mut library = model::load(&path)?;
    if gpu_ui_check {
        library = Library::default();
    }
    if library.rolls.is_empty() {
        let demo = model::seed_demo(&root);
        if !demo.frames.is_empty() {
            library.rolls.push(demo);
            model::save(&path, &library)?;
        }
    }
    let screenshot = if gpu_ui_check {
        Some(root.join("exports/gpu-preview/native-window.png"))
    } else {
        args.iter()
            .position(|a| a == "--screenshot")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Greign — Film workspace")
            .with_app_id("greign")
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!(
                "../assets/greign.png"
            ))?)
            .with_inner_size([1440.0, 960.0])
            .with_min_inner_size([1080.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Greign",
        options,
        Box::new(move |cc| {
            let mut app = App::new(&cc.egui_ctx, root, library, screenshot);
            if let Some(runtime) = platform::upright_gpu_library(&app.root) {
                let profile = app.profile();
                rayon::spawn(move || {
                    let start = Instant::now();
                    let result = film::UprightDetector::with_cuda(&runtime)
                        .map(|_| "ready")
                        .map_err(|e| e.to_string());
                    jobs::record(
                        &profile,
                        &serde_json::json!({"stage":"upright-gpu-warmup","elapsed_ms":start.elapsed().as_secs_f64()*1000.,"result":result}),
                    );
                });
            }
            app.gpu_ui_check = gpu_ui_check;
            if gpu_ui_check {
                app.grid = false;
                app.refine = true;
            }
            if std::env::var_os("GREIGN_CPU_PREVIEW").is_none() {
                if let Some(gl) = &cc.gl {
                    match greign_gl_preview::Preview::new(gl.clone()) {
                        Ok(gpu) => app.gpu = Some(Arc::new(std::sync::Mutex::new(gpu))),
                        Err(e) => {
                            app.status = format!("CPU preview (GPU initialization failed: {e})");
                        }
                    }
                }
            }
            app.library_path = path;
            if let Some(i) = args.iter().position(|a| a == "--smoke-script") {
                let path = args.get(i + 1).ok_or("--smoke-script needs a JSON path")?;
                app.smoke = Some(smoke::Replay::load(std::path::Path::new(path))?);
            }
            Ok(Box::new(app))
        }),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neutral_proposal_is_preview_only_until_accept_and_undo_restores_every_edit() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().into(), Library::default(), None);
        let mut roll = Roll::new("Neutral proposal");
        let mut frame = model::Frame::new("frame.png".into());
        frame.adjustments.shadows = 22.;
        frame.adjustments.colors[0].hue = 5.;
        frame.crop = film::Rect {
            x: 0.1,
            y: 0.1,
            w: 0.8,
            h: 0.8,
            angle: 0.,
        };
        frame.rotation = 1;
        frame.mirrored = true;
        frame.review = true;
        frame.heals.push(film::Heal {
            target: [0.2, 0.3],
            source: [0.3, 0.3],
            radius: 0.01,
        });
        roll.frames.push(frame);
        roll.adjustments.brightness = 0.9;
        roll.adjustments.warmth = 11.;
        let analysis = film::Analysis {
            channel_scale: [1.; 3],
            illumination: [0.; 5],
            options: film::CalibrationOptions::default(),
            version: 3,
            base: [0.8, 0.5, 0.3],
            floor: [0.; 3],
            range: 2.9,
            method: "old".into(),
            warnings: vec![],
            frames: 1,
        };
        roll.analysis = Some(analysis.clone().into());
        app.library.rolls.push(roll);
        let before = serde_json::to_value(&app.library).unwrap();
        let mut next = model::Conversion::from(analysis);
        next.shared.version = film::ANALYSIS_VERSION;
        next.shared.range = 0.8;
        next.individual = true;
        next.per_frame.insert(
            app.library.rolls[0].frames[0].id.clone(),
            next.shared.clone(),
        );
        app.tx
            .send(Event::Analysis(app.library.rolls[0].id.clone(), Ok(next)))
            .unwrap();
        app.drain(&ctx);
        assert!(app.neutral_analysis);
        assert!(
            app.preview_analysis,
            "A completed proposal must be shown automatically"
        );
        assert_eq!(serde_json::to_value(&app.library).unwrap(), before);
        app.preview_analysis = true;
        let r = &app.library.rolls[0];
        assert_eq!(
            app.displayed_adjustments(r, &r.frames[0]),
            Adjustments::default()
        );
        assert_eq!(
            app.displayed_conversion(r).unwrap().shared.version,
            film::ANALYSIS_VERSION
        );
        app.neutral_analysis = false;
        assert_eq!(
            app.displayed_adjustments(r, &r.frames[0]),
            r.adjustments.combined(r.frames[0].adjustments)
        );
        app.neutral_analysis = true;
        app.apply_action(Action::AcceptAnalysis, &ctx);
        let accepted = serde_json::to_value(&app.library).unwrap();
        let r = &app.library.rolls[0];
        assert_eq!(r.adjustments, Adjustments::default());
        assert_eq!(r.frames[0].adjustments, Adjustments::default());
        for key in [
            "crop", "rotation", "mirrored", "heals", "review", "rejected",
        ] {
            assert_eq!(
                accepted["rolls"][0]["frames"][0][key],
                before["rolls"][0]["frames"][0][key]
            );
        }
        assert_eq!(r.versions.len(), 1);
        app.comparing = true;
        assert!((app.displayed_adjustments(r, &r.frames[0]).brightness - 0.9).abs() < 1e-6);
        app.comparing = false;
        app.apply_action(Action::Undo, &ctx);
        assert_eq!(serde_json::to_value(&app.library).unwrap(), before);
        app.apply_action(Action::Redo, &ctx);
        assert_eq!(serde_json::to_value(&app.library).unwrap(), accepted);
        assert_eq!(
            serde_json::to_value(model::load(&app.library_path).unwrap()).unwrap(),
            accepted
        );
    }

    #[test]
    fn faded_preset_previews_without_applying_and_rejects_stale_inputs() {
        for initially_converted in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = egui::Context::default();
            let mut app = App::new(&ctx, dir.path().into(), Library::default(), None);
            let path = dir.path().join("negative.png");
            image::RgbImage::from_fn(160, 100, |x, _| {
                image::Rgb([220 - x as u8, 180 - x as u8 / 2, 120 - x as u8 / 3])
            })
            .save(&path)
            .unwrap();
            let scan = film::load_scan(&path, Some(960), false).unwrap();
            let mut roll = Roll::new("Faded film");
            roll.frames.push(model::Frame::new(path));
            roll.adjustments.brightness = 0.5;
            if initially_converted {
                roll.analysis = Some(
                    film::analyze(&[&scan], None, roll.analysis_area)
                        .unwrap()
                        .into(),
                );
            }
            let original = serde_json::to_value(&roll.analysis).unwrap();
            app.library.rolls.push(roll);
            app.show_negative = true;
            app.apply_action(Action::PreviewFaded, &ctx);
            let deadline = Instant::now() + Duration::from_secs(5);
            while app.busy && Instant::now() < deadline {
                app.drain(&ctx);
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(!app.busy);
            assert!(app.preview_analysis && app.neutral_analysis && !app.show_negative);
            let r = &app.library.rolls[0];
            let a = &app.pending_analysis.as_ref().unwrap().conversion;
            assert!(
                a.individual
                    && a.shared.options.illumination
                    && a.shared.options.color_balance > 0.99
            );
            assert!(a.inputs.is_some() && a.matches_settings(r));
            assert_eq!(serde_json::to_value(&r.analysis).unwrap(), original);
            assert_eq!(
                app.displayed_adjustments(r, &r.frames[0]),
                Adjustments::default()
            );
            app.library.rolls[0].calibration.color_balance = 0.4;
            app.apply_action(Action::AcceptAnalysis, &ctx);
            assert!(app.pending_analysis.is_some() && app.status.contains("Settings changed"));
            assert_eq!(
                serde_json::to_value(&app.library.rolls[0].analysis).unwrap(),
                original
            );
            app.library.rolls[0].calibration.color_balance = 1.;
            let previous_area = app.library.rolls[0].analysis_area;
            app.library.rolls[0].analysis_area.x += 0.01;
            assert!(
                app.pending_analysis
                    .as_ref()
                    .unwrap()
                    .conversion
                    .is_stale(&app.library.rolls[0])
            );
            app.library.rolls[0].analysis_area = previous_area;
            app.library.rolls[0].frames[0].rejected = true;
            assert!(
                app.pending_analysis
                    .as_ref()
                    .unwrap()
                    .conversion
                    .is_stale(&app.library.rolls[0])
            );
            app.library.rolls[0].frames[0].rejected = false;
            // Creative corrections and ignored shared-base settings do not invalidate individual calibration.
            app.library.rolls[0].base = Some([0.4; 3]);
            app.library.rolls[0].adjustments.brightness = 1.5;
            assert!(
                !app.pending_analysis
                    .as_ref()
                    .unwrap()
                    .conversion
                    .is_stale(&app.library.rolls[0])
            );
            let before_apply = serde_json::to_value(&app.library).unwrap();
            app.apply_action(Action::AcceptAnalysis, &ctx);
            assert!(app.pending_analysis.is_none() && !app.preview_analysis);
            assert_eq!(app.library.rolls[0].adjustments, Adjustments::default());
            if !initially_converted {
                assert_eq!(app.library.rolls[0].versions[0].name, "Initial conversion");
            }
            let saved = model::load(&app.library_path).unwrap();
            assert!(
                saved.rolls[0]
                    .analysis
                    .as_ref()
                    .unwrap()
                    .matches_settings(&saved.rolls[0])
            );
            app.apply_action(Action::Undo, &ctx);
            assert_eq!(serde_json::to_value(&app.library).unwrap(), before_apply);
        }
    }

    #[test]
    fn frame_base_preview_apply_reset_and_undo_preserve_every_other_frame() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().into(), Library::default(), None);
        let mut roll = Roll::new("Individual bases");
        let image = image::RgbImage::from_fn(160, 100, |x, _| {
            let density = x.saturating_sub(20) as u8;
            image::Rgb([220 - density, 190 - density / 2, 150 - density / 3])
        });
        for i in 0..3 {
            let path = dir.path().join(format!("frame-{i}.png"));
            image.save(&path).unwrap();
            let mut frame = model::Frame::new(path);
            frame.rotation = i;
            frame.mirrored = i == 2;
            frame.adjustments.tint = f32::from(i) * 3.;
            roll.frames.push(frame);
        }
        roll.adjustments = Adjustments {
            brightness: 0.75,
            warmth: 4.,
            saturation: 9.,
            ..Adjustments::default()
        };
        let source = Arc::new(film::load_scan(&roll.frames[0].path, Some(960), false).unwrap());
        let base = film::measure_base(
            &source,
            film::Rect {
                x: 0.03,
                y: 0.2,
                w: 0.04,
                h: 0.15,
                angle: 0.,
            },
        )
        .unwrap();
        let refs: Vec<_> = roll
            .frames
            .iter()
            .map(|f| (f.id.as_str(), source.as_ref()))
            .collect();
        roll.analysis = Some(
            model::Conversion::analyze(
                &refs,
                true,
                None,
                roll.analysis_area,
                roll.calibration,
                &roll.frame_bases(),
            )
            .unwrap(),
        );
        let original = roll.clone();
        for f in &roll.frames {
            app.cache.insert(f.id.clone(), source.clone());
        }
        app.selected = roll.frames.iter().map(|f| f.id.clone()).collect();
        app.frame = 1;
        app.library.rolls.push(roll);
        app.apply_action(Action::SampleBase, &ctx);
        assert!(app.show_negative && app.tool == Tool::Base && !app.grid);
        app.apply_action(
            Action::SetBase {
                frame: Some(1),
                base: Some(base),
            },
            &ctx,
        );
        let wait = |app: &mut App| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while app.busy && Instant::now() < deadline {
                app.drain(&ctx);
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(!app.busy);
        };
        wait(&mut app);
        let pending = app.pending_analysis.as_ref().unwrap();
        assert_eq!(pending.frame_id.as_ref(), Some(&original.frames[1].id));
        assert!(app.preview_analysis && !app.show_negative);
        for i in [0, 2] {
            assert_eq!(
                serde_json::to_value(pending.conversion.for_frame(&original.frames[i].id)).unwrap(),
                serde_json::to_value(original.analysis_for(&original.frames[i])).unwrap()
            );
            assert_eq!(
                app.displayed_adjustments(&app.library.rolls[0], &app.library.rolls[0].frames[i]),
                original
                    .adjustments
                    .combined(original.frames[i].adjustments)
            );
        }
        assert!(
            !app.neutral_analysis,
            "A base-only correction keeps the target's creative edits by default"
        );
        assert_eq!(
            app.displayed_adjustments(&app.library.rolls[0], &app.library.rolls[0].frames[1]),
            original
                .adjustments
                .combined(original.frames[1].adjustments)
        );
        app.neutral_analysis = true;
        assert_eq!(
            app.displayed_adjustments(&app.library.rolls[0], &app.library.rolls[0].frames[1]),
            Adjustments::default()
        );
        assert_eq!(
            serde_json::to_value(&app.library.rolls[0].analysis).unwrap(),
            serde_json::to_value(&original.analysis).unwrap()
        );
        app.library.rolls[0].frames[1].base = Some(base.map(|v| v * 0.9));
        app.apply_action(Action::AcceptAnalysis, &ctx);
        assert!(app.pending_analysis.is_some() && app.status.contains("Settings changed"));
        app.library.rolls[0].frames[1].base = Some(base);
        app.apply_action(Action::AcceptAnalysis, &ctx);
        let applied = app.library.rolls[0].clone();
        assert_eq!(applied.adjustments, original.adjustments);
        assert_eq!(
            applied.adjustments.combined(applied.frames[1].adjustments),
            Adjustments::default()
        );
        assert!(
            applied
                .analysis
                .as_ref()
                .unwrap()
                .matches_settings(&applied)
        );
        for i in [0, 2] {
            assert_eq!(
                serde_json::to_value(&applied.frames[i]).unwrap(),
                serde_json::to_value(&original.frames[i]).unwrap()
            );
            let render = |r: &Roll| {
                film::render(
                    &source,
                    r.material,
                    r.analysis_for(&r.frames[i]),
                    r.adjustments.combined(r.frames[i].adjustments),
                    r.frames[i].crop,
                    r.frames[i].rotation,
                    r.frames[i].mirrored,
                    &r.frames[i].heals,
                    false,
                )
                .unwrap()
                .to_rgb8()
            };
            assert_eq!(
                render(&applied),
                render(&original),
                "Untouched frame exports must be identical"
            );
        }
        let reloaded = model::load(&app.library_path).unwrap();
        assert_eq!(
            reloaded.rolls[0].frames[1]
                .base
                .map(|p| p.map(f32::to_bits)),
            Some(base.map(f32::to_bits))
        );
        let mut restored = applied.clone();
        restored.restore_state(&original.snapshot());
        assert!(restored.frames.iter().all(|f| f.base.is_none()));
        let mut legacy = serde_json::to_value(&applied.frames[1]).unwrap();
        legacy.as_object_mut().unwrap().remove("base");
        assert!(
            serde_json::from_value::<model::Frame>(legacy)
                .unwrap()
                .base
                .is_none()
        );
        let refs: Vec<_> = applied
            .frames
            .iter()
            .map(|f| (f.id.as_str(), source.as_ref()))
            .collect();
        let rebuilt = model::Conversion::analyze(
            &refs,
            true,
            None,
            applied.analysis_area,
            applied.calibration,
            &applied.frame_bases(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(rebuilt.for_frame(&applied.frames[1].id)).unwrap(),
            serde_json::to_value(applied.analysis_for(&applied.frames[1])).unwrap()
        );
        app.apply_action(
            Action::SetBase {
                frame: Some(1),
                base: None,
            },
            &ctx,
        );
        wait(&mut app);
        assert_eq!(
            serde_json::to_value(
                app.pending_analysis
                    .as_ref()
                    .unwrap()
                    .conversion
                    .for_frame(&original.frames[1].id)
            )
            .unwrap(),
            serde_json::to_value(original.analysis_for(&original.frames[1])).unwrap()
        );
        app.apply_action(Action::AcceptAnalysis, &ctx);
        app.apply_action(Action::Undo, &ctx);
        app.apply_action(Action::Undo, &ctx);
        assert_eq!(
            serde_json::to_value(&app.library.rolls[0]).unwrap(),
            serde_json::to_value(applied).unwrap()
        );
    }

    #[test]
    fn discard_or_keep_adjustments_preserves_the_existing_look_controls() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().into(), Library::default(), None);
        let mut roll = Roll::new("Keep corrections");
        roll.frames.push(model::Frame::new("frame.png".into()));
        roll.adjustments.brightness = 0.9;
        let analysis = film::Analysis {
            channel_scale: [1.; 3],
            illumination: [0.; 5],
            options: film::CalibrationOptions::default(),
            version: 3,
            base: [0.8, 0.5, 0.3],
            floor: [0.; 3],
            range: 2.9,
            method: "old".into(),
            warnings: vec![],
            frames: 1,
        };
        roll.analysis = Some(analysis.clone().into());
        app.library.rolls.push(roll);
        let before = serde_json::to_value(&app.library).unwrap();
        let proposal = model::ProposedConversion {
            roll_id: app.library.rolls[0].id.clone(),
            conversion: model::Conversion::from(analysis),
            frame_id: None,
        };
        app.pending_analysis = Some(proposal.clone());
        app.preview_analysis = true;
        app.apply_action(Action::DiscardAnalysis, &ctx);
        assert!(!app.preview_analysis);
        assert_eq!(serde_json::to_value(&app.library).unwrap(), before);
        app.pending_analysis = Some(proposal);
        app.neutral_analysis = false;
        app.apply_action(Action::AcceptAnalysis, &ctx);
        assert!((app.library.rolls[0].adjustments.brightness - 0.9).abs() < 1e-6);
    }

    #[test]
    fn r_rotates_contact_sheet_selection_atomically_and_shift_r_reverses_it() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, directory.path().to_owned(), Library::default(), None);
        let mut roll = Roll::new("Rotation selection");
        for rotation in 0..4 {
            let mut frame = model::Frame::new(format!("{rotation}.png").into());
            frame.rotation = rotation;
            frame.mirrored = rotation == 1;
            frame.review = true;
            roll.frames.push(frame);
        }
        roll.conversion_locked = true;
        app.library.rolls.push(roll);
        app.select_frame(0, false, false);
        app.select_frame(2, true, false);
        let before = serde_json::to_string(&app.library.rolls[0]).unwrap();
        let press = |app: &mut App, shift| {
            let _ = ctx.run(
                egui::RawInput {
                    events: [true, false]
                        .map(|pressed| egui::Event::Key {
                            key: egui::Key::R,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: egui::Modifiers {
                                shift,
                                ..Default::default()
                            },
                        })
                        .into(),
                    ..Default::default()
                },
                |ctx| {
                    app.edit_shortcuts(ctx);
                    for action in std::mem::take(&mut app.actions) {
                        app.apply_action(action, ctx);
                    }
                },
            );
        };
        press(&mut app, false);
        assert_eq!(
            app.library.rolls[0]
                .frames
                .iter()
                .map(|f| f.rotation)
                .collect::<Vec<_>>(),
            [1, 1, 3, 3]
        );
        assert!(app.grid && app.scope_roll); // Composition follows selection, not slider scope.
        assert!(
            app.library.rolls[0]
                .frames
                .iter()
                .all(|f| !f.rejected && f.review)
        );
        assert_eq!(app.history.len(), 1);
        app.apply_action(Action::Undo, &ctx);
        assert_eq!(
            serde_json::to_string(&app.library.rolls[0]).unwrap(),
            before
        );
        app.apply_action(Action::Redo, &ctx);
        press(&mut app, true);
        assert_eq!(
            serde_json::to_string(&app.library.rolls[0]).unwrap(),
            before
        );
        app.select_frame(1, false, false);
        press(&mut app, false);
        assert_eq!(
            app.library.rolls[0].frames[1].rotation, 0,
            "Mirrored photos turn clockwise on screen"
        );
        app.grid = false;
        app.frame = 3;
        press(&mut app, false);
        assert_eq!(app.library.rolls[0].frames[3].rotation, 0);
        assert_eq!(
            app.library.rolls[0].frames[1].rotation, 0,
            "Single frame ignores the contact-sheet selection"
        );
        let saved = model::load(&app.library_path).unwrap();
        assert_eq!(saved.rolls[0].frames[3].rotation, 0);
    }

    #[test]
    fn rotation_shortcuts_respect_text_modifiers_repeat_and_readonly_states() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, directory.path().to_owned(), Library::default(), None);
        let mut roll = Roll::new("Shortcut guards");
        roll.frames.push(model::Frame::new("frame.png".into()));
        app.library.rolls.push(roll);
        let input = |key, modifiers, repeat| egui::RawInput {
            events: [true, false]
                .map(|pressed| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat,
                    modifiers,
                })
                .into(),
            ..Default::default()
        };
        for modifiers in [
            egui::Modifiers::CTRL,
            egui::Modifiers::ALT,
            egui::Modifiers::COMMAND,
        ] {
            let _ = ctx.run(input(egui::Key::R, modifiers, false), |ctx| {
                app.edit_shortcuts(ctx);
            });
        }
        // egui derives repeat from held-key state, rather than trusting the flag.
        let mut held = input(egui::Key::R, egui::Modifiers::NONE, false);
        held.events.pop();
        let _ = ctx.run(held.clone(), |_| {});
        let _ = ctx.run(held, |ctx| {
            app.edit_shortcuts(ctx);
        });
        let _ = ctx.run(input(egui::Key::R, egui::Modifiers::NONE, false), |_| {});
        for state in 0..3 {
            app.busy = state == 0;
            app.comparing = state == 1;
            app.preview_analysis = state == 2;
            let _ = ctx.run(input(egui::Key::R, egui::Modifiers::NONE, false), |ctx| {
                app.edit_shortcuts(ctx);
            });
        }
        app.busy = false;
        app.comparing = false;
        app.preview_analysis = false;
        assert!(app.actions.is_empty());
        let _ = ctx.run(input(egui::Key::X, egui::Modifiers::NONE, false), |ctx| {
            app.edit_shortcuts(ctx);
            for action in std::mem::take(&mut app.actions) {
                app.apply_action(action, ctx);
            }
        });
        assert!(
            app.library.rolls[0].frames[0].rejected,
            "X retains the keep/reject shortcut"
        );
        let mut note = String::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.text_edit_singleline(&mut note).request_focus();
            });
        });
        let _ = ctx.run(input(egui::Key::R, egui::Modifiers::NONE, false), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.text_edit_singleline(&mut note);
            });
            app.edit_shortcuts(ctx);
        });
        assert!(app.actions.is_empty());
        assert_eq!(app.library.rolls[0].frames[0].rotation, 0);
    }

    #[test]
    fn auto_upright_is_atomic_preserves_composition_and_flags_uncertain_frames() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        let mut roll = Roll::new("Orientation");
        for index in 0..3 {
            let mut frame = model::Frame::new(format!("{index}.png").into());
            frame.rotation = 2;
            frame.mirrored = index % 2 == 0;
            frame.crop = film::Rect {
                x: 0.1,
                y: 0.1,
                w: 0.8,
                h: 0.8,
                angle: 2.,
            };
            roll.frames.push(frame);
        }
        roll.conversion_locked = true;
        let before = serde_json::to_string(&roll).unwrap();
        let results = roll
            .frames
            .iter()
            .enumerate()
            .map(|(index, frame)| jobs::OrientationResult {
                id: frame.id.clone(),
                previous_rotation: if index == 2 { 1 } else { 2 },
                previous_crop: frame.crop,
                previous_mirrored: frame.mirrored,
                guess: Ok(film::UprightGuess {
                    rotation: if index == 1 { None } else { Some(3) },
                    scores: [0., 0., 0., 0.95],
                    faces: [0, 0, 0, 1],
                    reason: "No clear faces".into(),
                }),
            })
            .collect();
        let id = roll.id.clone();
        app.library.rolls.push(roll);
        app.tx
            .send(Event::AutoOriented(
                id,
                Ok(jobs::OrientationBatch {
                    backend: "CPU".into(),
                    frames: results,
                }),
            ))
            .unwrap();
        app.drain(&ctx);
        let frames = &app.library.rolls[0].frames;
        assert_eq!(frames[0].rotation, 3);
        assert!(frames[0].mirrored && !frames[0].review);
        assert_eq!(frames[1].rotation, 2);
        assert!(frames[1].review && !frames[1].mirrored);
        assert_eq!(
            frames[2].rotation, 2,
            "Do not overwrite a newer manual rotation"
        );
        assert!(!frames[2].review);
        assert!(
            frames
                .iter()
                .all(|frame| (frame.crop.angle - 2.).abs() < f32::EPSILON)
        );
        assert!(app.grid && app.library.rolls[0].conversion_locked);
        assert_eq!(app.history.len(), 1);
        app.apply_action(Action::Undo, &ctx);
        assert_eq!(
            serde_json::to_string(&app.library.rolls[0]).unwrap(),
            before
        );
        app.apply_action(Action::Redo, &ctx);
        let restored = model::load(&app.library_path).unwrap();
        assert_eq!(restored.rolls[0].frames[0].rotation, 3);
        assert!(restored.rolls[0].frames[1].review);
    }

    #[test]
    fn auto_upright_requires_conversion_and_engine_failures_do_not_edit_frames() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        let mut roll = Roll::new("Unconverted");
        roll.frames.push(model::Frame::new("negative.cr3".into()));
        let id = roll.id.clone();
        app.library.rolls.push(roll);
        app.apply_action(Action::AutoUpright, &ctx);
        assert!(!app.busy && app.status.contains("Apply a conversion"));
        app.tx
            .send(Event::AutoOriented(id, Err("engine unavailable".into())))
            .unwrap();
        app.drain(&ctx);
        assert!(app.history.is_empty() && !app.library.rolls[0].frames[0].review);
        assert!(app.status.contains("engine unavailable"));
    }

    #[test]
    fn individual_proposal_and_mirror_respect_versions_locks_and_undo() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        let shared = film::Analysis {
            channel_scale: [1.; 3],
            illumination: [0.; 5],
            options: film::CalibrationOptions::default(),
            version: 2,
            base: [0.8, 0.5, 0.3],
            floor: [0.; 3],
            range: 1.,
            method: "fixture".into(),
            warnings: vec![],
            frames: 2,
        };
        let mut roll = Roll::new("Modes");
        roll.frames = vec![
            model::Frame::new("a.png".into()),
            model::Frame::new("b.png".into()),
        ];
        roll.analysis = Some(shared.clone().into());
        let frame_id = roll.frames[0].id.clone();
        app.library.rolls.push(roll);
        app.apply_action(Action::SetIndividual(true), &ctx);
        assert!(
            !app.displayed_conversion(&app.library.rolls[0])
                .unwrap()
                .individual
        );
        let mut single = shared.clone();
        single.range = 2.;
        let proposal = model::Conversion {
            frame_inputs: std::collections::BTreeMap::new(),
            inputs: None,
            shared,
            individual: true,
            per_frame: std::collections::BTreeMap::from([(frame_id.clone(), single)]),
        };
        app.tx
            .send(Event::Analysis(
                app.library.rolls[0].id.clone(),
                Ok(proposal),
            ))
            .unwrap();
        app.drain(&ctx);
        app.preview_analysis = true;
        let shown = app.displayed_conversion(&app.library.rolls[0]).unwrap();
        assert!(shown.individual);
        assert!(
            shown
                .for_frame(&app.library.rolls[0].frames[1].id)
                .is_none()
        );
        assert_eq!(
            shown.for_frame(&frame_id).unwrap().range.to_bits(),
            2_f32.to_bits()
        );
        app.library.rolls[0].conversion_locked = true;
        app.apply_action(Action::AcceptAnalysis, &ctx);
        assert!(app.pending_analysis.is_some());
        app.library.rolls[0].conversion_locked = false;
        app.apply_action(Action::AcceptAnalysis, &ctx);
        assert!(app.library.rolls[0].analysis.as_ref().unwrap().individual);
        app.comparing = true;
        assert!(
            !app.displayed_conversion(&app.library.rolls[0])
                .unwrap()
                .individual
        );
        app.comparing = false;
        app.dirty = false;
        app.apply_action(Action::Mirror, &ctx);
        assert!(app.library.rolls[0].frames[0].mirrored);
        app.apply_action(Action::OrientationToRoll, &ctx);
        assert!(app.library.rolls[0].frames.iter().all(|f| f.mirrored));
        app.apply_action(Action::Undo, &ctx);
        assert!(app.library.rolls[0].frames[0].mirrored);
        assert!(!app.library.rolls[0].frames[1].mirrored);
        app.apply_action(Action::Undo, &ctx);
        assert!(!app.library.rolls[0].frames[0].mirrored);
        app.apply_action(Action::Redo, &ctx);
        let restored = model::load(&app.library_path).unwrap();
        assert!(
            restored.rolls[0].frames[0].mirrored
                && restored.rolls[0].analysis.as_ref().unwrap().individual
        );
    }
    fn autocrop_fixture() -> Arc<Scan> {
        let mut pixels = vec![[0.00001; 3]; 240 * 160];
        for y in 18..147 {
            for x in 31..230 {
                pixels[y * 240 + x] = [0.4, 0.2, 0.1];
            }
        }
        let expected = film::Rect {
            x: 0.2,
            y: 0.2,
            w: 0.7,
            h: 0.65,
            angle: 2.,
        };
        for y in 0..160 {
            for x in 0..240 {
                let q =
                    expected.crop_point([(x as f32 + 0.5) / 240., (y as f32 + 0.5) / 160.], 1.5);
                if q.iter().all(|v| (0.0..=1.0).contains(v)) {
                    pixels[y * 240 + x] = [0.12, 0.06, 0.03];
                }
            }
        }
        Arc::new(Scan {
            image: greign_pipeline::LinearRgbImage {
                width: 240,
                height: 160,
                pixels,
            },
            camera_matrix: None,
            white_balance: [1.; 3],
            capture: "test".into(),
        })
    }

    #[test]
    fn batch_crop_handles_uncached_uncertain_and_missing_frames_as_one_edit() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        let scan = autocrop_fixture();
        let uncached = dir.path().join("uncached.png");
        let bytes: Vec<_> = scan
            .image
            .pixels
            .iter()
            .flatten()
            .map(|v| (v * 255.) as u8)
            .collect();
        image::save_buffer(&uncached, &bytes, 240, 160, image::ColorType::Rgb8).unwrap();
        let mut roll = Roll::new("Batch");
        roll.frames = [
            dir.path().join("cached.png"),
            uncached,
            dir.path().join("uncertain.png"),
            dir.path().join("missing.png"),
        ]
        .into_iter()
        .map(model::Frame::new)
        .collect();
        for (i, frame) in roll.frames.iter_mut().enumerate() {
            frame.rotation = i as u8;
            frame.mirrored = i % 2 == 0;
        }
        roll.frames[1].rejected = true; // "All frames" includes the whole contact sheet.
        roll.frames[2].crop = film::Rect {
            x: 0.1,
            y: 0.1,
            w: 0.8,
            h: 0.8,
            angle: 0.,
        };
        roll.conversion_locked = true;
        roll.analysis = Some(
            film::analyze(&[&scan], None, film::Rect::default())
                .unwrap()
                .into(),
        );
        let analysis = serde_json::to_string(&roll.analysis).unwrap();
        let before: Vec<_> = roll.frames.iter().map(|f| f.crop).collect();
        app.cache.insert(roll.frames[0].id.clone(), scan.clone());
        let mut uniform = (*scan).clone();
        uniform.image.pixels.fill([0.2; 3]);
        app.cache
            .insert(roll.frames[2].id.clone(), Arc::new(uniform));
        app.library.rolls.push(roll);
        app.apply_action(Action::AutoCropAll, &ctx);
        assert!(app.busy && app.grid);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while app.busy {
            app.drain(&ctx);
            if app.busy {
                assert!(std::time::Instant::now() < deadline, "{}", app.status);
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let after: Vec<_> = app.library.rolls[0].frames.iter().map(|f| f.crop).collect();
        assert_ne!(after[0], before[0]);
        assert_ne!(after[1], before[1]);
        assert_eq!(&after[2..], &before[2..]);
        assert!(
            app.status.contains("2 of 4")
                && app.status.contains("uncertain.png")
                && app.status.contains("missing.png"),
            "{}",
            app.status
        );
        assert_eq!(app.history.len(), 1);
        assert_eq!(
            serde_json::to_string(&app.library.rolls[0].analysis).unwrap(),
            analysis
        );
        for (i, frame) in app.library.rolls[0].frames.iter().enumerate() {
            assert_eq!(frame.rotation, i as u8);
            assert_eq!(frame.mirrored, i % 2 == 0);
        }
        app.apply_action(Action::Undo, &ctx);
        assert_eq!(
            app.library.rolls[0]
                .frames
                .iter()
                .map(|f| f.crop)
                .collect::<Vec<_>>(),
            before
        );
        app.apply_action(Action::Redo, &ctx);
        let restored = model::load(&app.library_path).unwrap();
        assert_eq!(
            restored.rolls[0]
                .frames
                .iter()
                .map(|f| f.crop)
                .collect::<Vec<_>>(),
            after
        );
    }

    #[test]
    fn batch_result_leaves_newer_crops_intact_and_empty_batch_has_no_undo() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        let mut roll = Roll::new("Edited while detecting");
        let mut frame = model::Frame::new("manual.png".into());
        frame.crop = film::Rect {
            x: 0.2,
            y: 0.2,
            w: 0.6,
            h: 0.6,
            angle: 0.,
        };
        let current = frame.crop;
        let result = jobs::CropResult {
            id: frame.id.clone(),
            previous: film::Rect::default(),
            crop: Ok(Some(film::Rect {
                angle: 1.,
                ..current
            })),
        };
        roll.frames.push(frame);
        let id = roll.id.clone();
        app.library.rolls.push(roll);
        app.tx.send(Event::AutoCropped(id, vec![result])).unwrap();
        app.drain(&ctx);
        assert_eq!(app.library.rolls[0].frames[0].crop, current);
        assert!(app.history.is_empty());
        assert!(
            app.status.contains("No crops changed")
                && app.status.contains("edited during detection")
        );
    }

    #[test]
    fn mirror_all_preserves_individual_rotation_and_crop_and_undoes_mixed_state() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        let mut roll = Roll::new("Mixed orientations");
        for rotation in 0..4 {
            let mut frame = model::Frame::new(format!("{rotation}.png").into());
            frame.rotation = rotation;
            frame.mirrored = rotation % 2 == 0;
            frame.crop = film::Rect {
                x: 0.1,
                y: 0.1,
                w: 0.8,
                h: 0.8,
                angle: 2.,
            };
            roll.frames.push(frame);
        }
        let before = serde_json::to_string(&roll).unwrap();
        app.library.rolls.push(roll);
        app.apply_action(Action::MirrorAll(true), &ctx);
        for (i, frame) in app.library.rolls[0].frames.iter().enumerate() {
            assert!(frame.mirrored);
            assert_eq!(frame.rotation, i as u8);
            assert!((frame.crop.angle - 2.).abs() < f32::EPSILON);
        }
        app.apply_action(Action::MirrorAll(true), &ctx);
        assert_eq!(app.history.len(), 1, "Repeated setting is not another edit");
        app.apply_action(Action::MirrorAll(false), &ctx);
        assert!(app.library.rolls[0].frames.iter().all(|f| !f.mirrored));
        app.apply_action(Action::Undo, &ctx);
        app.apply_action(Action::Undo, &ctx);
        assert_eq!(
            serde_json::to_string(&app.library.rolls[0]).unwrap(),
            before
        );
        assert!(app.grid);
    }

    #[test]
    fn auto_crop_is_undoable_and_preserves_accepted_conversion() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        let mut roll = Roll::new("Holder");
        let frame = model::Frame::new("scan.png".into());
        let scan = autocrop_fixture();
        roll.analysis = Some(
            film::analyze(&[&scan], None, film::Rect::default())
                .unwrap()
                .into(),
        );
        roll.conversion_locked = true;
        let analysis = serde_json::to_string(&roll.analysis).unwrap();
        app.cache.insert(frame.id.clone(), scan);
        roll.frames.push(frame);
        app.library.rolls.push(roll);
        app.apply_action(Action::AutoCrop, &ctx);
        let cropped = app.library.rolls[0].frames[0].crop;
        assert_ne!(cropped, film::Rect::default());
        assert!((cropped.angle - 2.).abs() < 0.3, "{cropped:?}");
        assert_eq!(
            serde_json::to_string(&app.library.rolls[0].analysis).unwrap(),
            analysis
        );
        app.apply_action(Action::Undo, &ctx);
        assert_eq!(app.library.rolls[0].frames[0].crop, film::Rect::default());
        app.apply_action(Action::Redo, &ctx);
        assert_eq!(app.library.rolls[0].frames[0].crop, cropped);
        let reloaded = model::load(&app.library_path).unwrap();
        assert_eq!(reloaded.rolls[0].frames[0].crop, cropped);
        app.apply_action(Action::ResetCrop, &ctx);
        assert_eq!(app.library.rolls[0].frames[0].crop, film::Rect::default());
    }
    #[test]
    fn histogram_discards_old_results_and_counts_the_current_crop() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        let mut roll = Roll::new("Histogram");
        roll.material = Material::Positive;
        let frame = model::Frame::new("fixture.png".into());
        app.cache.insert(
            frame.id.clone(),
            Arc::new(Scan {
                image: greign_pipeline::LinearRgbImage {
                    width: 9,
                    height: 9,
                    pixels: vec![[0.2, 0.4, 0.6]; 81],
                },
                camera_matrix: None,
                white_balance: [1.; 3],
                capture: "fixture".into(),
            }),
        );
        roll.frames.push(frame);
        app.library.rolls.push(roll);
        app.request_histogram(&ctx);
        app.library.rolls[0].frames[0].crop = film::Rect {
            x: 0.,
            y: 0.,
            w: 0.5,
            h: 0.5,
            angle: 0.0,
        };
        app.request_histogram(&ctx); // changes desired key, without queuing another worker
        let event = app.rx.recv_timeout(Duration::from_secs(5)).unwrap();
        app.tx.send(event).unwrap();
        app.drain(&ctx);
        assert!(app.histogram.is_none());
        assert!(!app.histogram_pending);
        app.request_histogram(&ctx);
        let event = app.rx.recv_timeout(Duration::from_secs(5)).unwrap();
        app.tx.send(event).unwrap();
        app.drain(&ctx);
        for channel in app.histogram.as_ref().unwrap().iter() {
            assert_eq!(channel.iter().sum::<u32>(), 16);
        }
    }
    #[test]
    fn neutral_pick_respects_scope_is_undoable_and_rejects_stale_results() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        let mut roll = Roll::new("Neutral test");
        roll.material = Material::Positive;
        roll.adjustments.warmth = 5.;
        roll.frames.push(model::Frame::new("sample.png".into()));
        roll.frames[0].adjustments.warmth = 3.;
        app.library.rolls.push(roll);
        app.scope_roll = true;
        app.tx
            .send(Event::Neutral {
                key: app.neutral_key(),
                result: Ok([12., -4.]),
            })
            .unwrap();
        app.drain(&ctx);
        assert!((app.library.rolls[0].adjustments.warmth - 9.).abs() < 0.0001);
        assert!((app.library.rolls[0].frames[0].adjustments.warmth - 3.).abs() < 0.0001);
        app.apply_action(Action::Undo, &ctx);
        assert!((app.library.rolls[0].adjustments.warmth - 5.).abs() < 0.0001);
        let stale = app.neutral_key();
        app.invalidate();
        app.tx
            .send(Event::Neutral {
                key: stale,
                result: Ok([40., 40.]),
            })
            .unwrap();
        app.drain(&ctx);
        assert!((app.library.rolls[0].adjustments.warmth - 5.).abs() < 0.0001);
        app.scope_roll = false;
        app.tx
            .send(Event::Neutral {
                key: app.neutral_key(),
                result: Ok([12., -4.]),
            })
            .unwrap();
        app.drain(&ctx);
        assert!((app.library.rolls[0].frames[0].adjustments.warmth - 7.).abs() < 0.0001);
    }
    #[test]
    fn export_shortcut_opens_settings_without_starting_a_job() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        app.library.rolls.push(Roll::new("Shortcut test"));
        for busy in [false, true] {
            app.export_dialog = false;
            app.busy = busy;
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: egui::Key::E,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers {
                            ctrl: true,
                            command: true,
                            ..Default::default()
                        },
                    }],
                    ..Default::default()
                },
                |ctx| app.edit_shortcuts(ctx),
            );
            assert_eq!(app.export_dialog, !busy);
            assert!(app.actions.is_empty());
        }
    }
    #[test]
    fn clipboard_shortcuts_paste_groups_to_selection_and_leave_text_editing_alone() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        let mut roll = Roll::new("Clipboard");
        roll.adjustments.warmth = 7.;
        for name in ["private-source.png", "target.png", "third.png"] {
            roll.frames.push(model::Frame::new(name.into()));
        }
        roll.frames[0].adjustments.shadows = 45.;
        roll.frames[0].crop = film::Rect {
            x: 0.1,
            y: 0.1,
            w: 0.8,
            h: 0.8,
            angle: 0.0,
        };
        app.selected = roll.frames[1..].iter().map(|f| f.id.clone()).collect();
        app.library.rolls.push(roll);
        let output = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Copy],
                ..Default::default()
            },
            |ctx| {
                app.edit_shortcuts(ctx);
                for action in std::mem::take(&mut app.actions) {
                    app.apply_action(action, ctx);
                }
            },
        );
        let text = output
            .platform_output
            .commands
            .iter()
            .find_map(|c| {
                if let egui::OutputCommand::CopyText(text) = c {
                    Some(text.clone())
                } else {
                    None
                }
            })
            .unwrap();
        assert!(!text.contains("private-source"));
        app.clipboard = None; // prove paste uses the OS payload, not only internal state
        let before = app.library.clone();
        app.remember(before);
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Paste(text.clone())],
                ..Default::default()
            },
            |ctx| {
                app.edit_shortcuts(ctx);
                for action in std::mem::take(&mut app.actions) {
                    app.apply_action(action, ctx);
                }
            },
        );
        assert!(app.scope_selected && !app.scope_roll);
        for frame in &app.library.rolls[0].frames[1..] {
            assert!((frame.adjustments.shadows - 45.).abs() < 1e-6);
            assert_eq!(frame.crop, film::Rect::default());
        }
        assert!((app.library.rolls[0].adjustments.warmth - 7.).abs() < 1e-6);
        app.apply_action(Action::Undo, &ctx);
        assert!(app.library.rolls[0].frames[1].adjustments.shadows.abs() < 1e-6);
        let mut entry = "notes".to_owned();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.text_edit_singleline(&mut entry).request_focus();
            });
        });
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Paste(text)],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.text_edit_singleline(&mut entry);
                });
                app.edit_shortcuts(ctx);
            },
        );
        assert!(app.actions.is_empty());
        assert!(entry.contains("greign-frame-edits-v1"));
        assert!(model::EditClipboard::decode("ordinary clipboard text").is_none());
    }
    #[test]
    fn frame_edits_do_not_render_unchanged_neighbors() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        let mut roll = Roll::new("Two frames");
        roll.material = Material::Positive;
        for name in ["one", "two"] {
            let frame = model::Frame::new(name.into());
            app.cache.insert(
                frame.id.clone(),
                Arc::new(Scan {
                    image: greign_pipeline::LinearRgbImage {
                        width: 8,
                        height: 8,
                        pixels: vec![[0.2; 3]; 64],
                    },
                    camera_matrix: None,
                    white_balance: [1.0; 3],
                    capture: "fixture".into(),
                }),
            );
            roll.frames.push(frame);
        }
        app.library.rolls.push(roll);
        app.schedule_render(&ctx);
        let mut initial = Vec::new();
        while initial.len() < 2 {
            if let Event::PreviewReady(output) =
                app.rx.recv_timeout(Duration::from_secs(10)).unwrap()
            {
                initial.push(output);
            }
        }
        for output in initial {
            app.tx.send(Event::PreviewReady(output)).unwrap();
        }
        app.drain(&ctx);
        app.library.rolls[0].frames[0].adjustments.shadows = 20.0;
        app.invalidate();
        app.schedule_render(&ctx);
        let Event::PreviewReady(output) = app.rx.recv_timeout(Duration::from_secs(10)).unwrap()
        else {
            panic!("Expected render");
        };
        assert_eq!(output.id, app.library.rolls[0].frames[0].id);
        assert!(app.rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn compare_renders_saved_look_without_mutating_current_edits() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        let mut roll = Roll::new("Compare");
        roll.material = Material::Positive;
        let frame = model::Frame::new("source.png".into());
        app.cache.insert(
            frame.id.clone(),
            Arc::new(Scan {
                image: greign_pipeline::LinearRgbImage {
                    width: 8,
                    height: 8,
                    pixels: vec![[0.1, 0.2, 0.3]; 64],
                },
                camera_matrix: None,
                white_balance: [1.0; 3],
                capture: "test".into(),
            }),
        );
        roll.frames.push(frame);
        roll.save_version("Before");
        roll.adjustments.brightness = 2.0;
        app.library.rolls.push(roll);
        let state = serde_json::to_vec(&app.library).unwrap();
        let mut outputs = Vec::new();
        for comparing in [false, true, false] {
            app.comparing = comparing;
            app.invalidate();
            app.schedule_render(&ctx);
            loop {
                match app.rx.recv_timeout(Duration::from_secs(10)).unwrap() {
                    Event::PreviewReady(output) => {
                        assert!(output.error.is_none(), "{:?}", output.error);
                        outputs.push(output.bytes);
                        break;
                    }
                    Event::Failed(_, error) => panic!("{error}"),
                    _ => {}
                }
            }
        }
        assert_ne!(outputs[0], outputs[1]);
        assert_eq!(outputs[0], outputs[2]);
        assert_eq!(serde_json::to_vec(&app.library).unwrap(), state);
    }

    #[test]
    fn selection_edits_preserve_offsets_and_reanalysis_requires_acceptance() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        let mut roll = Roll::new("Selection");
        for name in ["one", "two", "three"] {
            roll.frames.push(model::Frame::new(name.into()));
        }
        roll.frames[0].adjustments.shadows = 10.0;
        roll.frames[1].adjustments.shadows = 25.0;
        app.library.rolls.push(roll);
        app.select_frame(0, false, false);
        app.select_frame(1, true, false);
        app.scope_roll = false;
        app.scope_selected = true;
        let previous = app.library.rolls[0].frames[1].adjustments;
        let mut next = previous;
        next.shadows += 15.0;
        next.colors[0].hue = 10.0;
        app.change_adjustments(previous, next);
        let r = &app.library.rolls[0];
        assert!((r.frames[0].adjustments.shadows - 25.0).abs() < 1e-6);
        assert!((r.frames[1].adjustments.shadows - 40.0).abs() < 1e-6);
        assert!(r.frames[2].adjustments.shadows.abs() < 1e-6);
        let old = film::Analysis {
            channel_scale: [1.; 3],
            illumination: [0.; 5],
            options: film::CalibrationOptions::default(),
            version: 1,
            base: [0.8, 0.5, 0.3],
            floor: [0.0; 3],
            range: 1.0,
            method: "old".into(),
            warnings: vec![],
            frames: 3,
        };
        app.library.rolls[0].analysis = Some(old.clone().into());
        app.library.rolls[0].conversion_locked = true;
        let mut new = old;
        new.range = 2.0;
        app.tx
            .send(Event::Analysis(
                app.library.rolls[0].id.clone(),
                Ok(new.into()),
            ))
            .unwrap();
        app.drain(&ctx);
        assert_eq!(
            app.library.rolls[0]
                .analysis
                .as_ref()
                .unwrap()
                .shared
                .range
                .to_bits(),
            1.0_f32.to_bits()
        );
        assert!(app.pending_analysis.is_some());
        app.apply_action(Action::AcceptAnalysis, &ctx);
        assert!(app.pending_analysis.is_some());
        app.library.rolls[0].conversion_locked = false;
        app.apply_action(Action::AcceptAnalysis, &ctx);
        assert!(app.pending_analysis.is_none());
        assert_eq!(
            app.library.rolls[0]
                .analysis
                .as_ref()
                .unwrap()
                .shared
                .range
                .to_bits(),
            2.0_f32.to_bits()
        );
        assert_eq!(app.library.rolls[0].versions.len(), 1);
    }

    #[test]
    fn empty_library_actions_and_stale_previews_are_safe() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        app.apply_action(Action::Reject, &ctx);
        app.apply_action(Action::Rotate, &ctx);
        app.revision = 3;
        app.rendering = true;
        app.tx
            .send(Event::PreviewReady(Box::new(preview::Output {
                id: "old-frame".into(),
                key: "stale".into(),
                quality: preview::Quality::Preview,
                width: 1,
                height: 1,
                bytes: vec![0; 3],
                error: None,
            })))
            .unwrap();
        app.drain(&ctx);
        assert!(app.textures.is_empty());
        assert!(app.rendering); // stale results cannot complete a current request
    }

    #[test]
    fn undo_redo_restore_frame_state_without_changing_roll_adjustments() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().to_owned(), Library::default(), None);
        let mut roll = Roll::new("Test");
        roll.adjustments.warmth = 12.0;
        roll.frames.push(model::Frame::new("source.CR3".into()));
        app.library.rolls.push(roll);
        app.remember(app.library.clone());
        app.apply_action(Action::Reject, &ctx);
        assert!(app.library.rolls[0].frames[0].rejected);
        app.apply_action(Action::Undo, &ctx);
        assert!(!app.library.rolls[0].frames[0].rejected);
        app.apply_action(Action::Redo, &ctx);
        assert!(app.library.rolls[0].frames[0].rejected);
        assert!((app.library.rolls[0].adjustments.warmth - 12.0).abs() < f32::EPSILON);
    }
}
