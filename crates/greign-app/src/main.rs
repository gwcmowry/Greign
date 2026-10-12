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
use eframe::egui::{self, TextureHandle};
use greign_film::{self as film, Adjustments, Material, Scan};
use jobs::Event;
use model::{Library, Roll};
use std::{
    collections::{HashMap, HashSet},
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
}
#[derive(Clone, Copy)]
enum Action {
    Import,
    Load,
    Analyze,
    Export,
    Undo,
    Redo,
    Reset,
    Rotate,
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
    pending_analysis: Option<(String, film::Analysis)>,
    preview_analysis: bool,
    export_retry: Option<Box<jobs::ExportRetry>>,
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
            preview_analysis: false,
            export_retry: None,
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
                            if r.analysis.is_some() || r.conversion_locked {
                                self.pending_analysis = Some((id, a));
                                self.preview_analysis = false;
                                self.status = "New analysis ready · preview it in Conversion before accepting".into();
                                continue;
                            }
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
                    Err(e) => self.status = format!("Analysis needs attention: {e}"),
                },
                Event::Finished => {
                    self.busy = false;
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
                    self.export_retry = retry;
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
            if let Some((id, analysis)) = &self.pending_analysis {
                if *id == roll.id {
                    roll.analysis = Some(analysis.clone());
                }
            }
        }
        let negative = (!self.comparing && self.show_negative)
            || (roll.analysis.is_none() && roll.material != Material::Positive);
        let scene = serde_json::to_string(&(roll.material, &roll.analysis, negative)).unwrap();
        let active = roll.frames.get(self.frame).map(|f| f.id.clone());
        let mut requests = Vec::new();
        for frame in &roll.frames {
            if self.dragging && Some(&frame.id) != active.as_ref() {
                continue;
            }
            let Some(scan) = self.cache.get(&frame.id) else {
                continue;
            };
            let quality = if self.dragging {
                preview::Quality::Draft
            } else {
                preview::Quality::Preview
            };
            let scene_key = format!("{}:{:p}:{quality:?}:{scene}", frame.id, Arc::as_ptr(scan));
            let edits = roll.adjustments.combined(frame.adjustments);
            let key = serde_json::to_string(&(
                &scene_key,
                edits,
                frame.crop,
                frame.rotation,
                &frame.heals,
            ))
            .unwrap();
            requests.push(preview::Request {
                key,
                scene_key,
                frame: frame.clone(),
                scan: scan.clone(),
                material: roll.material,
                analysis: roll.analysis.clone(),
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
            && !negative
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
                    r.scene_key = format!("{}:{:p}:Full:{scene}", r.frame.id, Arc::as_ptr(&r.scan));
                    r.key = serde_json::to_string(&(
                        &r.scene_key,
                        r.edits,
                        r.frame.crop,
                        r.frame.rotation,
                        &r.frame.heals,
                    ))
                    .unwrap();
                    if r.gpu {
                        r.key = r.scene_key.clone();
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
            Action::Import => {
                if let Some(paths) = rfd::FileDialog::new()
                    .add_filter(
                        "Scans",
                        &[
                            "cr3", "CR3", "cr2", "nef", "arw", "dng", "orf", "rw2", "raf", "tif",
                            "tiff", "jpg", "jpeg", "png",
                        ],
                    )
                    .pick_files()
                {
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
                self.preview_analysis = false;
                self.pending_analysis = None;
                self.invalidate();
                self.begin_load(ctx, true);
            }
            Action::Export => {
                if self.busy || self.preview_analysis {
                    return;
                }
                let roll = self.library.rolls[self.roll].clone();
                if let Err(error) = greign_export::CaptureDate::parse(&roll.date) {
                    self.status = format!("Roll date: {error}");
                    return;
                }
                let only = match self.export_scope {
                    ExportScope::Roll => None,
                    ExportScope::Selected => Some(self.selected.clone()),
                    ExportScope::Frame => Some(
                        roll.frames
                            .get(self.frame)
                            .map(|f| f.id.clone())
                            .into_iter()
                            .collect(),
                    ),
                };
                if !roll.frames.iter().any(|f| {
                    !f.rejected
                        && only
                            .as_ref()
                            .is_none_or(|ids: &HashSet<String>| ids.contains(&f.id))
                }) {
                    self.status = "No kept frames in the chosen export scope".into();
                    return;
                }
                if roll.frames.iter().all(|f| f.rejected)
                    || (roll.material != Material::Positive && roll.analysis.is_none())
                {
                    return;
                }
                let directory = self.library.rolls[self.roll]
                    .export
                    .destination
                    .clone()
                    .unwrap_or_else(|| self.export_dir.clone())
                    .join(format!("roll-{}", roll.id))
                    .join(format!("export-{}", model::id()));
                if let Err(e) = std::fs::create_dir_all(&directory) {
                    self.status = e.to_string();
                    return;
                }
                if let Err(e) = std::fs::write(
                    directory.join("roll-recipe.json"),
                    serde_json::to_vec_pretty(&roll).unwrap_or_default(),
                ) {
                    self.status = e.to_string();
                    return;
                }
                let selected_ids: Vec<_> = roll
                    .frames
                    .iter()
                    .filter(|f| !f.rejected && only.as_ref().is_none_or(|ids| ids.contains(&f.id)))
                    .map(|f| &f.id)
                    .collect();
                if let Err(e) = std::fs::write(
                    directory.join("export-selection.json"),
                    serde_json::to_vec_pretty(&selected_ids).unwrap_or_default(),
                ) {
                    self.status = e.to_string();
                    return;
                }
                self.busy = true;
                self.export_progress = None;
                jobs::export_with_mode(
                    roll,
                    directory,
                    only,
                    self.tx.clone(),
                    ctx.clone(),
                    self.profile(),
                    if self.gpu.is_some() && std::env::var_os("GREIGN_CPU_EXPORT").is_none() {
                        jobs::ExportMode::Gpu
                    } else {
                        jobs::ExportMode::Cpu
                    },
                );
            }
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
            Action::Rotate => {
                if let Some(f) = self.library.rolls[self.roll].frames.get_mut(self.frame) {
                    f.rotation = (f.rotation + 1) % 4;
                    self.dirty = true;
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
                let r = &mut self.library.rolls[self.roll];
                if r.conversion_locked {
                    self.status = "Unlock conversion before accepting new analysis".into();
                    return;
                }
                if let Some((id, a)) = self.pending_analysis.take() {
                    if id == r.id {
                        r.save_version("Before re-analysis");
                        r.previous_analysis = r.analysis.replace(a);
                        self.preview_analysis = false;
                        self.dirty = true;
                        self.status =
                            "New analysis accepted · prior look saved as a version".into();
                    } else {
                        self.pending_analysis = Some((id, a));
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
                    if let Some(retry) = self.export_retry.take() {
                        self.busy = true;
                        self.export_progress = None;
                        jobs::export_with_mode(
                            retry.roll,
                            retry.directory,
                            Some(retry.ids),
                            self.tx.clone(),
                            ctx.clone(),
                            self.profile(),
                            if self.gpu.is_some() && std::env::var_os("GREIGN_CPU_EXPORT").is_none()
                            {
                                jobs::ExportMode::Gpu
                            } else {
                                jobs::ExportMode::Cpu
                            },
                        );
                    }
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
                    self.tool = Tool::View;
                    self.move_source = false;
                }
                if i.key_pressed(egui::Key::R) && !self.busy {
                    self.actions.push(Action::Reject);
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
                && !self.textures.is_empty()
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
    std::fs::create_dir_all(root.join(".greign"))?;
    if args
        .iter()
        .any(|a| a == "--gpu-benchmark" || a == "--gpu-export-check" || a == "--jpeg-export-check")
    {
        struct Close;
        impl eframe::App for Close {
            fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        let jpeg_check = args.iter().any(|a| a == "--jpeg-export-check");
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
            .with_inner_size([1440.0, 960.0])
            .with_min_inner_size([1080.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Greign",
        options,
        Box::new(move |cc| {
            let mut app = App::new(&cc.egui_ctx, root, library, screenshot);
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
            version: 1,
            base: [0.8, 0.5, 0.3],
            floor: [0.0; 3],
            range: 1.0,
            method: "old".into(),
            warnings: vec![],
            frames: 3,
        };
        app.library.rolls[0].analysis = Some(old.clone());
        app.library.rolls[0].conversion_locked = true;
        let mut new = old;
        new.range = 2.0;
        app.tx
            .send(Event::Analysis(app.library.rolls[0].id.clone(), Ok(new)))
            .unwrap();
        app.drain(&ctx);
        assert_eq!(
            app.library.rolls[0]
                .analysis
                .as_ref()
                .unwrap()
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
