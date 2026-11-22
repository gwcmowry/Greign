//! Offline quarter-turn suggestions from a bundled `YuNet` face/landmark model.
use anyhow::{Result, ensure};
use image::{RgbImage, imageops};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tract_onnx::prelude::*;

const SIZE: usize = 640;
pub(crate) const MODEL: &[u8] = include_bytes!("../models/yunet-2023mar.onnx");

/// Evidence for each clockwise quarter turn. Scores are ranking evidence, not probabilities.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UprightGuess {
    /// Absolute clockwise quarter turns of the supplied image, if unambiguous.
    pub rotation: Option<u8>,
    /// Combined face evidence for 0, 90, 180 and 270 degrees.
    pub scores: [f32; 4],
    /// Number of plausible faces in each orientation.
    pub faces: [usize; 4],
    /// Short explanation suitable for a review message.
    pub reason: String,
}

/// Local inference engine with explicit CPU and optional CUDA constructors. Construct once and reuse across a roll.
pub struct UprightDetector {
    engine: Engine,
}

enum Engine {
    Cpu {
        model: Box<TypedRunnableModel<TypedModel>>,
        workers: rayon::ThreadPool,
    },
    #[cfg(target_os = "linux")]
    Cuda(crate::upright_cuda::CudaModel),
}

pub(crate) const OUTPUTS: [&str; 12] = [
    "cls_8", "cls_16", "cls_32", "obj_8", "obj_16", "obj_32", "bbox_8", "bbox_16", "bbox_32",
    "kps_8", "kps_16", "kps_32",
];

#[derive(Clone, Debug)]
struct Face {
    bounds: [f32; 4],
    landmarks: [[f32; 2]; 5],
    confidence: f32,
}

impl UprightDetector {
    /// Load the bundled model without network or runtime downloads.
    /// # Errors
    /// Invalid model or unsupported inference operations.
    pub fn new() -> Result<Self> {
        Self::with_workers(
            std::thread::available_parallelism()
                .map_or(1, usize::from)
                .min(4),
        )
    }

    /// Bound CPU use for the four independent orientation passes. One worker
    /// provides the serial reference; the default uses up to four workers.
    /// # Errors
    /// Worker count outside 1–4, model loading failure, or thread creation failure.
    pub fn with_workers(workers: usize) -> Result<Self> {
        ensure!(
            (1..=4).contains(&workers),
            "Upright worker count must be 1–4"
        );
        let mut model = tract_onnx::onnx().model_for_read(&mut std::io::Cursor::new(MODEL))?;
        model.set_input_fact(0, f32::fact([1, 3, SIZE, SIZE]).into())?;
        model.set_output_names(OUTPUTS)?;
        Ok(Self {
            engine: Engine::Cpu {
                model: Box::new(model.into_optimized()?.into_runnable()?),
                workers: rayon::ThreadPoolBuilder::new()
                    .num_threads(workers)
                    .thread_name(|i| format!("upright-{i}"))
                    .build()?,
            },
        })
    }

    /// Create an explicit CUDA engine. Errors never silently select CPU.
    /// The application handles fallback, and keeps the runtime's libraries local.
    /// # Errors
    /// Missing/incompatible runtime or CUDA initialization failure.
    pub fn with_cuda(library: &std::path::Path) -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            Ok(Self {
                engine: Engine::Cuda(crate::upright_cuda::CudaModel::new(library)?),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = library;
            anyhow::bail!("CUDA upright currently requires Linux");
        }
    }

    /// Actual inference backend, suitable for status and benchmark logs.
    #[must_use]
    pub fn backend(&self) -> &'static str {
        match &self.engine {
            Engine::Cpu { .. } => "CPU",
            #[cfg(target_os = "linux")]
            Engine::Cuda(_) => "CUDA",
        }
    }

    /// Flush an optional CUDA operator profile enabled by the process environment.
    /// # Errors
    /// Runtime profiling failure.
    pub fn finish_profile(&self) -> Result<Option<String>> {
        match &self.engine {
            Engine::Cpu { .. } => Ok(None),
            #[cfg(target_os = "linux")]
            Engine::Cuda(model) => model.finish_profile(),
        }
    }

    /// Compare all four orientations of a converted positive picture.
    /// # Errors
    /// Empty input or inference failure.
    pub fn detect(&self, image: &RgbImage) -> Result<UprightGuess> {
        ensure!(
            image.width() > 0 && image.height() > 0,
            "Empty orientation preview"
        );
        let scale = SIZE as f32 / image.width().max(image.height()) as f32;
        let small = imageops::resize(
            image,
            (image.width() as f32 * scale).round().max(1.) as u32,
            (image.height() as f32 * scale).round().max(1.) as u32,
            imageops::FilterType::Triangle,
        );
        let views = self.views(&small)?;
        let mut guess = decide(&views);
        if guess.rotation.is_none() {
            // A marginal face needs both a clear lead and a repeat detection at
            // the same location after reflection. Never break close ties this way.
            for (turn, candidates) in views.iter().enumerate() {
                let Some(face) = candidates.first() else {
                    continue;
                };
                if face.confidence < 0.85
                    || guess
                        .scores
                        .iter()
                        .enumerate()
                        .any(|(other, score)| other != turn && guess.scores[turn] - score < 0.15)
                {
                    continue;
                }
                let reflected = imageops::flip_horizontal(&quarter_turn(&small, turn));
                let [x, y, w, h] = face.bounds;
                let expected = [reflected.width() as f32 - x - w, y, w, h];
                if self
                    .faces(&reflected)?
                    .iter()
                    .any(|f| f.confidence >= 0.85 && overlap(expected, f.bounds) >= 0.5)
                {
                    guess.rotation = Some(turn as u8);
                    guess.reason = "Face orientation confirmed in mirrored view".into();
                }
                break;
            }
        }
        if guess.rotation.is_none() {
            // Large close-up faces sometimes produce better landmarks at a
            // smaller scale. Check all four views so alternatives still compete.
            let compact = imageops::resize(
                &small,
                (small.width() / 2).max(1),
                (small.height() / 2).max(1),
                imageops::FilterType::Triangle,
            );
            let mut compact_views = self.views(&compact)?;
            let retry = decide(&compact_views);
            if let Some(turn) = retry.rotation {
                let index = usize::from(turn);
                let conflicting = guess.scores.iter().enumerate().any(|(other, &score)| {
                    other != index && score >= 0.85 && score > guess.scores[index]
                });
                if !conflicting {
                    return Ok(UprightGuess {
                        reason: "Clear face orientation at smaller scale".into(),
                        ..retry
                    });
                }
            }
            let mut full_views = views;
            source_boxes(&mut full_views, small.dimensions());
            source_boxes(&mut compact_views, compact.dimensions());
            let mut ranking = [0, 1, 2, 3];
            ranking.sort_by(|&a, &b| guess.scores[b].total_cmp(&guess.scores[a]));
            let [best, second, ..] = ranking;
            if agrees_on_two_faces(&full_views, &compact_views, best) {
                guess.rotation = Some(best as u8);
                guess.reason = "Two faces agree across scales".into();
            } else if let ([first], [rival]) = (&full_views[best][..], &full_views[second][..])
                && rival.confidence >= 0.75
                && first.confidence - rival.confidence < 0.15
                && overlap(first.bounds, rival.bounds) >= 0.45
                && let Some(verified) =
                    self.verify_face_crop(image, first.bounds, [best, second])?
            {
                return Ok(verified);
            }
        }
        if guess.rotation.is_none() {
            guess.rotation = match &self.engine {
                Engine::Cpu { workers, .. } => crate::scene_upright::verify(
                    image,
                    Some(workers),
                    crate::scene_upright::cpu_logits,
                ),
                #[cfg(target_os = "linux")]
                Engine::Cuda(model) => {
                    crate::scene_upright::verify(image, None, |input| model.scene_logits(input))
                }
            }?;
            if guess.rotation.is_some() {
                guess.reason = "Whole scene agrees in all four orientations".into();
            }
        }
        Ok(guess)
    }

    fn verify_face_crop(
        &self,
        image: &RgbImage,
        bounds: [f32; 4],
        contenders: [usize; 2],
    ) -> Result<Option<UprightGuess>> {
        // Keep context around the face and take pixels from the original input,
        // before the 640px pass. Missing context at an image edge is clipped.
        let (width, height) = (image.width() as f32, image.height() as f32);
        let [x, y, w, h] = bounds;
        let half = (w * width).max(h * height);
        let cx = (x + w * 0.5) * width;
        let cy = (y + h * 0.5) * height;
        let left = (cx - half).floor().clamp(0., width - 1.) as u32;
        let top = (cy - half).floor().clamp(0., height - 1.) as u32;
        let cw = (cx + half).ceil().clamp(left as f32 + 1., width) as u32 - left;
        let ch = (cy + half).ceil().clamp(top as f32 + 1., height) as u32 - top;
        let crop = imageops::crop_imm(image, left, top, cw, ch).to_image();
        let crop = if cw.max(ch) > SIZE as u32 {
            let scale = SIZE as f32 / cw.max(ch) as f32;
            imageops::resize(
                &crop,
                ((cw as f32 * scale).round() as u32).max(1),
                ((ch as f32 * scale).round() as u32).max(1),
                imageops::FilterType::Triangle,
            )
        } else {
            crop
        };
        let expected = [
            (x * width - left as f32) / cw as f32,
            (y * height - top as f32) / ch as f32,
            w * width / cw as f32,
            h * height / ch as f32,
        ];
        let mut views = self.views(&crop)?;
        source_boxes(&mut views, crop.dimensions());
        // Verify the disputed face itself, even if a second person enters the crop.
        for view in &mut views {
            view.retain(|f| overlap(f.bounds, expected) >= 0.45);
            view.truncate(1);
        }
        let mut verified = decide(&views);
        if verified
            .rotation
            .is_some_and(|t| contenders.contains(&usize::from(t)))
        {
            verified.reason = "Face orientation confirmed in source crop".into();
            Ok(Some(verified))
        } else {
            Ok(None)
        }
    }

    fn views(&self, image: &RgbImage) -> Result<[Vec<Face>; 4]> {
        let mut views: [Vec<Face>; 4] = std::array::from_fn(|_| Vec::new());
        let evaluate = |turn| self.faces(&quarter_turn(image, turn));
        match &self.engine {
            Engine::Cpu { workers, .. } => workers.install(|| {
                views
                    .par_iter_mut()
                    .enumerate()
                    .try_for_each(|(turn, view)| -> Result<()> {
                        *view = evaluate(turn)?;
                        Ok(())
                    })
            })?,
            #[cfg(target_os = "linux")]
            Engine::Cuda(_) => {
                for (turn, view) in views.iter_mut().enumerate() {
                    *view = evaluate(turn)?;
                }
            }
        }
        Ok(views)
    }

    fn faces(&self, image: &RgbImage) -> Result<Vec<Face>> {
        // YuNet expects raw BGR values in NCHW order, padded at right/bottom.
        let input = tract_ndarray::Array4::from_shape_fn((1, 3, SIZE, SIZE), |(_, c, y, x)| {
            if x < image.width() as usize && y < image.height() as usize {
                f32::from(image.get_pixel(x as u32, y as u32)[2 - c])
            } else {
                0.
            }
        });
        match &self.engine {
            Engine::Cpu { model, .. } => {
                let input: Tensor = input.into();
                let outputs = model.run(tvec!(input.into()))?;
                let slices = outputs
                    .iter()
                    .map(|v| v.as_slice::<f32>())
                    .collect::<Result<Vec<_>>>()?;
                decode(&slices, image)
            }
            #[cfg(target_os = "linux")]
            Engine::Cuda(model) => {
                let outputs = model.run(input.into_raw_vec_and_offset().0)?;
                let slices: Vec<_> = outputs.iter().map(Vec::as_slice).collect();
                decode(&slices, image)
            }
        }
    }
}

fn quarter_turn(image: &RgbImage, turn: usize) -> RgbImage {
    match turn {
        1 => imageops::rotate90(image),
        2 => imageops::rotate180(image),
        3 => imageops::rotate270(image),
        _ => image.clone(),
    }
}

fn decode(outputs: &[&[f32]], image: &RgbImage) -> Result<Vec<Face>> {
    ensure!(outputs.len() == 12, "Unexpected YuNet output count");
    let mut faces = Vec::new();
    for (level, stride) in [8, 16, 32].into_iter().enumerate() {
        let cols = SIZE / stride;
        let count = cols * cols;
        let class = outputs[level];
        let object = outputs[level + 3];
        let bounds = outputs[level + 6];
        let points = outputs[level + 9];
        ensure!(
            class.len() == count
                && object.len() == count
                && bounds.len() == count * 4
                && points.len() == count * 10,
            "Unexpected YuNet output shape"
        );
        for index in 0..count {
            let confidence = (class[index].clamp(0., 1.) * object[index].clamp(0., 1.)).sqrt();
            if !confidence.is_finite() || confidence < 0.6 {
                continue;
            }
            let column = (index % cols) as f32;
            let row = (index / cols) as f32;
            let stride = stride as f32;
            let center_x = (column + bounds[index * 4]) * stride;
            let center_y = (row + bounds[index * 4 + 1]) * stride;
            let width = bounds[index * 4 + 2].exp() * stride;
            let height = bounds[index * 4 + 3].exp() * stride;
            let landmarks = std::array::from_fn(|point| {
                [
                    (points[index * 10 + point * 2] + column) * stride,
                    (points[index * 10 + point * 2 + 1] + row) * stride,
                ]
            });
            let face = Face {
                bounds: [
                    center_x - width * 0.5,
                    center_y - height * 0.5,
                    width,
                    height,
                ],
                landmarks,
                confidence,
            };
            if face
                .bounds
                .iter()
                .chain(face.landmarks.iter().flatten())
                .all(|v| v.is_finite())
                && center_x >= 0.
                && center_x < image.width() as f32
                && center_y >= 0.
                && center_y < image.height() as f32
                && width >= 16.
                && height >= 16.
            {
                faces.push(face);
            }
        }
    }
    faces.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    let mut kept: Vec<Face> = Vec::new();
    for face in faces.into_iter().take(500) {
        if kept
            .iter()
            .all(|other| overlap(face.bounds, other.bounds) <= 0.3)
        {
            kept.push(face);
        }
    }
    Ok(kept.into_iter().filter(plausible).collect())
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> f32 {
    let width = ((a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0])).max(0.);
    let height = ((a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1])).max(0.);
    let intersection = width * height;
    intersection / (a[2] * a[3] + b[2] * b[3] - intersection).max(0.0001)
}

// Map each rotated view into normalized coordinates of the unrotated image.
fn source_boxes(views: &mut [Vec<Face>; 4], (width, height): (u32, u32)) {
    for (turn, faces) in views.iter_mut().enumerate() {
        for face in faces {
            let [x, y, w, h] = face.bounds;
            let (width, height) = (width as f32, height as f32);
            let [x, y, w, h] = match turn {
                1 => [y, height - x - w, h, w],
                2 => [width - x - w, height - y - h, w, h],
                3 => [width - y - h, x, h, w],
                _ => [x, y, w, h],
            };
            face.bounds = [x / width, y / height, w / width, h / height];
        }
    }
}

fn agrees_on_two_faces(full: &[Vec<Face>; 4], compact: &[Vec<Face>; 4], turn: usize) -> bool {
    // A person must favor the same turn over their own alternate detections at
    // both scales. Two tracks must use distinct boxes in both views.
    let favors = |face: &Face, views: &[Vec<Face>; 4]| {
        face.confidence >= 0.85
            && views
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != turn)
                .flat_map(|(_, faces)| faces)
                .filter(|other| overlap(face.bounds, other.bounds) >= 0.4)
                .all(|other| face.confidence - other.confidence >= 0.025)
    };
    if full[turn].len() < 2
        || compact[turn].len() < 2
        || compact.iter().enumerate().any(|(other, v)| {
            other != turn
                && v.first()
                    .is_some_and(|f| f.confidence > compact[turn][0].confidence)
        })
    {
        return false;
    }
    let mut first_match = None;
    for face in &full[turn] {
        if !favors(face, full) {
            continue;
        }
        let Some(partner) = compact[turn]
            .iter()
            .filter(|other| overlap(face.bounds, other.bounds) >= 0.5)
            .max_by(|a, b| {
                overlap(face.bounds, a.bounds).total_cmp(&overlap(face.bounds, b.bounds))
            })
        else {
            continue;
        };
        if !favors(partner, compact) {
            continue;
        }
        if let Some((first, small)) = first_match {
            if overlap(face.bounds, first) < 0.2 && overlap(partner.bounds, small) < 0.2 {
                return true;
            }
        } else {
            first_match = Some((face.bounds, partner.bounds));
        }
    }
    false
}

fn plausible(face: &Face) -> bool {
    let [right_eye, left_eye, nose, right_mouth, left_mouth] = face.landmarks;
    let eyes = [
        (right_eye[0] + left_eye[0]) * 0.5,
        (right_eye[1] + left_eye[1]) * 0.5,
    ];
    let mouth = [
        (right_mouth[0] + left_mouth[0]) * 0.5,
        (right_mouth[1] + left_mouth[1]) * 0.5,
    ];
    let dx = mouth[0] - eyes[0];
    let dy = mouth[1] - eyes[1];
    let eye_width = (left_eye[0] - right_eye[0]).abs();
    eye_width >= 5.
        && (left_eye[1] - right_eye[1]).abs() <= eye_width * 0.6
        && dy > 4.
        && dy / dx.hypot(dy).max(0.001) >= 0.85
        && nose[1] > eyes[1] - eye_width * 0.2
        && nose[1] < mouth[1] + eye_width * 0.2
}

fn decide(views: &[Vec<Face>; 4]) -> UprightGuess {
    let faces = views.each_ref().map(Vec::len);
    // A second face provides corroboration without letting a crowd of weak
    // detections overwhelm one clear face. The margin includes competing views.
    let scores = views.each_ref().map(|faces| {
        faces.first().map_or(0., |f| f.confidence) + faces.get(1).map_or(0., |f| f.confidence * 0.1)
    });
    let mut ranking = [0, 1, 2, 3];
    ranking.sort_by(|a, b| scores[*b].total_cmp(&scores[*a]));
    let best = ranking[0];
    let confident = views[best].first().is_some_and(|f| f.confidence >= 0.90);
    let clear = scores[best] - scores[ranking[1]] >= 0.08;
    let rotation = (confident && clear).then_some(best as u8);
    let reason = if rotation.is_some() {
        "Clear face orientation"
    } else if faces.iter().all(|n| *n == 0) {
        "No clear faces"
    } else {
        "Ambiguous face orientation"
    }
    .into();
    UprightGuess {
        rotation,
        scores,
        faces,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn face(confidence: f32) -> Face {
        Face {
            bounds: [0., 0., 100., 120.],
            landmarks: [[25., 30.], [75., 30.], [50., 55.], [35., 80.], [65., 80.]],
            confidence,
        }
    }
    #[test]
    fn rectangular_views_map_back_to_the_same_face() {
        let boxes = [
            [20., 10., 40., 30.],
            [60., 20., 30., 40.],
            [140., 60., 40., 30.],
            [10., 140., 30., 40.],
        ];
        let mut views = boxes.map(|bounds| {
            vec![Face {
                bounds,
                ..face(0.9)
            }]
        });
        source_boxes(&mut views, (200, 100));
        for view in views {
            assert!(overlap(view[0].bounds, [0.1, 0.1, 0.2, 0.3]) > 0.999);
        }
    }

    #[test]
    fn matching_needs_two_distinct_faces_that_agree_at_both_scales() {
        let first = Face {
            bounds: [0.1, 0.1, 0.3, 0.3],
            ..face(0.92)
        };
        let second = Face {
            bounds: [0.5, 0.2, 0.3, 0.3],
            ..face(0.91)
        };
        let views = [vec![first.clone(), second], vec![], vec![], vec![]];
        assert!(agrees_on_two_faces(&views, &views, 0));
        let mut retry = views.clone();
        retry[2].push(first.clone());
        assert!(!agrees_on_two_faces(&views, &retry, 0));
        retry[2].clear();
        retry[0][1] = first;
        assert!(!agrees_on_two_faces(&views, &retry, 0));
    }

    #[test]
    fn unclear_and_conflicting_evidence_does_not_rotate() {
        assert!(
            decide(&std::array::from_fn(|_| Vec::new()))
                .rotation
                .is_none()
        );
        assert!(
            decide(&[vec![face(0.95)], vec![], vec![face(0.92)], vec![]])
                .rotation
                .is_none()
        );
        assert!(
            decide(&[vec![], vec![face(0.84)], vec![], vec![]])
                .rotation
                .is_none()
        );
        assert_eq!(
            decide(&[vec![face(0.72)], vec![], vec![face(0.96)], vec![]]).rotation,
            Some(2)
        );
    }
    #[test]
    fn landmarks_reject_sideways_and_inverted_layouts() {
        let mut candidate = face(0.95);
        assert!(plausible(&candidate));
        for point in &mut candidate.landmarks {
            point[1] = 120. - point[1];
        }
        assert!(!plausible(&candidate));
        for point in &mut candidate.landmarks {
            point.swap(0, 1);
        }
        assert!(!plausible(&candidate));
    }
    #[test]
    fn bundled_model_runs_and_abstains_on_blank_input() {
        let detector = UprightDetector::new().unwrap();
        assert_eq!(detector.backend(), "CPU");
        assert!(detector.finish_profile().unwrap().is_none());
        let guess = detector
            .detect(&RgbImage::from_pixel(320, 240, image::Rgb([128; 3])))
            .unwrap();
        assert!(guess.rotation.is_none());
    }

    #[test]
    fn absent_gpu_runtime_returns_error_without_initializing_a_device() {
        assert!(
            UprightDetector::with_cuda(std::path::Path::new("/nonexistent/greign-onnxruntime.so"))
                .is_err()
        );
    }
}
