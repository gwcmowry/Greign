# Darktable-based film workflow direction

Research date: 2026-08-31.

## Recommendation

Fork darktable and build a film-scan workspace around its existing RAW,
catalog, tethering, color-management, pixelpipe, masking, export, and OpenCL
infrastructure. Keep Lua for optional workflow glue, not as the core product.

Do not begin by rewriting `negadoctor`. Its density-domain inversion and
virtual-paper model are a sound base, its CPU loop is already vectorized and
parallelized, and it already has an OpenCL kernel. The main product gap is the
analysis and workflow surrounding that per-image transform.

Base the fork on the current stable `release-5.6.0` tag, retain darktable's
GPL-3.0-or-later licensing and notices, and keep a clean upstream remote. Avoid
building on a nightly database schema for users' real catalogs.

## Why Lua is not enough

Lua can add actions, panels, import/export hooks, metadata helpers, styles, and
batch orchestration. It cannot provide all of the following as a coherent,
supported workflow:

- a new pixelpipe operation or changes to negadoctor's parameters and math;
- full-resolution image statistics shared across a roll;
- processing-aware overlays, density scopes, or a live conversion workspace;
- new RAW/scanner ingest paths, IR planes, or tightly integrated tethering;
- OpenCL kernels, cache behavior, or pixelpipe scheduling changes;
- durable, versioned roll-analysis state integrated with history and sidecars.

A Lua proof of concept can still validate terminology and button flow while the
native implementation is built.

## What users value in Negative Lab Pro that negadoctor lacks

This table distinguishes a real engine/workflow gap from functionality that
darktable has elsewhere but does not present as one film workflow.

| Capability people value | Negative Lab Pro | negadoctor / darktable today | Product implication |
|---|---|---|---|
| Fast automatic starting point | Analyzes each negative, sets channel-specific conversion values, and exposes positive-sense controls | negadoctor has useful pickers, but its documented workflow requires several ordered, manual area selections and histogram checks | One **Convert** action should automatically detect the frame, base, density range, and safe tone endpoints, while showing confidence and the values it chose |
| Roll-aware consistency | Roll Analysis uses the whole physical roll to reduce scene dependence and offers Pro Lab, Basic Lab, and Darkroom Paper processes | No roll-level statistical model; copied negadoctor values are fixed settings, not shared analysis with per-frame adaptation | Make a roll a first-class analysis object and separate roll baseline from per-frame corrections |
| Exact reference matching | Match copies both analysis and edit settings; Sync Settings copies only creative settings | styles/history can copy module parameters, but there is no explicit distinction between analysis state and creative edits | Provide **Match reference**, **Sync look**, and **Re-analyze frame** as different commands |
| Automatic color balancing | Auto-Neutral, Auto-Warm, Auto-Cool, Auto-AVG plus fixed Kodak/Fuji/Cine starting points | manual highlight/shadow neutral pickers; other darktable color modules are powerful but not negative-aware | Add robust neutral inference with intent-preserving variants and never force a dominant scene color to gray |
| Familiar lab looks | Basic, Frontier, and Noritsu color models; named tone profiles and LUTs | negadoctor simulates generic paper; darktable LUTs/styles can approximate looks but are not coupled to negative analysis | Ship open, versioned **render intents**, not unverifiable claims of exact proprietary scanner emulation |
| Unified tonal controls | Exposure, brightness, contrast, blacks, whites, Lab Glow/Fade, black/white clipping, and soft highs/lows | similar results are possible across negadoctor and other modules, but operation order and negative-vs-positive semantics are easy to get wrong | Present a film-specific quick panel backed by correctly ordered native modules; retain full darktable controls in an advanced view |
| Batch navigation and editing | Convert a selection, move through it in one panel, sync selected settings, undo/unconvert | darktable has lighttable batch tools and history, but no film conversion session | Add a contact-sheet-centric session with background analysis, fast keyboard culling, reference pinning, and batch exceptions |
| Negative-specific camera profiles | Hundreds of custom camera profiles intended to avoid positive-photo look curves corrupting inversion | darktable has strong input profiles but no guided negative-capture calibration/profile selection | Default to a documented linear input path and add an open calibration builder for camera + light + film |
| Film-specific metadata | Analog camera/lens, film, rating, format, development, digitization setup, automatic captions and export mapping | generic metadata and tags exist, but no film schema or distinction between taking camera and digitizing camera | Add a portable film metadata namespace while preserving the scanner-camera EXIF needed for RAW decoding |
| B&W-specific defaults | One-step B&W mode, photographic-paper gamma starting point, toning, and sharpening choices | negadoctor has a B&W stock switch, but users still report color casts and must combine other modules for true monochrome | Make monochrome output explicit, preserve optional paper tone, and offer grain-aware detail defaults |
| Positive/slide handling | Positive and Edit Only modes extend the same panel to slide film and prior conversions | darktable already develops positive RAWs well, but not through a unified film session | Treat negative, B&W, slide, and already-positive scans as ingest intents within one workspace |

The strongest repeated user feedback is not that negadoctor's inversion is
incapable. It is that negadoctor is manual, intimidating, and slow to apply
consistently across a roll. Users praise Negative Lab Pro for reliable initial
color, batch conversion, and editing the inverted RAW with controls that behave
as expected. At the same time, NLP users still report crop-dependent analysis,
color casts, dense-negative failures, and inconsistent roll results. Those
failures define a useful opening: provide automation without hiding or losing
the measured analysis state.

## Features that can be better than Negative Lab Pro

### 1. Explainable, editable roll analysis

Persist the measurements, not just the result:

- film-base RGB and spatial-uniformity map;
- robust low/mid/high density quantiles per channel;
- roll baseline and per-frame deltas for luma and chroma separately;
- frames rejected as outliers and the reason;
- analysis crop/mask, algorithm version, and confidence;
- chosen render intent and all derived negadoctor parameters.

Let the user edit, lock, copy, or re-run those values. This makes a conversion
reproducible when the original roll is offline and avoids “change the crop and
hope” behavior.

### 2. Lighting-aware roll groups

A physical roll does not imply one scene illuminant. Analyze the entire roll,
then automatically suggest subgroups such as daylight, shade, tungsten, and
night while sharing only film/scanner properties globally. Use robust
statistics and down-weight blank frames, light leaks, extreme density, and
single-color scenes. The user can merge/split groups or lock a reference.

### 3. Automated frame and film-base geometry

Detect the image rectangle, rebate, sprockets, holder, and direct-light holes.
Use separate masks for:

- conversion analysis (image area only);
- film-base estimation (clean rebate only);
- final crop (user's aesthetic choice, which may include borders).

This removes crop-dependent analysis and lets a user change the final crop
without silently changing the conversion.

### 4. Capture-session workspace

Build on darktable tethering with a film-specific live view:

- alignment and perpendicularity guides;
- focus map and corner-vs-center sharpness;
- per-channel ETTR/clipping warning on the actual RAW mosaic;
- capture naming by roll and frame number;
- unchanged camera/light-setting guardrails;
- automatic next-frame detection and duplicate/missing-frame warnings;
- optional light-source control and RGB sequential capture.

The ideal workflow begins before import: feed film, trigger, and see a converted
preview immediately while the full-resolution result refines in the background.

### 5. Scan-system calibration and correction

- flat-field capture for light falloff, sensor dust, and lens shading;
- flare/reflection and uneven-illumination diagnostics;
- open camera + light + film calibration from a target and documented spectral
  data where available;
- bracketed capture/HDR merge for exceptionally dense film;
- RGB sequential capture alignment and crosstalk correction;
- scanner RGB+IR ingest and an IR-derived dust/scratch mask;
- calibration validity warnings when camera, lens, aperture, light, or exposure
  no longer match the session.

### 6. Film-native scopes and retouching

Show both the negative measurement and positive rendering:

- density histogram and per-channel headroom;
- H&D curve with toe/straight-line/shoulder placement;
- zone strip and clipping counts;
- spot densitometer reading density above film base and rendered value;
- dust/scratch detection that is stable across later tonal edits;
- grain-aware sharpening, denoising, and defect repair that do not erase the
  film texture by default.

### 7. Contact sheet as the primary editing surface

Make consistency visible. A user should be able to pin two reference frames,
compare a whole roll, adjust a roll baseline, select lighting subgroups, and
spot outliers without opening 36 separate darkroom sessions. Render a printable
contact sheet with frame IDs and film metadata.

### 8. Reproducible evaluation

For every conversion algorithm, record:

- exact source hashes, analysis masks, parameters, and algorithm version;
- render hashes and perceptual metrics against approved references;
- cross-frame neutral/color consistency metrics;
- performance distributions for import, first preview, roll analysis, slider
  latency, export, peak RAM, and peak VRAM;
- human-approved reference variants where color intent is ambiguous.

## Native fork design

Keep the fork delta modular even though it compiles inside darktable:

```text
darktable RAW decode / input profile
              |
              +--> film-session analyzer (background, reduced resolution)
              |       |
              |       +--> roll baseline + lighting groups + frame deltas
              |       +--> geometry/base masks + confidence + QA warnings
              |                         |
              +-------------------------+--> versioned derived parameters
                                                 |
                                         negadoctor pixel transform
                                                 |
                                         normal darktable modules
                                                 |
                                  preview / contact sheet / export
```

Suggested ownership boundaries:

- `src/common/film_session.*`: immutable analysis records, roll/group/frame
  relationships, serialization, cache keys, and background jobs;
- `src/common/film_analysis.*`: pure statistics and geometry with headless
  tests; no GTK dependencies;
- `src/iop/negadoctor.c` and its OpenCL kernel: only the per-pixel transform and
  parameters actually needed by the pixelpipe;
- `src/libs/film_scan.*`: lighttable/contact-sheet workflow and batch actions;
- a darkroom quick-access panel for positive-sense film controls and scopes;
- database migration plus XMP-compatible namespaced metadata for durable roll
  state and derived frame parameters;
- Lua hooks for user automation after the native commands exist.

Do not make the per-pixel module reach into the catalog or other images. The
analyzer should compute immutable, versioned parameters and commit ordinary
per-image history entries. That keeps exports deterministic, preserves
darktable's cache model, and allows a frame to render when the rest of its roll
is offline.

## Performance approach

The negadoctor pixel kernel is simple and already has CPU and OpenCL paths, so
it is unlikely to be the first bottleneck. Measure before changing it.

1. Analyze a small linear proxy for every frame and cache sufficient statistics.
2. Decode/analyze frames concurrently through darktable's existing job system,
   with cancellation and bounded memory.
3. Recompute roll aggregation from cached frame statistics rather than pixels.
4. Keep masks and histograms compact; do not retain one full float image per
   frame.
5. Only then profile native rendering with saved flamegraphs/OpenCL timings and
   consider kernel fusion or layout changes.

## Ordered implementation

1. **Darktable fork spike:** pin 5.6.0, isolate config/catalog paths, build and
   run the three-film corpus, and establish reproducible CPU results.
2. **One-click single-frame conversion:** automatic geometry, film base,
   density range, positive preview, confidence, and editable measurements.
3. **Roll model:** persistent roll object, luma/chroma baselines, outlier
   handling, Match/Sync/Re-analyze semantics, and contact-sheet review.
4. **Film metadata and presets:** portable schema, render intents, import/export,
   and preset sharing.
5. **Capture QA:** tethered workspace, flat fields, focus/alignment, RAW channel
   clipping, and naming/frame progression.
6. **Advanced restoration:** IR dust, bracket merge, RGB sequential capture,
   density scopes, and grain-aware repair.

## Sources

- [darktable negadoctor manual](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/negadoctor/)
- [darktable negadoctor source](https://github.com/darktable-org/darktable/blob/master/src/iop/negadoctor.c)
- [darktable image-operation module API](https://github.com/darktable-org/darktable/blob/master/dev-doc/IOP_Module_API.md)
- [darktable 5.6.0 release](https://github.com/darktable-org/darktable/releases/tag/release-5.6.0)
- [Negative Lab Pro basic workflow and controls](https://www.negativelabpro.com/guide/basics/)
- [Negative Lab Pro roll analysis and presets](https://forums.negativelabpro.com/t/negative-lab-pro-v3-0-the-next-level-of-film-processing/6429)
- [Negative Lab Pro 3.1 Match and color processing](https://forums.negativelabpro.com/t/negative-lab-pro-v3-1-new-color-processing-support-for-slide-film-improved-batch-editing-and-more/8492)
- [Negative Lab Pro batch editing](https://www.negativelabpro.com/guide/batch/)
- [Negative Lab Pro film metadata](https://www.negativelabpro.com/guide/film-metadata/)
- [Negative Lab Pro B&W workflow](https://www.negativelabpro.com/guide/blackwhite/)
- [Community report: negadoctor is capable but very manual](https://www.reddit.com/r/AnalogCommunity/comments/1i8hep7/alternative_to_negative_lab_pro_that_doesnt_need/)
- [Community report: manual per-frame negadoctor conversion takes hours](https://www.reddit.com/r/AnalogCommunity/comments/1tar270/help_with_batch_negative_converting_in_darktable/)
- [NLP request for editable analysis values and reproducibility](https://forums.negativelabpro.com/t/make-analysis-results-editable/10149)
- [NLP report of crop-dependent analysis](https://forums.negativelabpro.com/t/anomalous-behavior-on-cropped-photos/9647)
- [NLP report of inconsistent roll results](https://forums.negativelabpro.com/t/inconsistent-with-negative-conversion-and-roll-analysis-on/9738)
- [NegPy changelog, useful prior art for open film workflows](https://github.com/marcinz606/NegPy/blob/main/docs/CHANGELOG.md)
