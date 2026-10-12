# Pre-UI fused GPU render benchmark

Date: 2026-08-30

Hardware: Ryzen 9 7900X, 64 GB RAM, NVIDIA GeForce RTX 5070 12 GB, Vulkan.
Release build. Input: public CC0 Canon EOS R7 full RAW; camera crop 6960×4640.

## Before profile

The CPU quality JPEG path took 3.815 seconds and peaked at 976,276 KiB RSS.
The saved `cargo flamegraph` artifacts are
`artifacts/perf/pre-gpu-full-render.{svg,log}`. Sampling lost 21 chunks and a
large unknown bucket, so the named percentages are prioritization evidence,
not an exhaustive accounting:

| Named stack | Samples |
| --- | ---: |
| Edge-aware demosaic | 22.99% |
| Color-difference reconstruction | 12.45% |
| Capture sharpening | 11.94% |
| Rec.2020 to sRGB | 8.16% |
| `powf` within display conversion | 6.74% |
| JPEG encoder core | 4.05% |

The result justified moving the repeated full-frame numerical passes to fused
GPU kernels before adding more CPU parallelism.

## Cold end-to-end export

Command:

```sh
./target/release/greign raw render \
  fixtures/downloads/canon-eos-r7-raw.cr3 /tmp/greign-r7-gpu-final.jpg \
  --quality --gpu --jpeg-quality 92
```

| Stage | Time |
| --- | ---: |
| RAW decode | 237.00 ms |
| GPU initialization | 116.87 ms |
| GPU render, readback, RGB packing | 106.93 ms |
| MozJPEG encode | 311.82 ms |
| End to end | **772.63 ms** |

This is a 4.94× wall-time speedup over the 3.815-second CPU quality baseline.
Peak RSS is 644,100 KiB and peak conservatively accounted GPU memory is
1,488,714,048 bytes. The quality-92 output is a 6960×4640, 3.3 MiB baseline
JPEG with an embedded sRGB ICC profile. Raw JSON is saved as
`artifacts/gpu-cold-export.json`.

Greign now defaults to JPEG quality 95. A matching quality-95 sample encoded
in 304.72 ms and produced 5,259,947 bytes, compared with 3,392,352 bytes at
quality 92. The encode-time difference is within single-run noise, while the
55.1% file-size increase is material. Its raw cold-start result is saved as
`artifacts/gpu-cold-export-q95.json`; the higher 898.22 ms total in that sample
came from variable GPU initialization (252.33 ms), not JPEG quality.

The timed GPU kernels total 7.76 ms:

| Work | Time |
| --- | ---: |
| Sensor fusion | 0.41 ms |
| Edge-aware demosaic fusion | 4.34 ms |
| Working-space color/highlight | 1.79 ms |
| Output fusion | 1.22 ms |

CPU upload packing is 4.98 ms and readback RGB packing is 7.53 ms after Rayon
parallelization. Host texture/bind-group setup is 61.26 ms and submission,
execution, timestamp mapping, and image mapping are 12.12 ms. These costs occur
on the export/readback path; the interactive path presents the GPU texture
directly.

## Cached full-resolution editing

After one RAW decode and one scene-linear GPU preparation, 30 exposure edits
gave these distributions:

| Metric | p50 | p95 |
| --- | ---: | ---: |
| Host submit | 1.12 ms | 1.20 ms |
| Full-resolution texture ready | **2.46 ms** | **2.75 ms** |

Full-resolution cache preparation is 65.56 ms. Peak accounted GPU memory with
the prepared and output textures resident is 1,617,891,648 bytes; peak RSS is
451,968 KiB. The raw distribution is saved at
`artifacts/gpu-full-edit-benchmark.json`.

The editor should still choose resolution from viewport pixels and zoom: fit
views use a pyramid level close to display resolution, while 1:1 views request
original pixels and only visible tiles need residency on constrained devices.
On the main GPU the complete R7 working image fits, and cached full-resolution
edits already beat the 50 ms interaction budget by roughly 18× at p95.

## Remaining bottlenecks

For interactive viewing, no JPEG encode or GPU readback is needed; the final
kernel and presentation are the material work. For a single cold export, the
largest remaining stages are MozJPEG (40.4%), RAW decode (30.7%), GPU startup
(15.1%), and render/readback/resource work (13.8%). MozJPEG replaced the scalar
reference encoder and is SIMD optimized, but one image is not divided across
CPU cores. A single baseline JPEG remains one entropy stream; Greign will
parallelize batch exports across images rather than add a nonstandard or
licensing-constrained single-image codec. This also scales naturally with
independent GPU render/readback slots and preserves ordinary JPEG
compatibility. Reusing device and resource state removes the cold-start
component in the desktop process.
