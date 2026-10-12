# Gate 3 GPU baseline

Date: 2026-08-29

Adapter: NVIDIA GeForce RTX 5070, Vulkan through `wgpu` 26.0.1. Timestamp
queries are supported. Reported maximum 2D texture dimension is 32768 and
maximum storage-buffer binding size is 2,147,483,647 bytes.

The release GPU suite is explicitly invoked and excluded from default CI:

```sh
cargo test --release -p greign-gpu --test gpu_parity -- --ignored --nocapture
```

All normalization, white-balance, exposure, signed-tone, and three-by-three
matrix kernels matched their CPU reference on 17×33 inputs, exercising guarded
non-multiple workgroups. Three consecutive adapter/device initializations
passed. The RGBA32F resident texture path retained an allocation until drop and
performed no CPU image readback.

For 20 GPU-resident 1920×1080 matrix dispatches, observed kernel p50 was
approximately 0.09–0.10 ms. Scalar CPU p50 was approximately 10–11 ms, a
roughly 100× or greater kernel-level improvement. Host wall time includes
command encoding, queue submission, synchronization, and reading the tiny
timestamp buffer; it is not used as the interactive kernel latency.

Raw test output is saved under ignored `artifacts/gate3/`.
