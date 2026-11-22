//! Robust picture-boundary detection and shared crop geometry.
use crate::{Rect, Scan};

#[allow(clippy::many_single_char_names)]
fn bilinear(image: &greign_pipeline::LinearRgbImage, x: f32, y: f32) -> [f32; 3] {
    let x = x.clamp(0., (image.width - 1) as f32);
    let y = y.clamp(0., (image.height - 1) as f32);
    let (ix, iy) = (x as u32, y as u32);
    let (tx, ty) = (x - ix as f32, y - iy as f32);
    let pixel = |x, y| image.pixels[(y * image.width + x) as usize];
    let a = pixel(ix, iy);
    let b = pixel((ix + 1).min(image.width - 1), iy);
    let c = pixel(ix, (iy + 1).min(image.height - 1));
    let d = pixel(
        (ix + 1).min(image.width - 1),
        (iy + 1).min(image.height - 1),
    );
    std::array::from_fn(|i| {
        (a[i] * (1. - tx) + b[i] * tx) * (1. - ty) + (c[i] * (1. - tx) + d[i] * tx) * ty
    })
}

pub(crate) fn apply_crop(
    rgb: &greign_pipeline::LinearRgbImage,
    crop: Rect,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> anyhow::Result<greign_pipeline::LinearRgbImage> {
    use anyhow::{Result, ensure};
    use rayon::prelude::*;
    let x = (crop.x * rgb.width as f32) as u32;
    let y = (crop.y * rgb.height as f32) as u32;
    let w = ((crop.w * rgb.width as f32) as u32)
        .max(1)
        .min(rgb.width - x);
    let h = ((crop.h * rgb.height as f32) as u32)
        .max(1)
        .min(rgb.height - y);
    let mut pixels = Vec::with_capacity((w * h) as usize);
    if crop.angle == 0.0 {
        for row in y..y + h {
            ensure!(!cancelled(), "Preview superseded");
            pixels.extend_from_slice(
                &rgb.pixels[(row * rgb.width + x) as usize..(row * rgb.width + x + w) as usize],
            );
        }
    } else {
        pixels.resize((w * h) as usize, [0.; 3]);
        let [origin, step_x, step_y] = crop.sampling_grid(rgb.width, rgb.height);
        pixels.par_chunks_mut(w as usize).enumerate().try_for_each(
            |(row, pixels)| -> Result<()> {
                ensure!(!cancelled(), "Preview superseded");
                let start_x = origin[0] + step_y[0] * row as f32;
                let start_y = origin[1] + step_y[1] * row as f32;
                for (column, out) in pixels.iter_mut().enumerate() {
                    *out = bilinear(
                        rgb,
                        start_x + step_x[0] * column as f32,
                        start_y + step_x[1] * column as f32,
                    );
                }
                Ok(())
            },
        )?;
    }
    Ok(greign_pipeline::LinearRgbImage {
        width: w,
        height: h,
        pixels,
    })
}

impl Rect {
    /// Source pixel center of the first output pixel, then column and row steps.
    /// Shared by CPU resampling and GPU uniforms to avoid coordinate drift.
    #[must_use]
    pub fn sampling_grid(self, width: u32, height: u32) -> [[f32; 2]; 3] {
        let (width, height) = (width as f32, height as f32);
        let (w, h) = (
            (self.w * width).floor().max(1.),
            (self.h * height).floor().max(1.),
        );
        let origin = self.source_point([0.5 / w, 0.5 / h], width / height);
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let (dx, dy) = (self.w * width / w, self.h * height / h);
        [
            [origin[0] * width - 0.5, origin[1] * height - 0.5],
            [cos * dx, sin * dx],
            [-sin * dy, cos * dy],
        ]
    }

    /// Map normalized crop coordinates into the original image. Aspect is W/H.
    #[must_use]
    pub fn source_point(self, point: [f32; 2], aspect: f32) -> [f32; 2] {
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let dx = (point[0] - 0.5) * self.w;
        let dy = (point[1] - 0.5) * self.h;
        [
            self.x + self.w * 0.5 + cos * dx - sin * dy / aspect,
            self.y + self.h * 0.5 + sin * dx * aspect + cos * dy,
        ]
    }

    /// Inverse of `source_point`, before quarter turns or display mirroring.
    #[must_use]
    pub fn crop_point(self, point: [f32; 2], aspect: f32) -> [f32; 2] {
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let dx = point[0] - self.x - self.w * 0.5;
        let dy = point[1] - self.y - self.h * 0.5;
        [
            0.5 + (cos * dx + sin * dy / aspect) / self.w,
            0.5 + (-sin * dx * aspect + cos * dy) / self.h,
        ]
    }

    /// Check that every tilted corner lies inside the source image.
    #[must_use]
    pub fn valid_for(self, width: u32, height: u32) -> bool {
        self.valid()
            && width > 0
            && height > 0
            && [[0., 0.], [1., 0.], [1., 1.], [0., 1.]]
                .iter()
                .flat_map(|&p| self.source_point(p, width as f32 / height as f32))
                .all(|v| (-0.000_001..=1.000_001).contains(&v))
    }

    /// Preserve integer pixel selection for legacy, axis-aligned crops.
    #[must_use]
    pub fn rasterized(mut self, width: u32, height: u32) -> Self {
        if self.angle == 0.0 {
            let (width, height) = (width as f32, height as f32);
            self.x = (self.x * width).floor() / width;
            self.y = (self.y * height).floor() / height;
            self.w = (self.w * width).floor().max(1.).min(width - self.x * width) / width;
            self.h = (self.h * height)
                .floor()
                .max(1.)
                .min(height - self.y * height)
                / height;
        }
        self
    }
}

#[derive(Clone, Copy, Debug)]
struct Edge {
    slope: f32,
    intercept: f32,
    support: usize,
}

// Deterministic consensus fitting rejects dust, perforations and scene features.
// Coordinates are pixels, so a slope represents the same angle on either axis.
fn fit_edge(points: &[[f32; 2]], span: f32, tolerance: f32) -> Option<Edge> {
    let mut best = Vec::new();
    for (i, a) in points.iter().enumerate() {
        for b in &points[i + 1..] {
            if (a[0] - b[0]).abs() < span * 0.25 {
                continue;
            }
            let slope = (a[1] - b[1]) / (a[0] - b[0]);
            if slope.abs() > 5_f32.to_radians().tan() {
                continue;
            }
            let intercept = a[1] - slope * a[0];
            let inliers: Vec<_> = points
                .iter()
                .filter(|p| (p[1] - slope * p[0] - intercept).abs() <= tolerance)
                .copied()
                .collect();
            if inliers.len() > best.len() {
                best = inliers;
            }
        }
    }
    if best.len() < 25 || best.last()?[0] - best.first()?[0] < span * 0.5 {
        return None;
    }
    let count = best.len() as f32;
    let x = best.iter().map(|p| p[0]).sum::<f32>() / count;
    let y = best.iter().map(|p| p[1]).sum::<f32>() / count;
    let slope = best.iter().map(|p| (p[0] - x) * (p[1] - y)).sum::<f32>()
        / best.iter().map(|p| (p[0] - x).powi(2)).sum::<f32>();
    Some(Edge {
        slope,
        intercept: y - slope * x,
        support: best.len(),
    })
}

fn find_edge(
    pixel: &impl Fn(usize, usize) -> [f32; 3],
    w: usize,
    h: usize,
    side: usize,
    percentile: f32,
    contrast: f32,
) -> Option<(Edge, [f32; 3])> {
    let (vertical, reverse) = (side >= 2, side % 2 == 1);
    let (length, cross) = if vertical { (h, w) } else { (w, h) };
    let mut points = Vec::new();
    let mut bases = Vec::new();
    for i in 0..61 {
        let across = cross * (15 + i * 70 / 60) / 100;
        let sample = |n: usize| {
            let n = if reverse { length - 1 - n } else { n };
            if vertical {
                pixel(across, n)
            } else {
                pixel(n, across)
            }
        };
        let band: Vec<_> = (0..length * 35 / 100)
            .map(sample)
            .filter(|p| p.iter().all(|v| v.is_finite() && *v > 0.000_001))
            .collect();
        if band.len() < 8 {
            continue;
        }
        let base = crate::channels(&band, percentile);
        let at = |n: usize| {
            let p = sample(n);
            (0..3).map(|c| 1. - p[c] / base[c]).fold(0_f32, f32::max)
        };
        let run = (length / 150).max(3);
        let mut base_run = 0;
        for n in 0..length * 35 / 100 {
            if at(n) < contrast * 0.7 {
                base_run += 1;
            } else if base_run >= 2 && (0..run).all(|d| at(n + d) > contrast) {
                bases.push(base);
                points.push([
                    across as f32 + 0.5,
                    if reverse {
                        (length - n) as f32
                    } else {
                        n as f32
                    },
                ]);
                break;
            } else if at(n) > contrast {
                base_run = 0;
            }
        }
    }
    let tolerance = (length as f32 * 0.002).max(1.5);
    let fitted = fit_edge(&points, cross as f32, tolerance)?;
    let bases: Vec<_> = points
        .iter()
        .zip(bases)
        .filter_map(|(p, base)| {
            ((p[1] - fitted.slope * p[0] - fitted.intercept).abs() <= tolerance).then_some(base)
        })
        .collect();
    Some((fitted, crate::channels(&bases, 0.5)))
}

// When the picture reaches the opaque surround, there may be no broad film-base
// plateau. Fit the inner end of that transition, rather than the first lit pixel.
// Boundary detail and interior contrast distinguish a picture from an empty
// holder or a wide, uniform film-base border.
fn tight_edge(
    pixel: &impl Fn(usize, usize) -> [f32; 3],
    w: usize,
    h: usize,
    side: usize,
) -> Option<(Edge, bool)> {
    let (vertical, reverse) = (side >= 2, side % 2 == 1);
    let (length, cross) = if vertical { (h, w) } else { (w, h) };
    let step = (length.min(cross) / 75).max(3);
    let mut points = Vec::new();
    let mut starts = Vec::new();
    let mut plateaus = Vec::new();
    for i in 0..61 {
        let across = cross * (15 + i * 70 / 60) / 100;
        let sample = |n: usize| {
            let n = if reverse { length - 1 - n } else { n };
            if vertical {
                pixel(across, n)
            } else {
                pixel(n, across)
            }
        };
        let band: Vec<_> = (0..length * 35 / 100).map(sample).collect();
        if !band.iter().flatten().all(|v| v.is_finite()) {
            continue;
        }
        let level = |p: [f32; 3]| p.iter().map(|v| v.max(0.)).sum::<f32>();
        let dark = level(crate::channels(&band[..(length / 30).max(2)], 0.5));
        let bright = level(crate::channels(&band, 0.8));
        if bright < dark.max(0.000_01) * 10. {
            continue;
        }
        let threshold = dark + (bright - dark) * 0.08;
        let Some(start) =
            (2..band.len() - 3 * step).find(|&n| (0..3).all(|d| level(band[n + d]) > threshold))
        else {
            continue;
        };
        let plateau = crate::channels(&band[start + step..start + 3 * step], 0.5);
        let threshold = dark + (level(plateau) - dark) * 0.75;
        let Some(at) = (start..start + 3 * step - 2)
            .find(|&n| (0..3).all(|d| level(band[n + d]) >= threshold))
        else {
            continue;
        };
        starts.push([
            across as f32 + 0.5,
            if reverse {
                (length - start) as f32
            } else {
                start as f32
            },
        ]);
        points.push([
            across as f32 + 0.5,
            if reverse {
                (length - at) as f32
            } else {
                at as f32
            },
        ]);
        plateaus.push(plateau);
    }
    let tolerance = (length as f32 * 0.002).max(1.5);
    let mut edge = fit_edge(&starts, cross as f32, tolerance)?;
    let inliers: Vec<_> = starts
        .iter()
        .zip(&points)
        .filter(|(p, _)| (p[1] - edge.slope * p[0] - edge.intercept).abs() <= tolerance)
        .map(|(_, p)| p[1] - edge.slope * p[0])
        .collect();
    let values: Vec<_> = starts
        .iter()
        .zip(plateaus)
        .filter_map(|(p, v)| {
            ((p[1] - edge.slope * p[0] - edge.intercept).abs() <= tolerance).then_some(v)
        })
        .collect();
    edge.intercept = crate::quantile(inliers, if reverse { 0.25 } else { 0.75 });
    let low = crate::channels(&values, 0.15);
    let mid = crate::channels(&values, 0.5);
    let high = crate::channels(&values, 0.85);
    // Smooth falloff across clear film is not scene texture. Require variation
    // between neighboring scanlines as well as a broad brightness range.
    let detail = crate::quantile(
        values
            .windows(2)
            .map(|p| {
                (0..3)
                    .map(|c| {
                        (p[0][c].max(0.000_01) / p[1][c].max(0.000_01))
                            .log10()
                            .abs()
                    })
                    .fold(0_f32, f32::max)
            })
            .collect(),
        0.75,
    );
    let textured = detail > 0.025 && (0..3).any(|c| high[c] - low[c] > mid[c].max(0.000_01) * 0.25);
    Some((edge, textured))
}

fn tight_picture_edges(
    pixel: &impl Fn(usize, usize) -> [f32; 3],
    w: usize,
    h: usize,
) -> Option<[Edge; 4]> {
    let edges: Vec<_> = (0..4)
        .map(|side| tight_edge(pixel, w, h, side))
        .collect::<Option<_>>()?;
    if !edges.iter().any(|(_, textured)| *textured) {
        return None;
    }
    let edges = std::array::from_fn(|i| edges[i].0);
    let region = inscribed_crop(edges, w, h)?;
    if !region.valid_for(w as u32, h as u32) {
        return None;
    }
    let mut interior = Vec::new();
    for row in 0..9 {
        for column in 0..9 {
            let p = region.source_point(
                [0.2 + column as f32 * 0.075, 0.2 + row as f32 * 0.075],
                w as f32 / h as f32,
            );
            interior.push(pixel(
                (p[0] * w as f32) as usize,
                (p[1] * h as f32) as usize,
            ));
        }
    }
    let low = crate::channels(&interior, 0.1);
    let mid = crate::channels(&interior, 0.5);
    let high = crate::channels(&interior, 0.9);
    (0..3)
        .any(|c| high[c] - low[c] > mid[c].max(0.000_01) * 0.45)
        .then_some(edges)
}

// Direct backlight through perforations can dominate a very narrow border.
// Follow a short run of clipped border pixels inward until the whole line clears;
// never chase a bright scene region deeper into the photograph.
fn exclude_perforations(
    mut edge: Edge,
    pixel: &impl Fn(usize, usize) -> [f32; 3],
    w: usize,
    h: usize,
    side: usize,
) -> Edge {
    let span = if side < 2 { h } else { w };
    let direction = if side % 2 == 0 { 1. } else { -1. };
    let limit = (w.min(h) as f32 * 0.03).ceil() as usize;
    let clipped: Vec<usize> = (0..=limit)
        .map(|inset| {
            (0..61)
                .filter(|i| {
                    let cross = span * (15 + i * 70 / 60) / 100;
                    let at = (edge.slope * cross as f32 + edge.intercept + direction * inset as f32)
                        .round()
                        .max(0.) as usize;
                    let (x, y) = if side < 2 { (at, cross) } else { (cross, at) };
                    x < w && y < h && pixel(x, y).iter().any(|v| *v >= 0.995)
                })
                .count()
        })
        .collect();
    if let Some(first_bad) = clipped.iter().take(4).position(|n| *n >= 4) {
        if let Some((inset, _)) = clipped
            .windows(3)
            .enumerate()
            .skip(first_bad + 1)
            .find(|(_, run)| run.iter().all(|n| *n <= 1))
        {
            edge.intercept += direction * inset as f32;
        }
    }
    edge
}

fn base_picture_edges(candidates: &[Vec<(Edge, [f32; 3])>; 4], uneven: bool) -> Option<[Edge; 4]> {
    let edge_angle = |side: usize, edge: Edge| edge.slope.atan() * if side < 2 { -1. } else { 1. };
    let mut best = None;
    let mut best_score = 0.;
    // Four edges must agree on a single tilt, even when contrast differs by side.
    for (side, candidates_for_side) in candidates.iter().enumerate() {
        for &(hypothesis, reference) in candidates_for_side {
            let angle = edge_angle(side, hypothesis);
            let mut selected = Vec::new();
            let mut score = 0.;
            for (side, options) in candidates.iter().enumerate() {
                let choice = options
                    .iter()
                    .filter_map(|&(edge, base)| {
                        if uneven {
                            // Illumination changes level, while unexposed film
                            // retains its channel ratios. Do not match direct
                            // backlight/perforations to film just by geometry.
                            if base.iter().chain(&reference).any(|v| *v >= 0.995) {
                                return None;
                            }
                            let ratios = std::array::from_fn::<_, 3, _>(|c| base[c] / reference[c]);
                            let low = ratios.into_iter().fold(f32::INFINITY, f32::min);
                            let high = ratios.into_iter().fold(0_f32, f32::max);
                            if low < high * 0.84 || low < 0.25 || high > 4. {
                                return None;
                            }
                        }
                        let difference = (edge_angle(side, edge) - angle).abs().to_degrees();
                        (difference < 0.6).then_some((
                            edge.support as f32 - difference * 25.,
                            edge,
                            base,
                        ))
                    })
                    .max_by(|a, b| a.0.total_cmp(&b.0));
                if let Some((value, edge, base)) = choice {
                    score += value;
                    selected.push((edge, base));
                }
            }
            if selected.len() != 4 {
                continue;
            }
            // A scene patch must not become a fictitious base on a missing side.
            let consistent_base = (0..3).all(|channel| {
                let low = selected
                    .iter()
                    .map(|(_, base)| base[channel])
                    .fold(f32::INFINITY, f32::min);
                let high = selected
                    .iter()
                    .map(|(_, base)| base[channel])
                    .fold(0_f32, f32::max);
                low >= high * 0.6
            });
            if (uneven || consistent_base) && score > best_score {
                best_score = score;
                best = Some(std::array::from_fn::<_, 4, _>(|i| selected[i].0));
            }
        }
    }
    best
}

// A narrow strip can be much darker than the opposite border and can be
// preceded by direct backlight. Look for a local film-to-picture transition;
// a percentile of the whole scanline cannot describe both plateaus.
#[allow(clippy::too_many_lines)]
fn uneven_picture_edges(
    pixel: &impl Fn(usize, usize) -> [f32; 3],
    w: usize,
    h: usize,
    candidates: &[Vec<(Edge, [f32; 3])>; 4],
) -> Option<[Edge; 4]> {
    let colors: Vec<_> = candidates
        .iter()
        .flatten()
        .filter(|(_, p)| p.iter().all(|v| *v > 0.000_001 && *v < 0.995))
        .map(|(_, p)| p.map(|v| v / p.iter().sum::<f32>()))
        .collect();
    if colors.len() < 4 {
        return None;
    }
    let reference = crate::channels(&colors, 0.5);
    let mut edges = Vec::new();
    let mut mixed = candidates.clone();
    let level = crate::quantile(
        candidates
            .iter()
            .flatten()
            .map(|(_, p)| p.iter().sum())
            .collect(),
        0.5,
    );
    let angle = crate::quantile(
        candidates
            .iter()
            .enumerate()
            .flat_map(|(side, choices)| {
                choices
                    .iter()
                    .map(move |(edge, _)| edge.slope.atan() * if side < 2 { -1. } else { 1. })
            })
            .collect(),
        0.5,
    );
    for (side, choices) in mixed.iter_mut().enumerate() {
        let (vertical, reverse) = (side >= 2, side % 2 == 1);
        let (length, cross) = if vertical { (h, w) } else { (w, h) };
        let mut points = Vec::new();
        let mut all_points = Vec::new();
        for i in 0..61 {
            let across = cross * (15 + i * 70 / 60) / 100;
            let sample = |n: usize| {
                let n = if reverse { length - 1 - n } else { n };
                if vertical {
                    pixel(across, n)
                } else {
                    pixel(n, across)
                }
            };
            let mut best = None;
            let mut score = 0.08;
            for n in 4..length * 35 / 100 - 4 {
                let before = crate::channels(&[sample(n - 3), sample(n - 2), sample(n - 1)], 0.5);
                if !before
                    .iter()
                    .all(|v| v.is_finite() && *v > 0.000_01 && *v < 0.98)
                {
                    continue;
                }
                let sum = before.iter().sum::<f32>();
                if (0..3).any(|c| (before[c] / sum / reference[c] - 1.).abs() > 0.12) {
                    continue;
                }
                // A flat base plateau must precede the step, not a demosaic
                // spike or a transition out of a clipped perforation.
                if (1..=3).any(|d| (0..3).any(|c| (sample(n - d)[c] / before[c] - 1.).abs() > 0.18))
                {
                    continue;
                }
                let after = crate::channels(&[sample(n + 1), sample(n + 2), sample(n + 3)], 0.5);
                let contrast =
                    crate::quantile((0..3).map(|c| 1. - after[c] / before[c]).collect(), 0.5);
                if contrast > 0.08 {
                    all_points.push([
                        across as f32 + 0.5,
                        if reverse {
                            (length - n) as f32
                        } else {
                            n as f32
                        },
                    ]);
                }
                if contrast > score {
                    score = contrast;
                    best = Some(n);
                }
            }
            if let Some(n) = best {
                points.push([
                    across as f32 + 0.5,
                    if reverse {
                        (length - n) as f32
                    } else {
                        n as f32
                    },
                ]);
            }
        }
        let tolerance = (length as f32 * 0.002).max(1.5);
        let fitted = fit_edge(&points, cross as f32, tolerance).filter(|e| {
            (e.slope.atan() * if side < 2 { -1. } else { 1. } - angle).abs() < 0.6_f32.to_radians()
        });
        let fitted = fitted.or_else(|| {
            parallel_edge(
                &all_points,
                angle.tan() * if side < 2 { -1. } else { 1. },
                length,
                cross,
                tolerance,
            )
        });
        if let Some(edge) = fitted {
            edges.push(edge);
            choices.push((edge, reference.map(|v| v * level)));
        }
    }
    if edges.len() == 4 {
        let edges = std::array::from_fn(|i| edges[i]);
        if inscribed_crop(edges, w, h).is_some() {
            return Some(edges);
        }
    }
    base_picture_edges(&mixed, true)
}

// A strong scene feature can hide a weaker frame step on an individual line.
// Vote for the common intercept at the tilt supported by the other borders,
// giving each scanline at most one vote per bin.
#[allow(clippy::float_cmp)] // Identical scanline coordinates identify duplicate votes.
fn parallel_edge(
    points: &[[f32; 2]],
    slope: f32,
    length: usize,
    cross: usize,
    tolerance: f32,
) -> Option<Edge> {
    let mut votes = vec![0; length];
    let mut last = vec![f32::NEG_INFINITY; length];
    for p in points {
        let offset = p[1] - slope * p[0];
        for bin in
            (offset - tolerance).max(0.) as usize..=((offset + tolerance) as usize).min(length - 1)
        {
            if last[bin] != p[0] {
                votes[bin] += 1;
                last[bin] = p[0];
            }
        }
    }
    let at = votes.iter().enumerate().max_by_key(|(_, n)| *n)?.0 as f32;
    let mut selected: Vec<[f32; 2]> = Vec::new();
    for &p in points {
        if (p[1] - slope * p[0] - at).abs() <= tolerance
            && selected.last().is_none_or(|q| q[0] != p[0])
        {
            selected.push(p);
        }
    }
    fit_edge(&selected, cross as f32, tolerance)
}

/// Find the exposed picture inside unexposed film base, with up to 5° of tilt.
///
/// Uses linear capture channels, independent of inversion or accepted calibration.
/// Fits film-base transitions, or the inner dark-to-picture transition when
/// the capture leaves no broad film-base strip. Four supported edges and scene
/// detail are required for that tight-framing path. Blank or incomplete frames
/// return `None`.
/// A small inward margin excludes the soft transition and resampling footprint.
#[must_use]
pub fn detect_frame_crop(scan: &Scan) -> Option<Rect> {
    let image = &scan.image;
    let (width, height) = (image.width as usize, image.height as usize);
    if width < 64 || height < 64 || image.pixels.len() != width.checked_mul(height)? {
        return None;
    }
    // Bound analysis work independently of RAW resolution.
    let scale = (width.max(height) as f32 / 960.).max(1.);
    let (w, h) = (
        (width as f32 / scale) as usize,
        (height as f32 / scale) as usize,
    );
    if w < 64 || h < 64 {
        return None;
    }
    let pixel = |x: usize, y: usize| image.pixels[(y * height / h) * width + x * width / w];
    let candidates: [Vec<_>; 4] = std::array::from_fn(|side| {
        let mut found = Vec::new();
        for percentile in [0.96, 0.99] {
            for contrast in [0.06, 0.12, 0.18] {
                if let Some(candidate) = find_edge(&pixel, w, h, side, percentile, contrast) {
                    found.push(candidate);
                }
            }
        }
        found
    });
    let uneven = || {
        let edges = uneven_picture_edges(&pixel, w, h, &candidates)?;
        let region = inscribed_crop(edges, w, h)?;
        // This fallback is for tight captures with narrow uneven border strips.
        // Do not explain a missing side by moving deep into the photograph.
        [
            region.x,
            region.y,
            1. - region.x - region.w,
            1. - region.y - region.h,
        ]
        .iter()
        .all(|v| *v < 0.23)
        .then_some(edges)
    };
    let best = base_picture_edges(&candidates, false);
    let tight = tight_picture_edges(&pixel, w, h);
    // With detail reaching the surround, a much smaller rectangle can be a set
    // of aligned scene features rather than film-base boundaries.
    let best = best.filter(|edges| {
        tight.is_none_or(|outer| {
            match (inscribed_crop(*edges, w, h), inscribed_crop(outer, w, h)) {
                (Some(inner), Some(outer)) => inner.w * inner.h >= outer.w * outer.h * 0.8,
                _ => true,
            }
        })
    });
    let edges = if let Some(edges) = best {
        edges
    } else if let Some(mut edges) = tight {
        for (side, edge) in edges.iter_mut().enumerate() {
            let cross_span = if side < 2 { h } else { w } as f32;
            let at = edge.slope * cross_span * 0.5 + edge.intercept;
            let direction = if side % 2 == 0 { 1. } else { -1. };
            // Prefer a visible inner film-base transition close to the dark
            // boundary. Never replace it with a distant feature in the scene.
            if let Some((_, inner)) = candidates[side]
                .iter()
                .filter_map(|&(candidate, _)| {
                    let inset =
                        (candidate.slope * cross_span * 0.5 + candidate.intercept - at) * direction;
                    ((0.0..w.min(h) as f32 * 0.04).contains(&inset)
                        && (candidate.slope.atan() - edge.slope.atan()).abs()
                            < 0.6_f32.to_radians())
                    .then_some((inset, candidate))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
            {
                *edge = inner;
            }
            *edge = exclude_perforations(*edge, &pixel, w, h, side);
        }
        edges
    } else {
        uneven()?
    };
    let crop = inscribed_crop(edges, w, h)?;
    crop.valid_for(image.width, image.height).then_some(crop)
}

fn inscribed_crop(edges: [Edge; 4], w: usize, h: usize) -> Option<Rect> {
    let angles = edges.map(|e| e.slope.atan());
    let angles = [-angles[0], -angles[1], angles[2], angles[3]];
    let angle = angles
        .iter()
        .zip(edges)
        .map(|(a, e)| a * e.support as f32)
        .sum::<f32>()
        / edges.iter().map(|e| e.support as f32).sum::<f32>();
    if angles
        .iter()
        .any(|a| (a - angle).abs() > 0.8_f32.to_radians())
    {
        return None;
    }
    // Below this, preview-scale edge uncertainty outweighs the benefit of resampling.
    let angle = if angle.abs() < 0.1_f32.to_radians() {
        0.
    } else {
        angle
    };
    let (sin, cos) = angle.sin_cos();
    let mut bounds = [0.; 4];
    for (i, e) in edges.iter().enumerate() {
        let span = if i < 2 { h } else { w } as f32;
        let projected = [0., span].map(|cross| {
            let at = e.slope * cross + e.intercept;
            if i < 2 {
                cos * at + sin * cross
            } else {
                -sin * cross + cos * at
            }
        });
        bounds[i] = if i % 2 == 0 {
            projected[0].max(projected[1])
        } else {
            projected[0].min(projected[1])
        };
    }
    let margin = (w.min(h) as f32 * 0.004).max(3.);
    let [left, right, top, bottom] = bounds;
    let (cw, ch) = (right - left - margin * 2., bottom - top - margin * 2.);
    if cw < w as f32 * 0.35 || ch < h as f32 * 0.35 {
        return None;
    }
    let (u, v) = ((left + right) * 0.5, (top + bottom) * 0.5);
    let crop = Rect {
        x: (cos * u - sin * v - cw * 0.5) / w as f32,
        y: (sin * u + cos * v - ch * 0.5) / h as f32,
        w: cw / w as f32,
        h: ch / h as f32,
        angle: angle.to_degrees(),
    };
    Some(crop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use greign_pipeline::LinearRgbImage;

    fn tilted_frame(angle: f32, monochrome: bool) -> (Scan, Rect) {
        let (w, h) = (480, 320);
        let expected = Rect {
            x: 0.17,
            y: 0.13,
            w: 0.72,
            h: 0.74,
            angle,
        };
        let mut image = LinearRgbImage {
            width: w,
            height: h,
            pixels: vec![[0.; 3]; (w * h) as usize],
        };
        let base = if monochrome {
            [0.4; 3]
        } else {
            [0.4, 0.22, 0.10]
        };
        for y in 0..h {
            for x in 0..w {
                let p = [(x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32];
                let q = expected.crop_point(p, w as f32 / h as f32);
                let gain = if q.iter().all(|v| (0.0..=1.0).contains(v)) {
                    0.3 + 0.35 * (x as f32 * 0.1).sin().abs()
                } else if q.iter().all(|v| (-0.07..=1.07).contains(v)) {
                    1.
                } else {
                    0.0001
                };
                // Uneven illumination, including a weaker right-hand border.
                image.pixels[(y * w + x) as usize] = base.map(|v| v * gain * (1. - 0.3 * p[0]));
            }
        }
        (
            Scan {
                image,
                camera_matrix: None,
                white_balance: [1.; 3],
                capture: "synthetic film".into(),
            },
            expected,
        )
    }

    #[test]
    fn inner_crop_excludes_film_base_and_deskews_both_directions() {
        for angle in [-4.5, -3.0, 0., 2.5, 4.5] {
            for monochrome in [false, true] {
                let (mut scan, expected) = tilted_frame(angle, monochrome);
                // Dust and dark marks interrupt the film base on some scanlines.
                for y in 100..115 {
                    for x in 20..80 {
                        scan.image.pixels[y * 480 + x] = [0.00001; 3];
                    }
                }
                let crop = detect_frame_crop(&scan).expect("four supported picture edges");
                assert!(
                    (crop.angle - angle).abs() < 0.25,
                    "{crop:?} expected {angle}"
                );
                assert!(
                    crop.w > expected.w * 0.95 && crop.h > expected.h * 0.95,
                    "{crop:?}"
                );
                for corner in [[0., 0.], [1., 0.], [1., 1.], [0., 1.]] {
                    let p = expected.crop_point(crop.source_point(corner, 1.5), 1.5);
                    assert!(
                        p.iter().all(|v| (0.0..=1.0).contains(v)),
                        "film base in {crop:?}: {p:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn uneven_border_brightness_keeps_the_picture_instead_of_the_backlight() {
        for angle in [-2., 0., 2.] {
            let (mut scan, expected) = tilted_frame(angle, false);
            for (i, pixel) in scan.image.pixels.iter_mut().enumerate() {
                let gain = 10_f32.powf(-0.45 * (i % 480) as f32 / 480.);
                *pixel = pixel.map(|v| v * gain);
            }
            let crop = detect_frame_crop(&scan).expect("uneven but color-consistent film base");
            assert!((crop.angle - angle).abs() < 0.3);
            assert!(crop.w > expected.w * 0.95 && crop.h > expected.h * 0.95);
            for corner in [[0., 0.], [1., 0.], [0., 1.], [1., 1.]] {
                let point = expected.crop_point(crop.source_point(corner, 1.5), 1.5);
                assert!(point.iter().all(|v| (0.0..=1.0).contains(v)), "{crop:?}");
            }
        }
    }

    #[test]
    fn faint_soft_boundaries_still_leave_only_picture_pixels() {
        let (mut scan, expected) = tilted_frame(-2., false);
        for y in 0..320 {
            for x in 0..480 {
                let p = [(x as f32 + 0.5) / 480., (y as f32 + 0.5) / 320.];
                if expected
                    .crop_point(p, 1.5)
                    .iter()
                    .all(|v| (0.0..=1.0).contains(v))
                {
                    scan.image.pixels[y * 480 + x] =
                        [0.4, 0.22, 0.10].map(|v| v * 0.90 * (1. - 0.3 * p[0]));
                }
            }
        }
        let source = scan.image.clone();
        for y in 1..319 {
            for x in 1..479 {
                scan.image.pixels[y * 480 + x] = std::array::from_fn(|c| {
                    (y - 1..=y + 1)
                        .flat_map(|yy| (x - 1..=x + 1).map(move |xx| (yy, xx)))
                        .map(|(yy, xx)| source.pixels[yy * 480 + xx][c])
                        .sum::<f32>()
                        / 9.
                });
            }
        }
        let crop = detect_frame_crop(&scan).unwrap();
        assert!((crop.angle - expected.angle).abs() < 0.25, "{crop:?}");
        for corner in [[0., 0.], [1., 0.], [1., 1.], [0., 1.]] {
            let p = expected.crop_point(crop.source_point(corner, 1.5), 1.5);
            assert!(p.iter().all(|v| (0.0..=1.0).contains(v)), "{p:?}");
        }
    }

    #[test]
    fn portrait_and_large_scan_geometry_remains_consistent() {
        let (mut scan, expected) = tilted_frame(2.5, false);
        let source = scan.image.clone();
        scan.image = LinearRgbImage {
            width: 320,
            height: 480,
            pixels: (0..320 * 480)
                .map(|i| source.pixels[(319 - i % 320) * 480 + i / 320])
                .collect(),
        };
        let expected = Rect {
            x: 1. - expected.y - expected.h,
            y: expected.x,
            w: expected.h,
            h: expected.w,
            angle: expected.angle,
        };
        let portrait = detect_frame_crop(&scan).unwrap();
        assert!(
            (portrait.angle - expected.angle).abs() < 0.25,
            "{portrait:?}"
        );
        for corner in [[0., 0.], [1., 0.], [1., 1.], [0., 1.]] {
            let p = expected.crop_point(portrait.source_point(corner, 2. / 3.), 2. / 3.);
            assert!(p.iter().all(|v| (0.0..=1.0).contains(v)), "{p:?}");
        }
        let source = scan.image.clone();
        scan.image = LinearRgbImage {
            width: 960,
            height: 1440,
            pixels: (0..960 * 1440)
                .map(|i| source.pixels[(i / 960 / 3) * 320 + i % 960 / 3])
                .collect(),
        };
        let enlarged = detect_frame_crop(&scan).unwrap();
        assert!((enlarged.angle - portrait.angle).abs() < 0.2);
        assert!((enlarged.w - portrait.w).abs() < 0.015 && (enlarged.h - portrait.h).abs() < 0.015);
    }

    #[test]
    fn inner_crop_declines_missing_base_and_invalid_samples() {
        let mut scan = framed();
        assert!(
            detect_frame_crop(&scan).is_none(),
            "a holder alone is not a picture edge"
        );
        for value in [0., 0.2, f32::NAN, f32::INFINITY] {
            scan.image.pixels.fill([value; 3]);
            assert!(detect_frame_crop(&scan).is_none());
        }
        let (mut scan, _) = tilted_frame(0., false);
        for y in 0..320 {
            for x in 0..150 {
                scan.image.pixels[y * 480 + x] = [0.1, 0.05, 0.02];
            }
        }
        assert!(
            detect_frame_crop(&scan).is_none(),
            "missing left film-base edge"
        );
        scan.image = LinearRgbImage {
            width: 64,
            height: 4096,
            pixels: vec![[0.2; 3]; 64 * 4096],
        };
        assert!(detect_frame_crop(&scan).is_none(), "extreme aspect ratio");
    }

    fn tight_frame(angle: f32, monochrome: bool) -> (Scan, Rect) {
        let (mut scan, _) = tilted_frame(angle, monochrome);
        let expected = Rect {
            x: 0.19,
            y: 0.15,
            w: 0.73,
            h: 0.70,
            angle,
        };
        for y in 0..320 {
            for x in 0..480 {
                let q =
                    expected.crop_point([(x as f32 + 0.5) / 480., (y as f32 + 0.5) / 320.], 1.5);
                let distance = (q[0].min(1. - q[0]) * expected.w * 480.)
                    .min(q[1].min(1. - q[1]) * expected.h * 320.);
                let feather = (distance / 2.).clamp(0., 1.);
                let detail = 0.4 + 0.15 * (x as f32 * 0.07).sin() + 0.12 * (y as f32 * 0.09).cos();
                let base = if monochrome {
                    [0.4; 3]
                } else {
                    [0.4, 0.22, 0.1]
                };
                let mut value = base.map(|v| 0.0003 + v * detail * feather);
                // A short row of direct-backlight perforations inside the surround.
                if q[1] > 0. && q[1] < 0.024 && q[0] > 0. && q[0] < 1. && (q[0] * 10.).fract() < 0.3
                {
                    value = [1.2; 3];
                }
                scan.image.pixels[y * 480 + x] = value;
            }
        }
        (scan, expected)
    }

    #[test]
    fn tight_framing_keeps_picture_and_excludes_perforations_with_both_tilt_directions() {
        for angle in [-3., 0., 3.] {
            for monochrome in [false, true] {
                let (scan, expected) = tight_frame(angle, monochrome);
                let crop = detect_frame_crop(&scan).expect("picture reaches the dark surround");
                assert!(
                    (crop.angle - angle).abs() < 0.4,
                    "{crop:?} expected {angle}"
                );
                assert!(
                    crop.w > expected.w * 0.90 && crop.h > expected.h * 0.90,
                    "{crop:?}"
                );
                for side in 0..4 {
                    for i in 0..100 {
                        let t = i as f32 / 99.;
                        let q = [[t, 0.], [1., t], [t, 1.], [0., t]][side];
                        let p = crop.source_point(q, 1.5);
                        let inside = expected.crop_point(p, 1.5);
                        assert!(
                            inside.iter().all(|v| (0.0..=1.0).contains(v)),
                            "{inside:?} in {crop:?}"
                        );
                        let pixel = scan.image.pixels
                            [(p[1] * 320.) as usize * 480 + (p[0] * 480.) as usize];
                        assert!(pixel.iter().all(|v| *v < 0.995), "perforation in {crop:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn perforation_clearance_starts_after_the_bright_border_not_before_it() {
        let edge = Edge {
            slope: 0.,
            intercept: 20.,
            support: 61,
        };
        let pixel = |x: usize, y| {
            if (23..=25).contains(&y) && x % 20 < 8 {
                [1.2; 3]
            } else {
                [0.2; 3]
            }
        };
        let cleared = exclude_perforations(edge, &pixel, 240, 320, 2);
        assert!(
            cleared.intercept >= 26.,
            "The clear band must be inside the perforations"
        );
    }

    #[test]
    fn tight_framing_fallback_does_not_substitute_a_wide_film_base_border() {
        let (scan, _) = tilted_frame(0., false);
        let pixel = |x, y| scan.image.pixels[y * 480 + x];
        assert!(tight_picture_edges(&pixel, 480, 320).is_none());
        let mut blank = framed();
        for (i, p) in blank.image.pixels.iter_mut().enumerate() {
            if p[0] > 0.1 {
                *p = p.map(|v| v * (1. - 0.2 * (i % 240) as f32 / 240.));
            }
        }
        assert!(
            detect_frame_crop(&blank).is_none(),
            "a blank with illumination falloff is not a picture"
        );
    }

    #[test]
    fn rotated_sampling_and_inverse_coordinates_agree() {
        let image = LinearRgbImage {
            width: 120,
            height: 80,
            pixels: (0..120 * 80)
                .map(|i| [i as f32 % 120., (i / 120) as f32, 0.])
                .collect(),
        };
        let prepared = crate::Prepared {
            image,
            material: crate::Material::Positive,
            negative: true,
        };
        let crop = Rect {
            x: 0.15,
            y: 0.2,
            w: 0.7,
            h: 0.6,
            angle: -3.,
        };
        let output = crate::finish(
            &prepared,
            crate::Adjustments::default(),
            crop,
            0,
            false,
            &[],
            &|| false,
        )
        .unwrap();
        for y in 0..output.height {
            for x in 0..output.width {
                let q = [
                    (x as f32 + 0.5) / output.width as f32,
                    (y as f32 + 0.5) / output.height as f32,
                ];
                let p = crop.source_point(q, 1.5);
                let back = crop.crop_point(p, 1.5);
                assert!((q[0] - back[0]).abs() < 0.00001 && (q[1] - back[1]).abs() < 0.00001);
                let actual = output.pixels[(y * output.width + x) as usize];
                assert!((actual[0] - (p[0] * 120. - 0.5)).abs() < 0.0001);
                assert!((actual[1] - (p[1] * 80. - 0.5)).abs() < 0.0001);
            }
        }
        let invalid = Rect {
            angle: 4.,
            ..Rect::default()
        };
        assert!(!invalid.valid_for(120, 80));
        assert!(
            crate::finish(
                &prepared,
                crate::Adjustments::default(),
                invalid,
                0,
                false,
                &[],
                &|| false
            )
            .is_err()
        );
    }

    fn framed() -> Scan {
        let mut scan = Scan {
            image: LinearRgbImage {
                width: 240,
                height: 160,
                pixels: vec![[0.00001; 3]; 240 * 160],
            },
            camera_matrix: None,
            white_balance: [1.; 3],
            capture: "test".into(),
        };
        for y in 18..147 {
            for x in 31..230 {
                scan.image.pixels[y * 240 + x] = [0.4, 0.2, 0.1];
            }
        }
        scan
    }

    #[test]
    fn analysis_uses_the_tilted_picture_while_estimating_base_outside_it() {
        let (mut scan, picture) = tilted_frame(3., false);
        for y in 0..320 {
            for x in 0..480 {
                let q = picture.crop_point([(x as f32 + 0.5) / 480., (y as f32 + 0.5) / 320.], 1.5);
                if q.iter().all(|v| (0.0..=1.0).contains(v)) {
                    scan.image.pixels[y * 480 + x] =
                        [0.4, 0.22, 0.10].map(|v| v * 10_f32.powf(-0.3 - 1.5 * q[0]));
                }
            }
        }
        let analysis = crate::analyze(&[&scan], None, Rect::default()).unwrap();
        assert!(
            analysis.base[0] > 0.3,
            "Film base must come from outside the exposure: {:?}",
            analysis.base
        );
        assert!(
            analysis.floor.iter().all(|v| *v > 0.15),
            "Film base leaked into image density: {:?}",
            analysis.floor
        );
        assert!((1.3..1.6).contains(&analysis.range), "{}", analysis.range);
        assert_eq!(analysis.version, crate::ANALYSIS_VERSION);
    }

    #[test]
    fn black_holder_does_not_inflate_density_range() {
        let mut scan = framed();
        for y in 18..147 {
            for x in 31..230 {
                let density = (x - 31) as f32 / 198. * 1.5;
                scan.image.pixels[y * 240 + x] =
                    [0.4, 0.2, 0.1].map(|v| v * 10.0_f32.powf(-density));
            }
        }
        let analysis = crate::analyze(&[&scan], Some([0.4, 0.2, 0.1]), Rect::default()).unwrap();
        assert!(
            analysis.range > 1.3 && analysis.range < 1.6,
            "{}",
            analysis.range
        );
        assert_eq!(analysis.version, crate::ANALYSIS_VERSION);
    }
}
