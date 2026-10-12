use anyhow::{Result, ensure};
use greign_film::{Adjustments, Analysis, Heal, Material, Rect};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Frame {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub rejected: bool,
    pub adjustments: Adjustments,
    pub crop: Rect,
    pub rotation: u8,
    pub heals: Vec<Heal>,
    pub error: Option<String>,
    #[serde(default)]
    pub review: bool,
    #[serde(default)]
    pub scan_warning: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Roll {
    pub id: String,
    pub name: String,
    pub stock: String,
    pub material: Material,
    pub film_camera: String,
    pub date: String,
    pub notes: String,
    pub demo: bool,
    pub frames: Vec<Frame>,
    pub adjustments: Adjustments,
    pub analysis: Option<Analysis>,
    pub previous_analysis: Option<Analysis>,
    pub base: Option<[f32; 3]>,
    pub analysis_area: Rect,
    #[serde(default)]
    pub conversion_locked: bool,
    #[serde(default)]
    pub versions: Vec<SavedVersion>,
    #[serde(default)]
    pub export: ExportPreset,
    #[serde(default)]
    pub export_presets: Vec<ExportPreset>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SavedVersion {
    pub name: String,
    pub created: String,
    pub state: RollState,
}

// No nested versions or file data: snapshots contain only reproducible edit state.
#[derive(Clone, Serialize, Deserialize)]
pub struct RollState {
    pub adjustments: Adjustments,
    pub analysis: Option<Analysis>,
    pub base: Option<[f32; 3]>,
    pub analysis_area: Rect,
    pub conversion_locked: bool,
    pub frames: Vec<Frame>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportPreset {
    pub name: String,
    pub destination: Option<PathBuf>,
    pub tiff: bool,
    pub dng: bool,
    /// Zero keeps full resolution; otherwise constrain the longest edge.
    pub long_edge: u32,
    pub jpeg_quality: u8,
    pub prefix: String,
}
impl Default for ExportPreset {
    fn default() -> Self {
        Self {
            name: "Full-resolution JPEG".into(),
            destination: None,
            tiff: false,
            dng: false,
            long_edge: 0,
            jpeg_quality: 95,
            prefix: String::new(),
        }
    }
}

#[cfg(test)]
mod jpeg_preset_tests {
    use super::*;
    #[test]
    fn legacy_presets_keep_quality_95_and_custom_quality_roundtrips() {
        let mut preset: ExportPreset = serde_json::from_str("{}").unwrap();
        assert_eq!(preset.jpeg_quality, 95);
        preset.jpeg_quality = 87;
        preset.long_edge = 2048;
        let saved = serde_json::to_string(&preset).unwrap();
        let restored: ExportPreset = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.jpeg_quality, 87);
        assert_eq!(restored.long_edge, 2048);
    }
}

#[derive(Clone, Copy)]
#[allow(clippy::struct_excessive_bools)] // Independently selectable copy groups.
pub struct PasteOptions {
    pub lighting: bool,
    pub color: bool,
    pub composition: bool,
    pub dust: bool,
}
impl Default for PasteOptions {
    fn default() -> Self {
        Self {
            lighting: true,
            color: true,
            composition: false,
            dust: false,
        }
    }
}
pub fn paste_edits(target: &mut Frame, source: &Frame, options: PasteOptions) {
    if options.lighting {
        target.adjustments.brightness = source.adjustments.brightness;
        target.adjustments.contrast = source.adjustments.contrast;
        target.adjustments.highlights = source.adjustments.highlights;
        target.adjustments.shadows = source.adjustments.shadows;
    }
    if options.color {
        target.adjustments.warmth = source.adjustments.warmth;
        target.adjustments.tint = source.adjustments.tint;
        target.adjustments.saturation = source.adjustments.saturation;
        target.adjustments.colors = source.adjustments.colors;
    }
    if options.composition {
        target.crop = source.crop;
        target.rotation = source.rotation;
    }
    if options.dust {
        target.heals.clone_from(&source.heals);
    }
}

#[derive(Serialize, Deserialize)]
pub struct EditClipboard {
    format: String,
    adjustments: Adjustments,
    crop: Rect,
    rotation: u8,
    heals: Vec<Heal>,
}
impl EditClipboard {
    pub fn encode(frame: &Frame) -> String {
        serde_json::to_string(&Self {
            format: "greign-frame-edits-v1".into(),
            adjustments: frame.adjustments,
            crop: frame.crop,
            rotation: frame.rotation,
            heals: frame.heals.clone(),
        })
        .unwrap()
    }
    pub fn decode(text: &str) -> Option<Frame> {
        if text.len() > 1_048_576 {
            return None;
        }
        let value: Self = serde_json::from_str(text).ok()?;
        if value.format != "greign-frame-edits-v1"
            || !value.crop.valid()
            || value.rotation > 3
            || value.heals.len() > 10_000
        {
            return None;
        }
        let a = value.adjustments;
        if ![
            a.brightness,
            a.contrast,
            a.warmth,
            a.tint,
            a.saturation,
            a.shadows,
            a.highlights,
        ]
        .into_iter()
        .chain(
            a.colors
                .into_iter()
                .flat_map(|b| [b.hue, b.saturation, b.lightness]),
        )
        .all(|v| v.is_finite() && v.abs() <= 1000.)
        {
            return None;
        }
        if !value.heals.iter().all(|h| {
            h.radius.is_finite()
                && (0.0001..=0.1).contains(&h.radius)
                && h.target
                    .into_iter()
                    .chain(h.source)
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
        }) {
            return None;
        }
        let mut frame = Frame::new(PathBuf::new());
        frame.adjustments = value.adjustments;
        frame.crop = value.crop;
        frame.rotation = value.rotation;
        frame.heals = value.heals;
        Some(frame)
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Library {
    pub version: u32,
    pub rolls: Vec<Roll>,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            version: 1,
            rolls: Vec::new(),
        }
    }
}

pub fn id() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "{:x}-{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}
impl Frame {
    pub fn new(path: PathBuf) -> Self {
        Self {
            id: id(),
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            path,
            rejected: false,
            adjustments: Adjustments::default(),
            crop: Rect::default(),
            rotation: 0,
            heals: Vec::new(),
            error: None,
            review: false,
            scan_warning: None,
        }
    }
}
impl Roll {
    pub fn new(name: &str) -> Self {
        Self {
            id: id(),
            name: name.into(),
            stock: String::new(),
            material: Material::default(),
            film_camera: String::new(),
            date: String::new(),
            notes: String::new(),
            demo: false,
            frames: Vec::new(),
            adjustments: Adjustments::default(),
            analysis: None,
            previous_analysis: None,
            base: None,
            analysis_area: Rect {
                x: 0.06,
                y: 0.06,
                w: 0.88,
                h: 0.88,
            },
            conversion_locked: false,
            versions: Vec::new(),
            export: ExportPreset::default(),
            export_presets: Vec::new(),
        }
    }

    pub fn snapshot(&self) -> RollState {
        RollState {
            adjustments: self.adjustments,
            analysis: self.analysis.clone(),
            base: self.base,
            analysis_area: self.analysis_area,
            conversion_locked: self.conversion_locked,
            frames: self.frames.clone(),
        }
    }
    pub fn save_version(&mut self, name: &str) {
        self.versions.push(SavedVersion {
            name: name.trim().to_owned(),
            created: id(),
            state: self.snapshot(),
        });
    }
    pub fn restore_state(&mut self, state: &RollState) {
        self.adjustments = state.adjustments;
        self.analysis.clone_from(&state.analysis);
        self.base = state.base;
        self.analysis_area = state.analysis_area;
        self.conversion_locked = state.conversion_locked;
        for frame in &mut self.frames {
            if let Some(saved) = state.frames.iter().find(|s| s.id == frame.id) {
                paste_edits(
                    frame,
                    saved,
                    PasteOptions {
                        lighting: true,
                        color: true,
                        composition: true,
                        dust: true,
                    },
                );
                frame.rejected = saved.rejected;
                frame.review = saved.review;
            }
        }
    }
}

pub fn load(path: &Path) -> Result<Library> {
    if !path.exists() {
        return Ok(Library::default());
    }
    let library: Library = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(library.version == 1, "Unsupported roll-library version");
    Ok(library)
}
pub fn save(path: &Path, library: &Library) -> Result<()> {
    let data = serde_json::to_vec_pretty(library)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Same-filesystem rename makes a completed save atomic. Preserve last version.
    if path.exists() {
        fs::copy(path, path.with_extension("backup.json"))?;
    }
    let pending = path.with_extension("pending.json");
    fs::write(&pending, data)?;
    fs::File::open(&pending)?.sync_all()?;
    fs::rename(pending, path)?;
    Ok(())
}
pub fn seed_demo(root: &Path) -> Roll {
    let mut roll = Roll::new("Summer light");
    roll.demo = true;
    roll.stock = "Simulated color negative".into();
    roll.notes="Illustrative demo built from existing positive R7 photos. These are not real film scans and do not validate conversion color.".into();
    for name in [
        "152A0123", "152A0295", "152A0325", "152A9835", "152A9855", "152A9949", "152A9968",
        "152A9971",
    ] {
        let path = root.join(format!("exports/r7-render-fix/{name}-after.jpg"));
        if path.exists() {
            roll.frames.push(Frame::new(path));
        }
    }
    roll.base = Some([0.82, 0.52, 0.30]);
    roll
}
pub fn supported(path: &Path) -> bool {
    path.extension().is_some_and(|v| {
        matches!(
            v.to_string_lossy().to_lowercase().as_str(),
            "cr3"
                | "cr2"
                | "nef"
                | "arw"
                | "dng"
                | "orf"
                | "rw2"
                | "raf"
                | "jpg"
                | "jpeg"
                | "png"
                | "tif"
                | "tiff"
        )
    })
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    #[test]
    fn old_libraries_default_new_controls_to_neutral() {
        let mut value = serde_json::to_value(Roll::new("Old roll")).unwrap();
        let object = value.as_object_mut().unwrap();
        for name in ["versions", "conversion_locked", "export", "export_presets"] {
            object.remove(name);
        }
        object.insert(
            "adjustments".into(),
            serde_json::json!({"brightness":0.5,"warmth":7.0}),
        );
        let restored: Roll = serde_json::from_value(value).unwrap();
        assert_eq!(restored.adjustments.highlights, 0.0);
        assert_eq!(restored.adjustments.colors, [film_band(); 8]);
        assert!(!restored.conversion_locked);
        assert!(restored.versions.is_empty());
    }
    fn film_band() -> greign_film::ColorBand {
        greign_film::ColorBand::default()
    }

    #[test]
    fn selective_paste_and_versions_preserve_identity_and_new_frames() {
        let mut roll = Roll::new("test");
        let mut source = Frame::new("one.png".into());
        source.adjustments.highlights = -35.0;
        source.adjustments.colors[2].hue = 12.0;
        source.rotation = 1;
        source.review = true;
        let mut target = Frame::new("two.png".into());
        let identity = target.id.clone();
        paste_edits(&mut target, &source, PasteOptions::default());
        assert_eq!(target.adjustments, source.adjustments);
        assert_eq!(target.rotation, 0);
        assert!(!target.review);
        assert_eq!(target.id, identity);
        roll.frames = vec![source, target];
        roll.save_version("Before");
        roll.frames[0].adjustments.shadows = 45.0;
        roll.frames.push(Frame::new("later.png".into()));
        let state = roll.versions[0].state.clone();
        roll.restore_state(&state);
        assert_eq!(roll.frames.len(), 3);
        assert_eq!(roll.frames[0].adjustments.shadows, 0.0);
        let restored: Roll = serde_json::from_slice(&serde_json::to_vec(&roll).unwrap()).unwrap();
        assert_eq!(restored.versions[0].name, "Before");
        assert_eq!(restored.frames[0].adjustments.colors[2].hue, 12.0);
    }
    #[test]
    fn roll_and_frame_edits_are_independent() {
        let mut r = Roll::new("test");
        r.frames.push(Frame::new("one.CR3".into()));
        r.frames[0].adjustments.warmth = 7.0;
        r.adjustments.warmth = 3.0;
        assert_eq!(r.adjustments.combined(r.frames[0].adjustments).warmth, 10.0);
        assert_eq!(r.frames[0].adjustments.warmth, 7.0);
    }
    #[test]
    fn serialization_keeps_manual_dust_and_analysis_separate_from_crop() {
        let mut r = Roll::new("test");
        r.frames.push(Frame::new("one.CR3".into()));
        r.frames[0].heals.push(Heal {
            target: [0.5, 0.5],
            source: [0.6, 0.5],
            radius: 0.01,
        });
        let out: Roll = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(out.frames[0].heals.len(), 1);
        assert_ne!(out.analysis_area, out.frames[0].crop);
    }

    #[test]
    fn save_reload_and_backup_keep_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rolls.json");
        let mut library = Library {
            version: 1,
            rolls: vec![Roll::new("Original name")],
        };
        save(&path, &library).unwrap();
        library.rolls[0].name = "Edited name".into();
        save(&path, &library).unwrap();
        assert_eq!(load(&path).unwrap().rolls[0].name, "Edited name");
        assert_eq!(
            load(&path.with_extension("backup.json")).unwrap().rolls[0].name,
            "Original name"
        );
        assert!(!path.with_extension("pending.json").exists());
    }
}
