//! Correct CPU fallback after GPU failures.

/// Backend which produced a render result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionBackend {
    /// GPU operation completed successfully.
    Gpu,
    /// GPU failed or was unavailable and CPU produced the result.
    CpuFallback,
}

/// Executes a GPU operation, falling back to the reference CPU closure on any
/// GPU error.
///
/// The GPU error is returned beside a successful CPU result for diagnostics.
///
/// # Errors
///
/// Returns the CPU error only when the GPU failed and the CPU fallback also
/// failed. GPU errors alone do not make this function fail.
pub fn execute_with_cpu_fallback<T, GpuError, CpuError>(
    gpu: impl FnOnce() -> Result<T, GpuError>,
    cpu: impl FnOnce() -> Result<T, CpuError>,
) -> Result<(T, ExecutionBackend, Option<GpuError>), CpuError> {
    match gpu() {
        Ok(result) => Ok((result, ExecutionBackend::Gpu, None)),
        Err(gpu_error) => {
            cpu().map(|result| (result, ExecutionBackend::CpuFallback, Some(gpu_error)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ExecutionBackend, execute_with_cpu_fallback};

    #[test]
    fn gpu_success_does_not_run_cpu() {
        let (value, backend, gpu_error) = execute_with_cpu_fallback(
            || Ok::<_, &str>(7),
            || -> Result<i32, &str> { panic!("CPU must not run") },
        )
        .expect("result");
        assert_eq!(value, 7);
        assert_eq!(backend, ExecutionBackend::Gpu);
        assert!(gpu_error.is_none());
    }

    #[test]
    fn device_loss_produces_correct_cpu_result() {
        let (value, backend, gpu_error) =
            execute_with_cpu_fallback(|| Err::<u32, _>("device lost"), || Ok::<_, &str>(42))
                .expect("CPU result");
        assert_eq!(value, 42);
        assert_eq!(backend, ExecutionBackend::CpuFallback);
        assert_eq!(gpu_error, Some("device lost"));
    }
}
