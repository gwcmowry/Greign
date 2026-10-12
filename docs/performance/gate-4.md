# Gate 4 render-quality baseline

Date: 2026-08-29

Hardware: Ryzen 9 7900X, 64 GB RAM, plus an NVIDIA GeForce RTX 5070 for the
explicit GPU parity suite. Public input is the CC0 Canon EOS R7 full RAW. The
camera-recommended output crop is 6960×4640.

The final release CPU quality JPEG render took 3.815 seconds and peaked at
976,276 KiB RSS. The Gate 2 bilinear reference was about 3.18 seconds, so the
edge-aware demosaic, highlight handling, and capture sharpening currently add
about 20% wall time. Full-frame normalized and RGB intermediates remain the
memory bottleneck and are the primary Gate 5 tiling target.

A correctness change initially computed a complete bilinear frame before
overwriting its interior with edge-aware output. That increased the same render
to 5.474 seconds. Computing bilinear values only for the physical border
reduced it to 3.815 seconds, a 30.3% improvement, while tiled/full identity and
border regressions remained green.

`cargo flamegraph` was attempted and saved no misleading partial profile:
Linux `perf_event_paranoid=4` denied sampling without `CAP_PERFMON`. Greign did
not change host security settings. Raw wall/RSS results are under ignored
`artifacts/gate4/`.

The Vulkan bilinear demosaic matches the CPU oracle for RGGB, BGGR, GRBG, and
GBRG at 17×33, including guarded workgroup boundaries. It is not yet the
end-to-end render path, so no full-render GPU speedup is claimed at this gate.
