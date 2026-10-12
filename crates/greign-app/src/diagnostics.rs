//! Opt-in preview diagnostics and neutral sampling, kept off the UI thread.
use crate::{App, Tool, jobs::Event};
use eframe::egui;
use greign_film::{self as film, Adjustments, Analysis, Material, Rect, Scan};

#[allow(clippy::many_single_char_names)]
pub fn neutral(
    scan: &Scan,
    material: Material,
    analysis: Option<&Analysis>,
    edits: Adjustments,
    point: [f32; 2],
) -> anyhow::Result<[f32; 2]> {
    anyhow::ensure!(
        point
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
        "Click inside the image"
    );
    anyhow::ensure!(
        material != Material::BlackAndWhite,
        "Neutral color sampling is not useful on B&W"
    );
    let prepared = film::prepare(scan, material, analysis, false, &|| false)?;
    let visible = greign_pipeline::rec2020_to_srgb(film::finish(
        &prepared,
        edits,
        Rect::default(),
        0,
        &[],
        &|| false,
    )?);
    let x = (point[0] * scan.image.width as f32) as u32;
    let y = (point[1] * scan.image.height as f32) as u32;
    let mut samples = Vec::new();
    for row in y.saturating_sub(2)..=(y + 2).min(scan.image.height - 1) {
        for col in x.saturating_sub(2)..=(x + 2).min(scan.image.width - 1) {
            let i = (row * scan.image.width + col) as usize;
            let p = prepared.image.pixels[i];
            if p.iter().all(|v| v.is_finite() && *v > 0.00001)
                && scan.image.pixels[i].iter().all(|v| *v < 0.995)
                && visible.pixels[i].iter().all(|v| *v > 0.015 && *v < 0.985)
            {
                samples.push(p);
            }
        }
    }
    anyhow::ensure!(
        samples.len() >= 3,
        "Choose a clean gray/white patch, away from clipping and deep shadows"
    );
    let median: [f32; 3] = std::array::from_fn(|c| {
        let mut v: Vec<_> = samples.iter().map(|p| p[c]).collect();
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    });
    let [r, g, b] = median;
    Ok([(b / r).log2() / 0.016, (g / (r * b).sqrt()).log2() / 0.008])
}

pub type Histogram = [[u32; 256]; 3];

pub fn histogram(bytes: &[u8]) -> Histogram {
    let mut bins = [[0; 256]; 3];
    for pixel in bytes.chunks_exact(3) {
        for c in 0..3 {
            bins[c][usize::from(pixel[c])] += 1;
        }
    }
    bins
}

impl App {
    pub(crate) fn neutral_key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.revision, self.roll, self.frame, self.scope_roll
        )
    }
    pub(crate) fn pick_neutral(&mut self, point: [f32; 2], ctx: &egui::Context) {
        let roll = &self.library.rolls[self.roll];
        let frame = &roll.frames[self.frame];
        let Some(scan) = self.cache.get(&frame.id).cloned() else {
            return;
        };
        let short = scan.image.width.min(scan.image.height) as f32;
        if frame.heals.iter().any(|h| {
            let dx = (point[0] - h.target[0]) * scan.image.width as f32 / short;
            let dy = (point[1] - h.target[1]) * scan.image.height as f32 / short;
            dx.hypot(dy) <= h.radius + 3. / short
        }) {
            self.status = "Choose a neutral patch outside a dust repair".into();
            return;
        }
        let key = self.neutral_key();
        let material = roll.material;
        let analysis = roll.analysis.clone();
        let edits = roll.adjustments.combined(frame.adjustments);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.tool = Tool::View;
        self.status = "Measuring neutral patch…".into();
        rayon::spawn(move || {
            let result = neutral(&scan, material, analysis.as_ref(), edits, point)
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Neutral { key, result });
            ctx.request_repaint();
        });
    }
    pub(crate) fn request_histogram(&mut self, ctx: &egui::Context) {
        let roll = &self.library.rolls[self.roll];
        let Some(active) = roll.frames.get(self.frame) else {
            self.histogram = None;
            self.histogram_key.clear();
            return;
        };
        let version = if self.comparing {
            roll.versions.get(self.version_index)
        } else {
            None
        };
        let frame = version
            .and_then(|v| v.state.frames.iter().find(|f| f.id == active.id))
            .unwrap_or(active);
        let Some(scan) = self.cache.get(&frame.id).cloned() else {
            self.histogram = None;
            self.histogram_key.clear();
            return;
        };
        let edits = version
            .map_or(roll.adjustments, |v| v.state.adjustments)
            .combined(frame.adjustments);
        let material = roll.material;
        let analysis = if let Some(version) = version {
            version.state.analysis.clone()
        } else if self.preview_analysis {
            self.pending_analysis
                .as_ref()
                .filter(|(id, _)| *id == roll.id)
                .map(|(_, a)| a.clone())
                .or_else(|| roll.analysis.clone())
        } else {
            roll.analysis.clone()
        };
        let negative = (!self.comparing && self.show_negative)
            || (analysis.is_none() && material != Material::Positive);
        let key = serde_json::to_string(&(
            format!("{}:{:p}", frame.id, std::sync::Arc::as_ptr(&scan)),
            material,
            &analysis,
            edits,
            frame.crop,
            frame.rotation,
            &frame.heals,
            negative,
        ))
        .unwrap();
        if key != self.histogram_key {
            self.histogram_key.clone_from(&key);
            self.histogram = None;
            self.histogram_error = None;
        }
        if self.histogram.is_some()
            || self.histogram_error.is_some()
            || self.histogram_pending
            || self.dragging
            || ctx.input(|i| i.pointer.any_down())
        {
            return;
        }
        self.histogram_pending = true;
        let frame = frame.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let profile = self.profile();
        rayon::spawn(move || {
            let start = std::time::Instant::now();
            let result = (|| -> anyhow::Result<Box<Histogram>> {
                let linear = film::render_linear(
                    &scan,
                    material,
                    analysis.as_ref(),
                    edits,
                    if negative {
                        Rect::default()
                    } else {
                        frame.crop
                    },
                    if negative { 0 } else { frame.rotation },
                    &frame.heals,
                    negative,
                )?;
                let rgb = greign_pipeline::rec2020_to_srgb(linear);
                Ok(Box::new(histogram(&rgb.to_rgb8())))
            })()
            .map_err(|e| e.to_string());
            crate::jobs::record(
                &profile,
                &serde_json::json!({"stage":"histogram-preview","elapsed_ms":start.elapsed().as_secs_f64()*1000.}),
            );
            let _ = tx.send(Event::Histogram { key, result });
            ctx.request_repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Opt-in CPU timing on the local R7 fixture"]
    fn histogram_cpu_profile() {
        use std::io::Write;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let scan = film::load_scan(
            &root.join("exports/r7-render-fix/152A0123-after.jpg"),
            Some(960),
            false,
        )
        .unwrap();
        let mut log =
            std::fs::File::create(root.join("artifacts/histogram-cpu-profile.csv")).unwrap();
        for i in 0_u8..10 {
            let start = std::time::Instant::now();
            let linear = film::render_linear(
                &scan,
                Material::Positive,
                None,
                Adjustments {
                    brightness: f32::from(i) * 0.1,
                    ..Default::default()
                },
                Rect::default(),
                0,
                &[],
                false,
            )
            .unwrap();
            let rgb = greign_pipeline::rec2020_to_srgb(linear);
            let bins = histogram(&rgb.to_rgb8());
            assert_eq!(bins[0].iter().sum::<u32>(), rgb.width * rgb.height);
            writeln!(log, "{i},{:.3}", start.elapsed().as_secs_f64() * 1000.).unwrap();
        }
    }
    #[test]
    fn histogram_counts_each_channel_and_endpoints() {
        let bins = histogram(&[255, 3, 4, 0, 0, 0, 0, 50, 90, 120, 130, 140]);
        assert_eq!(bins[0][0], 2);
        assert_eq!(bins[0][255], 1);
        assert_eq!(bins[1][3], 1);
        for channel in bins {
            assert_eq!(channel.iter().sum::<u32>(), 4);
        }
    }
    #[test]
    fn neutral_solver_recovers_known_warmth_and_tint() {
        let scan = Scan {
            image: greign_pipeline::LinearRgbImage {
                width: 9,
                height: 9,
                pixels: vec![[0.3, 0.24, 0.2]; 81],
            },
            camera_matrix: None,
            white_balance: [1.; 3],
            capture: "test".into(),
        };
        let [warmth, tint] = neutral(
            &scan,
            Material::Positive,
            None,
            Adjustments::default(),
            [0.5, 0.5],
        )
        .unwrap();
        let prepared = film::prepare(&scan, Material::Positive, None, false, &|| false).unwrap();
        let fixed = film::finish(
            &prepared,
            Adjustments {
                warmth,
                tint,
                ..Default::default()
            },
            Rect::default(),
            0,
            &[],
            &|| false,
        )
        .unwrap();
        let [r, g, b] = fixed.pixels[0];
        assert!((r - g).abs() < 0.000_001 && (r - b).abs() < 0.000_001);
        let clipped = Scan {
            image: greign_pipeline::LinearRgbImage {
                width: 9,
                height: 9,
                pixels: vec![[1.; 3]; 81],
            },
            ..scan
        };
        assert!(
            neutral(
                &clipped,
                Material::Positive,
                None,
                Adjustments::default(),
                [0.5, 0.5]
            )
            .is_err()
        );
    }
}
