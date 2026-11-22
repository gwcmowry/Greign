//! Whole-picture quarter-turn verification for unresolved face-based suggestions.
use anyhow::{Result, anyhow, ensure};
use image::{RgbImage, imageops};
use rayon::prelude::*;
use std::sync::OnceLock;
use tract_onnx::prelude::*;

pub(crate) const MODEL: &[u8] = include_bytes!("../models/scene-orientation-v2.onnx");
pub(crate) const SIZE: usize = 384;

pub(crate) fn cpu_logits(input: Vec<f32>) -> Result<[f32; 4]> {
    static MODEL_PLAN: OnceLock<Result<TypedRunnableModel<TypedModel>, String>> = OnceLock::new();
    let model = MODEL_PLAN
        .get_or_init(|| {
            let load = || -> Result<_> {
                let mut model =
                    tract_onnx::onnx().model_for_read(&mut std::io::Cursor::new(MODEL))?;
                model.set_input_fact(0, f32::fact([1, 3, SIZE, SIZE]).into())?;
                model.into_optimized()?.into_runnable()
            };
            load().map_err(|e| format!("{e:#}"))
        })
        .as_ref()
        .map_err(|e| anyhow!("{e}"))?;
    let input: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, SIZE, SIZE), input)?.into();
    let outputs = model.run(tvec!(input.into()))?;
    Ok(outputs[0].as_slice::<f32>()?.try_into()?)
}

pub(crate) fn verify(
    image: &RgbImage,
    workers: Option<&rayon::ThreadPool>,
    infer: impl Fn(Vec<f32>) -> Result<[f32; 4]> + Sync,
) -> Result<Option<u8>> {
    // Reference preprocessing: resize to a square, center crop, RGB ImageNet normalization.
    let resized = imageops::resize(image, 416, 416, imageops::FilterType::Triangle);
    let square = imageops::crop_imm(&resized, 16, 16, SIZE as u32, SIZE as u32).to_image();
    let evaluate = |turn: u8| -> Result<Option<u8>> {
        let view = match turn {
            1 => imageops::rotate270(&square),
            2 => imageops::rotate180(&square),
            3 => imageops::rotate90(&square),
            _ => square.clone(),
        };
        let mut input = vec![0.; 3 * SIZE * SIZE];
        for (i, pixel) in view.pixels().enumerate() {
            for c in 0..3 {
                input[c * SIZE * SIZE + i] = (f32::from(pixel[c]) / 255.
                    - [0.485, 0.456, 0.406][c])
                    / [0.229, 0.224, 0.225][c];
            }
        }
        let scores = infer(input)?;
        ensure!(
            scores.iter().all(|x| x.is_finite()),
            "Invalid scene orientation output"
        );
        let best = (0..4)
            .max_by(|&a, &b| scores[a].total_cmp(&scores[b]))
            .unwrap();
        let confidence = 1. / scores.iter().map(|s| (s - scores[best]).exp()).sum::<f32>();
        // Counterclockwise input view, clockwise model correction.
        Ok((confidence >= 0.90).then_some((best as u8 + 4 - turn) % 4))
    };
    if let Some(workers) = workers {
        let mut votes = [None; 4];
        workers.install(|| {
            votes
                .par_iter_mut()
                .enumerate()
                .try_for_each(|(i, vote)| -> Result<()> {
                    *vote = evaluate(i as u8)?;
                    Ok(())
                })
        })?;
        Ok(consensus(votes))
    } else {
        let first = evaluate(0)?;
        for turn in 1..4 {
            if first.is_none() || evaluate(turn)? != first {
                return Ok(None);
            }
        }
        Ok(first)
    }
}

fn consensus(votes: [Option<u8>; 4]) -> Option<u8> {
    if votes.iter().all(|v| *v == votes[0]) {
        votes[0]
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requires_confident_agreement_in_all_views() {
        assert_eq!(consensus([Some(3); 4]), Some(3));
        assert_eq!(consensus([Some(3), Some(3), Some(3), Some(1)]), None);
        assert_eq!(consensus([Some(3), Some(3), None, Some(3)]), None);
    }
}
