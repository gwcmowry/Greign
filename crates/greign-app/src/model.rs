use anyhow::{Context, Result, ensure};
use greign_film::{Adjustments, Analysis, Heal, Material, Rect};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
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
    /// Explicit film-base measurement for this frame in individual mode.
    #[serde(default)]
    pub base: Option<[f32; 3]>,
    pub crop: Rect,
    pub rotation: u8,
    #[serde(default)]
    pub mirrored: bool,
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
    #[serde(default)]
    pub source_folder: Option<PathBuf>,
    pub stock: String,
    pub material: Material,
    pub film_camera: String,
    pub date: String,
    pub notes: String,
    pub demo: bool,
    pub frames: Vec<Frame>,
    pub adjustments: Adjustments,
    pub analysis: Option<Conversion>,
    pub previous_analysis: Option<Conversion>,
    #[serde(default)]
    pub optimize_individually: bool,
    #[serde(default)]
    pub calibration: greign_film::CalibrationOptions,
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
    pub analysis: Option<Conversion>,
    #[serde(default)]
    pub optimize_individually: bool,
    #[serde(default)]
    pub calibration: greign_film::CalibrationOptions,
    pub base: Option<[f32; 3]>,
    pub analysis_area: Rect,
    pub conversion_locked: bool,
    pub frames: Vec<Frame>,
}

/// Inputs captured when a conversion is built, for detecting stale proposals.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnalysisInputs {
    pub base: Option<[f32; 3]>,
    pub area: Rect,
    pub frames: BTreeSet<String>,
    #[serde(default)]
    pub frame_bases: BTreeMap<String, [f32; 3]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameAnalysisInputs {
    pub base: Option<[f32; 3]>,
    pub area: Rect,
}

#[derive(Clone)]
pub struct ProposedConversion {
    pub roll_id: String,
    pub conversion: Conversion,
    /// None replaces the whole roll; Some replaces only this frame.
    pub frame_id: Option<String>,
}
impl ProposedConversion {
    pub fn affects(&self, roll: &Roll, frame: &Frame) -> bool {
        self.roll_id == roll.id && self.frame_id.as_ref().is_none_or(|id| *id == frame.id)
    }
}

/// Accepted or proposed calibration; old shared-analysis JSON still loads.
#[derive(Clone, Serialize, Deserialize)]
pub struct Conversion {
    #[serde(flatten)]
    pub shared: Analysis,
    #[serde(default)]
    pub individual: bool,
    #[serde(default)]
    pub per_frame: BTreeMap<String, Analysis>,
    /// Captured inputs distinguish a pending result from subsequently edited settings.
    #[serde(default)]
    pub inputs: Option<AnalysisInputs>,
    #[serde(default)]
    pub frame_inputs: BTreeMap<String, FrameAnalysisInputs>,
}
impl From<Analysis> for Conversion {
    fn from(shared: Analysis) -> Self {
        Self {
            shared,
            individual: false,
            per_frame: BTreeMap::new(),
            inputs: None,
            frame_inputs: BTreeMap::new(),
        }
    }
}
impl Conversion {
    pub fn analyze(
        scans: &[(&str, &greign_film::Scan)],
        individual: bool,
        base: Option<[f32; 3]>,
        area: Rect,
        options: greign_film::CalibrationOptions,
        frame_bases: &BTreeMap<String, [f32; 3]>,
    ) -> Result<Self> {
        ensure!(!scans.is_empty(), "Import some frames first");
        let inputs = Some(AnalysisInputs {
            base: if individual { None } else { base },
            area,
            frames: scans.iter().map(|(id, _)| (*id).to_owned()).collect(),
            frame_bases: if individual {
                frame_bases.clone()
            } else {
                BTreeMap::new()
            },
        });
        if !individual {
            let refs: Vec<_> = scans.iter().map(|(_, scan)| *scan).collect();
            let mut conversion = Self::from(greign_film::analyze_with_options(
                &refs, base, area, options,
            )?);
            conversion.inputs = inputs;
            return Ok(conversion);
        }
        let mut per_frame = BTreeMap::new();
        let mut frame_inputs = BTreeMap::new();
        for (id, scan) in scans {
            let base = frame_bases.get(*id).copied();
            let analysis = greign_film::analyze_with_options(&[scan], base, area, options)
                .with_context(|| format!("Could not optimize frame {id}"))?;
            per_frame.insert((*id).to_owned(), analysis);
            frame_inputs.insert((*id).to_owned(), FrameAnalysisInputs { base, area });
        }
        let mut shared = per_frame.values().next().unwrap().clone();
        shared.method = "Frames optimized individually".into();
        shared.frames = per_frame.len();
        shared.warnings = per_frame
            .values()
            .flat_map(|a| a.warnings.iter().cloned())
            .collect();
        shared.warnings.sort();
        shared.warnings.dedup();
        Ok(Self {
            shared,
            individual: true,
            per_frame,
            inputs,
            frame_inputs,
        })
    }
    pub fn frame_input(&self, id: &str) -> Option<FrameAnalysisInputs> {
        if !self.frame_inputs.is_empty() {
            return self.frame_inputs.get(id).cloned();
        }
        self.inputs.as_ref().map(|i| FrameAnalysisInputs {
            base: if self.individual {
                i.frame_bases.get(id).copied()
            } else {
                i.base
            },
            area: i.area,
        })
    }
    /// Rebuild one frame while retaining every other accepted calibration.
    pub fn analyze_frame(roll: &Roll, id: &str, scan: &greign_film::Scan) -> Result<Self> {
        ensure!(
            roll.optimize_individually,
            "Select individual optimization for a frame override"
        );
        let frame = roll
            .frames
            .iter()
            .find(|f| f.id == id)
            .ok_or_else(|| anyhow::anyhow!("Frame no longer exists"))?;
        let a = greign_film::analyze_with_options(
            &[scan],
            frame.base,
            roll.analysis_area,
            roll.calibration,
        )?;
        let mut next = Self::from(a.clone());
        next.individual = true;
        if let Some(previous) = &roll.analysis {
            for f in &roll.frames {
                if let Some(old) = previous.for_frame(&f.id) {
                    next.per_frame.insert(f.id.clone(), old.clone());
                }
                if let Some(input) = previous.frame_input(&f.id) {
                    next.frame_inputs.insert(f.id.clone(), input);
                }
            }
        }
        next.per_frame.insert(id.to_owned(), a);
        next.frame_inputs.insert(
            id.to_owned(),
            FrameAnalysisInputs {
                base: frame.base,
                area: roll.analysis_area,
            },
        );
        next.inputs = Some(roll.analysis_inputs());
        next.shared.method = "Frames optimized individually".into();
        next.shared.frames = next.per_frame.len();
        next.shared.warnings = next
            .per_frame
            .values()
            .flat_map(|a| a.warnings.iter().cloned())
            .collect();
        next.shared.warnings.sort();
        next.shared.warnings.dedup();
        Ok(next)
    }
    pub fn matches_settings(&self, roll: &Roll) -> bool {
        if self.individual && !self.frame_inputs.is_empty() {
            return roll.optimize_individually
                && roll.frames.iter().filter(|f| !f.rejected).all(|f| {
                    self.per_frame
                        .get(&f.id)
                        .is_some_and(|a| a.options == roll.calibration)
                        && self.frame_inputs.get(&f.id).is_none_or(|i| {
                            *i == FrameAnalysisInputs {
                                base: f.base,
                                area: roll.analysis_area,
                            }
                        })
                });
        }
        self.request_matches(roll)
    }
    fn request_matches(&self, roll: &Roll) -> bool {
        self.individual == roll.optimize_individually
            && self.shared.options == roll.calibration
            && self.inputs.as_ref().is_none_or(|inputs| {
                *inputs
                    == AnalysisInputs {
                        base: if roll.optimize_individually {
                            None
                        } else {
                            roll.base
                        },
                        area: roll.analysis_area,
                        frames: roll
                            .frames
                            .iter()
                            .filter(|f| !f.rejected)
                            .map(|f| f.id.clone())
                            .collect(),
                        frame_bases: if roll.optimize_individually {
                            roll.frame_bases()
                        } else {
                            BTreeMap::new()
                        },
                    }
            })
    }
    pub fn is_stale(&self, roll: &Roll) -> bool {
        self.inputs.is_some() && !self.request_matches(roll)
    }
    pub fn for_frame(&self, id: &str) -> Option<&Analysis> {
        if self.individual {
            self.per_frame.get(id)
        } else {
            Some(&self.shared)
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportPreset {
    pub name: String,
    pub destination: Option<PathBuf>,
    pub folder_suffix: String,
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
            folder_suffix: " - converted".into(),
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
        target.mirrored = source.mirrored;
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
    #[serde(default)]
    mirrored: bool,
    heals: Vec<Heal>,
}
impl EditClipboard {
    pub fn encode(frame: &Frame) -> String {
        serde_json::to_string(&Self {
            format: "greign-frame-edits-v1".into(),
            adjustments: frame.adjustments,
            crop: frame.crop,
            rotation: frame.rotation,
            mirrored: frame.mirrored,
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
        frame.mirrored = value.mirrored;
        frame.heals = value.heals;
        Some(frame)
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Library {
    pub version: u32,
    pub rolls: Vec<Roll>,
    #[serde(default)]
    pub export_defaults: ExportPreset,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            version: 1,
            rolls: Vec::new(),
            export_defaults: ExportPreset::default(),
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
            base: None,
            crop: Rect::default(),
            rotation: 0,
            mirrored: false,
            heals: Vec::new(),
            error: None,
            review: false,
            scan_warning: None,
        }
    }
}
impl Roll {
    pub fn frame_bases(&self) -> BTreeMap<String, [f32; 3]> {
        self.frames
            .iter()
            .filter_map(|f| f.base.map(|base| (f.id.clone(), base)))
            .collect()
    }
    pub fn analysis_inputs(&self) -> AnalysisInputs {
        AnalysisInputs {
            base: if self.optimize_individually {
                None
            } else {
                self.base
            },
            area: self.analysis_area,
            frames: self
                .frames
                .iter()
                .filter(|f| !f.rejected)
                .map(|f| f.id.clone())
                .collect(),
            frame_bases: if self.optimize_individually {
                self.frame_bases()
            } else {
                BTreeMap::new()
            },
        }
    }
    pub fn analysis_for(&self, frame: &Frame) -> Option<&Analysis> {
        self.analysis.as_ref()?.for_frame(&frame.id)
    }
    pub fn new(name: &str) -> Self {
        Self {
            id: id(),
            name: name.into(),
            source_folder: None,
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
            optimize_individually: true,
            calibration: greign_film::CalibrationOptions::default(),
            base: None,
            analysis_area: Rect {
                x: 0.06,
                y: 0.06,
                w: 0.88,
                h: 0.88,
                angle: 0.0,
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
            optimize_individually: self.optimize_individually,
            calibration: self.calibration,
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
        self.optimize_individually = state.optimize_individually;
        self.calibration = state.calibration;
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
                frame.base = saved.base;
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
    // macOS AppleDouble sidecars carry the original extension but no image data.
    if path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with("._"))
    {
        return false;
    }
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
    fn individual_calibration_handles_capture_exposure_and_roundtrips_legacy_state() {
        let scan = greign_film::Scan {
            image: greign_pipeline::LinearRgbImage {
                width: 80,
                height: 60,
                pixels: (0..4800)
                    .map(|i| [0.8, 0.5, 0.3].map(|v| v * 10_f32.powf(-((i % 80) as f32) / 60.)))
                    .collect(),
            },
            camera_matrix: None,
            white_balance: [1.; 3],
            capture: "fixture".into(),
        };
        let mut darker = scan.clone();
        for pixel in &mut darker.image.pixels {
            for v in pixel {
                *v *= 0.35;
            }
        }
        let scans = [("a", &scan), ("b", &darker)];
        let conversion = Conversion::analyze(
            &scans,
            true,
            Some([0.8, 0.5, 0.3]),
            Rect::default(),
            greign_film::CalibrationOptions::default(),
            &BTreeMap::new(),
        )
        .unwrap();
        assert!(conversion.individual);
        assert!(conversion.for_frame("newly-imported").is_none());
        assert!(
            conversion.for_frame("a").unwrap().base[0]
                > conversion.for_frame("b").unwrap().base[0] * 2.
        );
        let a = greign_film::prepare(
            &scan,
            Material::ColorNegative,
            conversion.for_frame("a"),
            false,
            &|| false,
        )
        .unwrap();
        let b = greign_film::prepare(
            &darker,
            Material::ColorNegative,
            conversion.for_frame("b"),
            false,
            &|| false,
        )
        .unwrap();
        for (a, b) in a
            .image
            .pixels
            .iter()
            .flatten()
            .zip(b.image.pixels.iter().flatten())
        {
            assert!((a - b).abs() < 0.00001);
        }
        let restored: Conversion =
            serde_json::from_str(&serde_json::to_string(&conversion).unwrap()).unwrap();
        assert!(
            restored.individual
                && restored.for_frame("a").is_some()
                && restored.for_frame("b").is_some()
        );
        let old_json = serde_json::to_string(conversion.for_frame("a").unwrap()).unwrap();
        let legacy: Conversion = serde_json::from_str(&old_json).unwrap();
        assert!(!legacy.individual && legacy.for_frame("any-frame").is_some());
        let mut roll = Roll::new("versions");
        let mut frame = Frame::new("a.png".into());
        frame.mirrored = true;
        roll.frames.push(frame);
        roll.analysis = Some(restored);
        roll.optimize_individually = true;
        roll.calibration = greign_film::CalibrationOptions {
            illumination: true,
            color_balance: 0.75,
        };
        let state = roll.snapshot();
        roll.frames[0].mirrored = false;
        roll.optimize_individually = false;
        roll.calibration = greign_film::CalibrationOptions::default();
        roll.analysis = None;
        roll.restore_state(&state);
        assert_eq!(roll.calibration, state.calibration);
        assert!(
            roll.optimize_individually
                && roll.frames[0].mirrored
                && roll.analysis.as_ref().unwrap().individual
        );
        let clipboard = EditClipboard::encode(&roll.frames[0]);
        assert!(EditClipboard::decode(&clipboard).unwrap().mirrored);
    }
    #[test]
    fn import_ignores_appledouble_sidecars() {
        assert!(supported(Path::new("IMG_0005.CR3")));
        assert!(!supported(Path::new("._IMG_0005.CR3")));
        assert!(!supported(Path::new("._scan.jpg")));
    }
    #[test]
    fn new_rolls_default_to_individual_but_saved_modes_are_preserved() {
        let roll = Roll::new("New roll");
        assert!(roll.optimize_individually);
        let mut value = serde_json::to_value(roll).unwrap();
        value["optimize_individually"] = false.into();
        let restored: Roll = serde_json::from_value(value.clone()).unwrap();
        assert!(!restored.optimize_individually);
        value
            .as_object_mut()
            .unwrap()
            .remove("optimize_individually");
        let legacy: Roll = serde_json::from_value(value).unwrap();
        assert!(!legacy.optimize_individually);
    }

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
        let mut frame = serde_json::to_value(Frame::new("old.png".into())).unwrap();
        frame["crop"].as_object_mut().unwrap().remove("angle");
        let restored: Frame = serde_json::from_value(frame).unwrap();
        assert_eq!(restored.crop.angle, 0.0);
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
        source.crop = Rect {
            x: 0.1,
            y: 0.1,
            w: 0.8,
            h: 0.8,
            angle: 2.3,
        };
        source.review = true;
        let mut target = Frame::new("two.png".into());
        let identity = target.id.clone();
        paste_edits(&mut target, &source, PasteOptions::default());
        assert_eq!(target.adjustments, source.adjustments);
        assert_eq!(target.rotation, 0);
        assert_eq!(target.crop.angle, 0.0);
        let copied = EditClipboard::decode(&EditClipboard::encode(&source)).unwrap();
        assert_eq!(copied.crop, source.crop);
        paste_edits(
            &mut target,
            &copied,
            PasteOptions {
                composition: true,
                ..PasteOptions::default()
            },
        );
        assert_eq!(target.crop, source.crop);
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
            ..Library::default()
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
