//! Optional dynamically loaded CUDA runtime; no GPU access from CPU constructors.
use anyhow::{Result, anyhow, ensure};
use ort::{
    execution_providers::{CUDAExecutionProvider, cuda::CuDNNConvAlgorithmSearch},
    session::Session,
    value::Tensor,
};
use std::{
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

pub(crate) struct CudaModel(Arc<Mutex<CudaSession>>);
struct CudaSession {
    session: Session,
    scene: Option<Session>,
    profiling: bool,
}
static SESSION: OnceLock<Result<Arc<Mutex<CudaSession>>, String>> = OnceLock::new();

fn cuda_builder() -> Result<ort::session::builder::SessionBuilder> {
    let provider = CUDAExecutionProvider::default()
        .with_tf32(false)
        .with_conv_algorithm_search(CuDNNConvAlgorithmSearch::Heuristic)
        .with_memory_limit(512 * 1024 * 1024)
        .build()
        .error_on_failure();
    Ok(Session::builder()?
        .with_intra_threads(1)?
        .with_inter_threads(1)?
        .with_execution_providers([provider])?)
}

impl CudaModel {
    pub(crate) fn new(library: &Path) -> Result<Self> {
        ensure!(
            library.is_file(),
            "ONNX Runtime library is missing: {}",
            library.display()
        );
        let session = SESSION
            .get_or_init(|| {
                // ort's dynamic loader can panic on invalid/incompatible libraries.
                // Optional acceleration must not take down the application.
                std::panic::catch_unwind(|| -> Result<Arc<Mutex<CudaSession>>> {
                    ort::init_from(library.to_string_lossy()).commit()?;
                    let mut builder = cuda_builder()?;
                    let profile = std::env::var_os("GREIGN_UPRIGHT_CUDA_PROFILE");
                    if let Some(path) = &profile {
                        builder = builder.with_profiling(path)?;
                    }
                    let mut session = builder.commit_from_memory(super::upright::MODEL)?;
                    // Trigger lazy CUDA/cuDNN kernel setup once, before publishing
                    // the cached session. The app can do this during scan loading.
                    let warmup =
                        Tensor::from_array(([1, 3, 640, 640], vec![0_f32; 3 * 640 * 640]))?;
                    let _ = session.run(ort::inputs!["input" => warmup])?;
                    Ok(Arc::new(Mutex::new(CudaSession {
                        session,
                        scene: None,
                        profiling: profile.is_some(),
                    })))
                })
                .map_err(|_| "ONNX Runtime could not be loaded".to_owned())?
                .map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(|e| anyhow!("{e}"))?;
        Ok(Self(session.clone()))
    }

    pub(crate) fn run(&self, input: Vec<f32>) -> Result<Vec<Vec<f32>>> {
        let mut model = self
            .0
            .lock()
            .map_err(|_| anyhow!("CUDA inference lock poisoned"))?;
        let input = Tensor::from_array(([1, 3, 640, 640], input))?;
        let outputs = model.session.run(ort::inputs!["input" => input])?;
        super::upright::OUTPUTS
            .iter()
            .map(|name| Ok(outputs[*name].try_extract_tensor::<f32>()?.1.to_vec()))
            .collect()
    }

    pub(crate) fn scene_logits(&self, input: Vec<f32>) -> Result<[f32; 4]> {
        use crate::scene_upright::{MODEL, SIZE};
        let mut model = self
            .0
            .lock()
            .map_err(|_| anyhow!("CUDA inference lock poisoned"))?;
        let scene = match &mut model.scene {
            Some(session) => session,
            slot => slot.insert(cuda_builder()?.commit_from_memory(MODEL)?),
        };
        let input = Tensor::from_array(([1, 3, SIZE, SIZE], input))?;
        let result = scene.run(ort::inputs![input])?;
        Ok(result[0].try_extract_tensor::<f32>()?.1.try_into()?)
    }

    pub(crate) fn finish_profile(&self) -> Result<Option<String>> {
        let mut model = self
            .0
            .lock()
            .map_err(|_| anyhow!("CUDA inference lock poisoned"))?;
        if !model.profiling {
            return Ok(None);
        }
        model.profiling = false;
        Ok(Some(model.session.end_profiling()?))
    }
}
