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
        let mut views: [Vec<Face>; 4] = std::array::from_fn(|_| Vec::new());
        let evaluate = |turn: usize| -> Result<Vec<Face>> {
            let rotated = match turn {
                1 => imageops::rotate90(&small),
                2 => imageops::rotate180(&small),
                3 => imageops::rotate270(&small),
                _ => small.clone(),
            };
            self.faces(&rotated)
        };
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
        Ok(decide(&views))
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
