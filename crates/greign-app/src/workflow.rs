//! Folder imports and a sequential queue of frozen roll exports.
use crate::{
    App, ExportScope, jobs,
    model::{self, ExportPreset, Frame, Roll},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{HashSet, VecDeque},
    fmt::Write as _,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::Instant,
};

/// Only direct image files belong to this roll; nested folders remain separate rolls.
pub fn folder_scans(folder: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in
        fs::read_dir(folder).with_context(|| format!("Cannot read {}", folder.display()))?
    {
        let path = entry?.path();
        if path.is_file() && model::supported(&path) {
            paths.push(path);
        }
    }
    paths.sort();
    ensure!(
        !paths.is_empty(),
        "No supported scans in this folder (subfolders are separate rolls)"
    );
    Ok(paths)
}

pub fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Produce one portable path component, retaining readable Unicode roll names.
pub fn export_folder_name(name: &str, suffix: &str) -> String {
    let clean = |value: &str| -> String {
        value
            .chars()
            .map(|c| {
                if c.is_control() || "/\\:*?\"<>|".contains(c) {
                    '_'
                } else {
                    c
                }
            })
            .collect()
    };
    let name = clean(name);
    let name = name.trim().trim_matches('.').trim();
    let name = if name.is_empty() {
        "Untitled roll"
    } else {
        name
    };
    let mut result = String::new();
    for c in format!("{name}{}", clean(suffix)).chars() {
        if result.len() + c.len_utf8() > 200 {
            break;
        }
        result.push(c);
    }
    result.truncate(result.trim_end_matches([' ', '.']).len());
    let stem = result
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|n| n.len() == 1 && (b'1'..=b'9').contains(&n.as_bytes()[0]))
        })
    {
        result.insert(0, '_');
    }
    result
}

pub fn export_ready(roll: &Roll, only: Option<&HashSet<String>>) -> Result<usize> {
    greign_export::CaptureDate::parse(&roll.date).map_err(|e| anyhow::anyhow!("Roll date: {e}"))?;
    let frames: Vec<_> = roll
        .frames
        .iter()
        .filter(|f| !f.rejected && only.is_none_or(|ids| ids.contains(&f.id)))
        .collect();
    ensure!(!frames.is_empty(), "No kept frames");
    ensure!(
        roll.material == greign_film::Material::Positive
            || frames.iter().all(|f| roll.analysis_for(f).is_some()),
        "Analyze every kept frame first"
    );
    Ok(frames.len())
}

/// Atomically reserve a fresh sibling directory, including for duplicate roll names.
pub fn prepare_export(
    roll: &Roll,
    only: Option<&HashSet<String>>,
    fallback: &Path,
) -> Result<PathBuf> {
    export_ready(roll, only)?;
    let root = roll.export.destination.as_deref().unwrap_or(fallback);
    fs::create_dir_all(root).with_context(|| format!("Cannot create {}", root.display()))?;
    let name = export_folder_name(&roll.name, &roll.export.folder_suffix);
    let directory = (1..)
        .find_map(|n| {
            let path = root.join(if n == 1 {
                name.clone()
            } else {
                format!("{name} ({n})")
            });
            match fs::create_dir(&path) {
                Ok(()) => Some(Ok(path)),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .expect("Unbounded destination search")?;
    let selected: Vec<_> = roll
        .frames
        .iter()
        .filter(|f| !f.rejected && only.is_none_or(|ids| ids.contains(&f.id)))
        .map(|f| &f.id)
        .collect();
    fs::write(
        directory.join("roll-recipe.json"),
        serde_json::to_vec_pretty(roll)?,
    )?;
    fs::write(
        directory.join("export-selection.json"),
        serde_json::to_vec_pretty(&selected)?,
    )?;
    Ok(directory)
}

pub struct ExportTask {
    pub roll: Roll,
    pub only: Option<HashSet<String>>,
    pub directory: Option<PathBuf>,
}
pub struct ExportBatch {
    pub queue: VecDeque<ExportTask>,
    pub total: usize,
    pub completed: usize,
    pub frames: usize,
    pub bytes: u64,
    pub last_directory: Option<PathBuf>,
    pub errors: Vec<String>,
    pub started: Instant,
}

impl App {
    pub fn choose_import_folder(&mut self, folder: PathBuf) {
        match folder_scans(&folder) {
            Ok(paths) => {
                if self.new_name.trim().is_empty()
                    || self
                        .new_folder
                        .as_ref()
                        .is_some_and(|p| self.new_name == folder_name(p))
                {
                    self.new_name = folder_name(&folder);
                }
                self.new_paths = paths;
                self.new_folder = Some(folder);
                self.new_import_error = None;
            }
            Err(error) => {
                self.new_paths.clear();
                self.new_folder = None;
                self.new_import_error = Some(error.to_string());
            }
        }
    }
    pub fn import_new_roll(&mut self, ctx: &eframe::egui::Context) {
        if self.busy || self.new_paths.is_empty() {
            return;
        }
        let before = self.library.clone();
        let mut roll = Roll::new(if self.new_name.trim().is_empty() {
            "Untitled roll"
        } else {
            self.new_name.trim()
        });
        roll.stock.clone_from(&self.new_stock);
        roll.material = self.new_material;
        roll.source_folder = self.new_folder.take();
        roll.export = self.library.export_defaults.clone();
        roll.frames = std::mem::take(&mut self.new_paths)
            .into_iter()
            .map(Frame::new)
            .collect();
        self.library.rolls.push(roll);
        self.roll = self.library.rolls.len() - 1;
        self.frame = 0;
        self.selected.clear();
        self.scope_selected = false;
        self.version_index = 0;
        self.pending_analysis = None;
        self.preview_analysis = false;
        self.new_roll_open = false;
        self.new_name.clear();
        self.new_stock.clear();
        self.new_import_error = None;
        self.remember(before);
        self.save();
        self.begin_load(ctx, false);
    }
    pub fn set_export_defaults(&mut self, preset: ExportPreset) {
        for roll in &mut self.library.rolls {
            roll.export = preset.clone();
        }
        self.library.export_defaults = preset;
        self.status = "Export defaults saved for existing and future rolls".into();
        self.dirty = true;
    }
    pub fn start_export(&mut self, ctx: &eframe::egui::Context) {
        if self.busy || self.preview_analysis || self.comparing {
            return;
        }
        let mut tasks = VecDeque::new();
        if self.export_scope == ExportScope::AllReady {
            for roll in &self.library.rolls {
                if export_ready(roll, None).is_ok() {
                    let mut roll = roll.clone();
                    roll.export = self.library.export_defaults.clone();
                    tasks.push_back(ExportTask {
                        roll,
                        only: None,
                        directory: None,
                    });
                }
            }
        } else if let Some(roll) = self.library.rolls.get(self.roll) {
            let only = match self.export_scope {
                ExportScope::Selected => Some(self.selected.clone()),
                ExportScope::Frame => Some(
                    roll.frames
                        .get(self.frame)
                        .map(|f| f.id.clone())
                        .into_iter()
                        .collect(),
                ),
                _ => None,
            };
            if let Err(error) = export_ready(roll, only.as_ref()) {
                self.status = error.to_string();
                return;
            }
            tasks.push_back(ExportTask {
                roll: roll.clone(),
                only,
                directory: None,
            });
        }
        self.start_export_tasks(tasks, ctx);
    }
    pub fn start_export_tasks(&mut self, tasks: VecDeque<ExportTask>, ctx: &eframe::egui::Context) {
        if tasks.is_empty() {
            self.status = "No rolls ready to export".into();
            return;
        }
        self.export_batch = Some(ExportBatch {
            total: tasks.len(),
            queue: tasks,
            completed: 0,
            frames: 0,
            bytes: 0,
            last_directory: None,
            errors: Vec::new(),
            started: Instant::now(),
        });
        self.save();
        self.next_export(ctx);
    }
    pub fn next_export(&mut self, ctx: &eframe::egui::Context) {
        while let Some(task) = self.export_batch.as_mut().and_then(|b| b.queue.pop_front()) {
            let directory = task.directory.map_or_else(
                || prepare_export(&task.roll, task.only.as_ref(), &self.export_dir),
                Ok,
            );
            match directory {
                Ok(directory) => {
                    self.busy = true;
                    self.export_progress = None;
                    self.status = format!("Exporting {}", task.roll.name);
                    jobs::export_with_mode(
                        task.roll,
                        directory,
                        task.only,
                        self.tx.clone(),
                        ctx.clone(),
                        self.profile(),
                        if self.gpu.is_some() && std::env::var_os("GREIGN_CPU_EXPORT").is_none() {
                            jobs::ExportMode::Gpu
                        } else {
                            jobs::ExportMode::Cpu
                        },
                    );
                    return;
                }
                Err(error) => {
                    let batch = self.export_batch.as_mut().unwrap();
                    batch.completed += 1;
                    batch.errors.push(format!("{}: {error}", task.roll.name));
                }
            }
        }
        if let Some(batch) = self.export_batch.take() {
            self.busy = false;
            self.status = format!(
                "Exported {} frames across {} rolls · {:.1} MB · {:.1}s",
                batch.frames,
                batch.total,
                batch.bytes as f64 / 1_000_000.,
                batch.started.elapsed().as_secs_f64()
            );
            if !batch.errors.is_empty() {
                let _ = write!(
                    self.status,
                    " · {} errors: {}",
                    batch.errors.len(),
                    batch.errors.join("; ")
                );
            }
            if batch.total > 1 {
                if let Some(directory) = batch.last_directory {
                    self.last_export = directory.parent().map(Path::to_owned);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, model::Library};
    use eframe::egui;
    use std::time::Duration;

    fn image(path: &Path) {
        image::RgbImage::from_pixel(64, 48, image::Rgb([80, 120, 160]))
            .save(path)
            .unwrap();
    }
    fn settle(app: &mut App, ctx: &egui::Context) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while app.busy {
            assert!(
                Instant::now() < deadline,
                "Job did not complete: {}",
                app.status
            );
            app.drain(ctx);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn positive(name: &str, paths: &[PathBuf]) -> Roll {
        let mut roll = Roll::new(name);
        roll.material = greign_film::Material::Positive;
        roll.frames = paths.iter().cloned().map(Frame::new).collect();
        roll
    }

    #[test]
    fn folder_import_filters_contents_names_roll_and_inherits_persisted_defaults() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Kyoto 2026 — Portra");
        fs::create_dir(&folder).unwrap();
        for name in ["02.png", "01.PNG"] {
            image(&folder.join(name));
        }
        fs::write(folder.join("notes.txt"), b"notes").unwrap();
        fs::write(folder.join("._sidecar.CR3"), b"not an image").unwrap();
        fs::create_dir(folder.join("nested.jpg")).unwrap();
        image(&folder.join("nested.jpg/03.png"));
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, root.path().into(), Library::default(), None);
        app.import_new_roll(&ctx);
        assert!(app.library.rolls.is_empty());
        let preset = ExportPreset {
            destination: Some(root.path().join("Converted")),
            folder_suffix: "_done".into(),
            jpeg_quality: 88,
            ..ExportPreset::default()
        };
        app.set_export_defaults(preset);
        app.choose_import_folder(folder.clone());
        assert_eq!(app.new_name, "Kyoto 2026 — Portra");
        assert_eq!(
            app.new_paths,
            vec![folder.join("01.PNG"), folder.join("02.png")]
        );
        app.new_material = greign_film::Material::Positive;
        app.import_new_roll(&ctx);
        settle(&mut app, &ctx);
        let roll = &app.library.rolls[0];
        assert_eq!(roll.source_folder.as_ref(), Some(&folder));
        assert_eq!(roll.export.jpeg_quality, 88);
        assert_eq!(roll.export.folder_suffix, "_done");
        assert!(roll.optimize_individually);
        let loaded = model::load(&app.library_path).unwrap();
        assert_eq!(loaded.export_defaults.folder_suffix, "_done");
        assert_eq!(loaded.rolls[0].source_folder, Some(folder.clone()));
        assert!(folder_scans(&folder.join("missing")).is_err());
        let empty = root.path().join("empty");
        fs::create_dir(&empty).unwrap();
        app.choose_import_folder(folder);
        app.choose_import_folder(empty);
        assert!(app.new_paths.is_empty() && app.new_import_error.is_some());
        let legacy: Library = serde_json::from_str(r#"{"version":1,"rolls":[]}"#).unwrap();
        assert_eq!(legacy.export_defaults.folder_suffix, " - converted");
    }

    #[test]
    fn export_directories_are_readable_unique_and_cannot_escape_the_master_folder() {
        let root = tempfile::tempdir().unwrap();
        let mut roll = positive("Kyoto 2026", &["a.png".into(), "b.png".into()]);
        let selected = HashSet::from([roll.frames[1].id.clone()]);
        let first = prepare_export(&roll, Some(&selected), root.path()).unwrap();
        assert_eq!(first, root.path().join("Kyoto 2026 - converted"));
        let ids: Vec<String> =
            serde_json::from_slice(&fs::read(first.join("export-selection.json")).unwrap())
                .unwrap();
        assert_eq!(ids, vec![roll.frames[1].id.clone()]);
        fs::write(first.join("keep.txt"), b"do not overwrite").unwrap();
        let a = roll.clone();
        let b = roll.clone();
        let outputs = std::thread::scope(|scope| {
            let one = scope.spawn(|| prepare_export(&a, None, root.path()).unwrap());
            let two = scope.spawn(|| prepare_export(&b, None, root.path()).unwrap());
            [one.join().unwrap(), two.join().unwrap()]
        });
        assert_ne!(outputs[0], outputs[1]);
        assert_eq!(
            fs::read(first.join("keep.txt")).unwrap(),
            b"do not overwrite"
        );
        roll.name = "../../CON".into();
        roll.export.folder_suffix = "/../done".into();
        let directory = prepare_export(&roll, None, root.path()).unwrap();
        assert_eq!(directory.parent(), Some(root.path()));
        assert_eq!(export_folder_name("CON", ""), "_CON");
        assert_eq!(export_folder_name("..", ""), "Untitled roll");
        assert!(export_folder_name(&"🌿".repeat(200), "_done").len() <= 200);
        let recipe: Roll =
            serde_json::from_slice(&fs::read(directory.join("roll-recipe.json")).unwrap()).unwrap();
        assert_eq!(recipe.name, roll.name);
    }

    #[test]
    fn multi_roll_queue_freezes_settings_continues_after_failure_and_retries_only_missing_frames() {
        let root = tempfile::tempdir().unwrap();
        let scan = root.path().join("scan.png");
        image(&scan);
        let missing = root.path().join("missing.png");
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, root.path().into(), Library::default(), None);
        let mut first = positive("Same roll", &[scan.clone(), missing.clone()]);
        let mut rejected = Frame::new(root.path().join("rejected.png"));
        rejected.rejected = true;
        first.frames.push(rejected);
        app.library.rolls = vec![
            first,
            positive("Same roll", std::slice::from_ref(&scan)),
            Roll::new("Not converted"),
        ];
        let mut invalid_date = positive("Invalid date", std::slice::from_ref(&scan));
        invalid_date.date = "2026-99-99".into();
        app.library.rolls.push(invalid_date);
        let destination = root.path().join("Collection");
        app.set_export_defaults(ExportPreset {
            destination: Some(destination.clone()),
            long_edge: 32,
            prefix: "proof".into(),
            ..ExportPreset::default()
        });
        app.export_scope = ExportScope::AllReady;
        app.start_export(&ctx);
        assert!(app.busy);
        assert_eq!(app.export_batch.as_ref().unwrap().total, 2);
        app.library.rolls[1].name = "Changed after starting".into();
        app.library.export_defaults.long_edge = 16;
        settle(&mut app, &ctx);
        assert!(
            app.status.contains("Exported 2 frames across 2 rolls"),
            "{}",
            app.status
        );
        assert!(app.status.contains("1 errors"));
        assert_eq!(app.export_retry.len(), 1);
        assert_eq!(app.last_export.as_ref(), Some(&destination));
        let folders = [
            destination.join("Same roll - converted"),
            destination.join("Same roll - converted (2)"),
        ];
        for folder in &folders {
            let jpeg = image::open(folder.join("proof-01-scan-positive.jpg")).unwrap();
            assert_eq!((jpeg.width(), jpeg.height()), (32, 24));
            let recipe: Roll =
                serde_json::from_slice(&fs::read(folder.join("roll-recipe.json")).unwrap())
                    .unwrap();
            assert_eq!(recipe.name, "Same roll");
            assert_eq!(recipe.export.long_edge, 32);
        }
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 2);
        let saved = folders[0].join("proof-01-scan-positive.jpg");
        let original = fs::read(&saved).unwrap();
        fs::copy(scan, missing).unwrap();
        app.apply_action(Action::RetryExport, &ctx);
        settle(&mut app, &ctx);
        assert!(app.export_retry.is_empty());
        assert_eq!(fs::read(saved).unwrap(), original);
        let retried = image::open(folders[0].join("proof-02-missing-positive.jpg")).unwrap();
        assert_eq!((retried.width(), retried.height()), (32, 24));
        assert_eq!(fs::read_dir(destination).unwrap().count(), 2);
    }

    #[test]
    fn destination_failure_does_not_stall_other_rolls() {
        let root = tempfile::tempdir().unwrap();
        let scan = root.path().join("scan.png");
        image(&scan);
        let occupied = root.path().join("occupied");
        fs::write(&occupied, b"original").unwrap();
        let mut bad = positive("Bad destination", std::slice::from_ref(&scan));
        bad.export.destination = Some(occupied.clone());
        let mut good = positive("Good destination", std::slice::from_ref(&scan));
        good.export.destination = Some(root.path().join("exports"));
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, root.path().into(), Library::default(), None);
        app.start_export_tasks(
            [bad, good]
                .map(|roll| ExportTask {
                    roll,
                    only: None,
                    directory: None,
                })
                .into(),
            &ctx,
        );
        settle(&mut app, &ctx);
        assert!(app.status.contains("Bad destination"));
        assert!(
            root.path()
                .join("exports/Good destination - converted/01-scan-positive.jpg")
                .exists()
        );
        assert_eq!(fs::read(occupied).unwrap(), b"original");
    }
}
