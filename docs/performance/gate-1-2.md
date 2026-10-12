# Gate 1–2 performance baseline

Date: 2026-08-29

Hardware: Ryzen 9 7900X, 64 GB RAM. GPU was not used. Release profile uses
thin LTO and one codegen unit. Public input is the CC0 Canon EOS R7 full RAW
declared in `fixtures/manifests/canon-r7-public.json` (7128×4732 stored mosaic,
6982×4654 active area).

Raw measurements and profiler summaries are kept under ignored
`artifacts/gate1/` and `artifacts/gate2/`.

## RAW paths

The in-process 20-iteration distribution is saved as
`artifacts/gate1/r7-release-benchmark.json`:

| Path | p50 | p95 |
| --- | ---: | ---: |
| Metadata without mosaic | 1.32 ms | 1.47 ms |
| Direct encoded preview | 1.33 ms | 1.48 ms |
| Full mosaic decode | 170.43 ms | 191.90 ms |

Peak RSS during repeated full decode was 257,596 KiB. The isolated CLI metadata
command took about 46 ms before an in-process benchmark existed; syscall
tracing showed only 2.3 ms in syscalls, and the difference was process and
one-time decoder initialization. Debug metadata inspection took about 359 ms,
so debug results must not be used as an import performance estimate.

Direct CR3 JPEG extraction reduced isolated preview time from about 630 ms to
30 ms and peak RSS from about 228 MiB to 40 MiB by avoiding decode/re-encode.

## CPU reference render

The accepted full-resolution R7 JPEG render takes about 3.18 seconds and peaks
near 588 MiB RSS. The uncompressed 16-bit TIFF run takes about 2.75 seconds,
peaks near 650 MiB RSS, and writes 194,986,194 bytes.

The principal known memory costs are Rawler's decoded mosaic plus full-frame
`f32` normalized and RGB intermediates. These are acceptable for the Gate 2
correctness oracle but not for interactive previews; Gate 5 must tile and bound
those allocations.

## Profiler limitation

`cargo flamegraph` was attempted for the C-RAW digest, but Linux
`perf_event_paranoid=4` denied sampling without `CAP_PERFMON`. The project did
not change host security settings. Wall/RSS and summarized syscall profiles
were saved successfully.

