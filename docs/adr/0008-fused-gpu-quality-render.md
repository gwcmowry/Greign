# ADR 0008: Fused, resident GPU quality rendering

- Status: Accepted
- Date: 2026-08-30
- Supersedes: The fixed 960-pixel interaction resolution described in ADR 0006

## Context

The full Canon R7 quality render originally executed edge-aware demosaic,
highlight reconstruction, sharpening, color transforms, and tone mapping as
CPU-wide `f32` image passes. A 6960×4640 JPEG took 3.815 seconds and about
976 MiB RSS. A 960-pixel cached preview met the interaction budget, but was too
coarse for large displays and unnecessary on the main GPU.

The saved CPU flamegraph localized the named samples in edge-aware demosaic,
color-difference reconstruction, sharpening, and color conversion. These are
regular per-pixel or local-neighborhood operations whose intermediate images
do not need to cross the PCIe bus.

## Decision

Keep the CPU implementation as the numerical oracle, but make a four-dispatch
Vulkan compute path the primary quality renderer:

1. unpack the `u16` mosaic and fuse crop, normalization, clipped-sample repair,
   and white balance into an `R32Float` sensor texture;
2. fuse edge-aware green reconstruction and color-difference demosaic into an
   `Rgba32Float` camera-RGB texture;
3. fuse the camera-to-Rec.2020 matrix and highlight chroma repair into an
   `Rgba32Float` scene-linear working texture; and
4. fuse capture sharpening, exposure, signed tone mapping, Rec.2020-to-sRGB,
   the sRGB transfer function, and orientation into a directly presentable
   `Rgba8Unorm` texture.

The scene-linear working texture is the edit cache boundary. Exposure and the
other output-stage controls rerun only the final kernel. The interactive API
returns a GPU texture and performs no CPU readback. Export uses the same GPU
operators, then reads back compact RGB8 for a SIMD MozJPEG encode. CPU upload
packing and readback compaction use Rayon because profiling showed independent
row/sample work; the image-processing passes themselves remain on the GPU.

Choose preview pyramid level from physical viewport pixels and zoom. Settled
rendering uses the coarsest level still at least as large as the displayed
image; rapid interaction may use one level coarser. At 1:1 or closer, use the
original level. Full-resolution residency is therefore an optimization on the
main GPU, not a requirement for constrained devices; the scheduler can still
use viewport-sized tiled levels within its configured memory budget.

Use conservative explicit accounting against a 2 GiB GPU budget. Resource
leases are released with their resident textures, including error paths.

## Consequences

On the RTX 5070, a cached full-resolution R7 exposure update is ready in
2.46 ms p50 and 2.75 ms p95. Preparing the full scene-linear cache takes
65.56 ms after decode/device initialization. Peak accounted GPU allocation is
1.62 GB, so it fits the current 2 GiB contract but leaves insufficient room
for several simultaneous full-resolution frames; those use LOD/tiling.

A cold RAW-to-JPEG command now takes 0.773 seconds, about 4.9 times faster than
the CPU quality baseline. Its remaining latency is primarily RAW decode,
one-time GPU initialization/resource creation, GPU readback, and JPEG encode,
not shader execution. Batch export can parallelize independent encodes across
CPU cores, but a single baseline JPEG entropy stream is not divided among
cores in this implementation.

The GPU final image remains covered by byte-tolerant CPU differential tests,
orientation coverage, direct-residency tests, memory-release tests, and real
Canon R7 visual inspection.
