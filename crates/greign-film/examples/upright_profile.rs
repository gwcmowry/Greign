//! Node-level CPU profiling when kernel perf sampling is unavailable.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use anyhow::Result;
use std::{io::Write, path::PathBuf, time::Instant};
use tract_onnx::prelude::*;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let image = PathBuf::from(args.next().expect("positive input image"));
    let output = PathBuf::from(args.next().expect("profile JSONL path"));
    // The bundled ONNX graph has fixed intermediate shape annotations.
    let size = 640;
    let mut model = tract_onnx::onnx().model_for_read(&mut std::io::Cursor::new(
        include_bytes!("../models/yunet-2023mar.onnx"),
    ))?;
    model.set_input_fact(0, f32::fact([1, 3, size, size]).into())?;
    let model = model.into_optimized()?.into_runnable()?;
    let image = image::open(image)?.to_rgb8();
    let scale = size as f32 / image.width().max(image.height()) as f32;
    let image = image::imageops::resize(
        &image,
        (image.width() as f32 * scale).round() as u32,
        (image.height() as f32 * scale).round() as u32,
        image::imageops::FilterType::Triangle,
    );
    let input: Tensor = tract_ndarray::Array4::from_shape_fn((1, 3, size, size), |(_, c, y, x)| {
        if x < image.width() as usize && y < image.height() as usize {
            f32::from(image.get_pixel(x as u32, y as u32)[2 - c])
        } else {
            0.
        }
    })
    .into();
    let mut log = std::fs::File::create(output)?;
    for repeat in 0..12 {
        let start = Instant::now();
        let mut state = SimpleState::new(&model)?;
        state.run_plan_with_eval(tvec!(input.clone().into()),|session,state,node,inputs|->TractResult<_>{
            let start=Instant::now();
            let result=tract_onnx::tract_core::plan::eval(session,state,node,inputs)?;
            let ns=start.elapsed().as_nanos();
            writeln!(log,"{}",serde_json::json!({"repeat":repeat,"node":node.name,"op":node.op.name(),"ns":ns}))?;
            Ok(result)
        })?;
        writeln!(
            log,
            "{}",
            serde_json::json!({"repeat":repeat,"stage":"total","ms":start.elapsed().as_secs_f64()*1000.})
        )?;
    }
    Ok(())
}
