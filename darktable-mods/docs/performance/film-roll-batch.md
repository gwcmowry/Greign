# Film-roll batch timing

## Scope

These measurements cover the native lighttable roll workflow: load a float
preview, process the user's pixelpipe up to Negadoctor, analyze a 1600-pixel
proxy, aggregate the roll, and write normal Negadoctor history. They do not
include later full-resolution exports.

The benchmark used the CPU-only RelWithDebInfo darktable 5.6 fork on the Ryzen
9 7900X. Two public color-negative RAW files were exposed under unique catalog
paths to model cold per-image processing. Every run used a fresh in-memory
library and cache. The repeated source content makes this a throughput test,
not color-correctness evidence.

Raw `/usr/bin/time -v` reports and per-frame timings are retained under
`artifacts/profiles/darktable-roll-preview-{1,4,12,36}.{time,log}`.

| Images | Internal workflow | Analysis | History | External wall time | Peak RSS |
|---:|---:|---:|---:|---:|---:|
| 1 | 0.258 s | 0.255 s | 0.003 s | 0.70 s | 259 MiB |
| 4 | 1.073 s | 1.064 s | 0.009 s | 1.51 s | 405 MiB |
| 12 | 2.929 s | 2.903 s | 0.026 s | 3.40 s | 786 MiB |
| 36 | 13.735 s | 9.026 s | 4.709 s | 14.20 s | 834 MiB |

For small and medium selections, proxy analysis costs about 0.24–0.27 seconds
per image and history is negligible. At 36 imported images, darktable's image
metadata cache is saturated in this isolated harness, making each cold history
load about 0.13 seconds. That produces a conservative estimate of roughly 14
seconds for a 36-exposure roll on this CPU, before full-resolution export.

## Bottleneck and implementation choice

An earlier version requested full-resolution float mipmaps and then scaled them
down. At 36 frames it used about 1.27 GiB RSS and spent 12.4 seconds in analysis.
The integrated job now requests darktable's native float preview mipmap and
evicts it after measurement. Analysis fell to 9.0 seconds and peak RSS to about
834 MiB. The density histograms and roll aggregation themselves are tiny; RAW
decode, upstream pixelpipe modules, and cold history loading dominate.

The job is intentionally sequential at this correctness stage. Individual
pixelpipes already use the CPU's available threads—the 36-frame run consumed
77 seconds of CPU time in 14.2 seconds wall time—so launching several complete
pixelpipes concurrently would increase memory pressure and is not yet justified.
