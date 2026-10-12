# Gate 5 progressive-render baseline

Date: 2026-08-30

Hardware: Ryzen 9 7900X, 64 GB RAM. Release build, CPU progressive path, CC0
Canon EOS R7 full RAW. The benchmark command was:

```sh
greign raw benchmark fixtures/downloads/canon-eos-r7-raw.cr3 \
  --iterations 30 --preview-render --preview-dimension 960
```

The camera crop reduces by an even Bayer factor to an 870×580 prepared preview
occupying 6,055,200 pixel bytes.

| Path | p50 | p95 |
| --- | ---: | ---: |
| Metadata fast path | 1.17 ms | 1.36 ms |
| Embedded preview fast path | 1.20 ms | 1.33 ms |
| Cached exposure/tone preview update | 15.31 ms | 17.19 ms |

One-time decode, normalize, crop, Bayer reduction, demosaic, and color
preparation took 539.32 ms and ran as refinement/background preparation. Peak
RSS was 357,524 KiB, versus about 976 MiB for the full-resolution Gate 4
quality render. An embedded camera preview remains available before this work.

The 50 ms main-system slider budget passes with roughly 2.9× p95 headroom.
Hard pending/in-flight bounds, oversized-entry refusal, exact LRU accounting,
and 1,000 rapid edit/cancel/switch iterations cover constrained configurations
without allocation growth. The 2 GB GPU allocation contract remains covered
by the Gate 3 resource-budget suite.

Raw benchmark JSON is saved at ignored
`artifacts/gate5-r7-preview-benchmark.json`.
