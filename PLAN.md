# Greign implementation plan

Greign is a free, open-source, Linux-first, GPU-accelerated, non-destructive
RAW developer and photographic library. Its defining workflow is fast,
roll-aware conversion of camera-scanned film negatives. Version 1 is built as
a local application without accounts, cloud dependencies, generative AI, or a
subscription.

> **Project separation (2026-09-05):** This plan describes the original
> standalone Greign editor. The darktable fork explored from 2026-08-31 onward
> is now a separate project under [darktable-mods/](darktable-mods/README.md),
> including its [film workflow direction](darktable-mods/docs/research/darktable-film-direction.md).

This plan deliberately contains no calendar estimates. Work proceeds through
ordered validation gates. A gate is complete only when its tests and exit
criteria pass; development can move through those gates as quickly as the
implementation allows.

## Product principles

1. Never modify an imported photograph.
2. Make the common path feel immediate, including on older hardware.
3. Maintain a correct CPU reference path even when the GPU path is primary.
4. Measure performance. Do not justify complexity with intuition alone.
5. Prefer atomic, cohesive crates over thin layers and wrapper proliferation.
6. Treat color correctness, catalog durability, and malformed input as safety
   properties, not polish work.
7. Use progressive rendering: show a useful result quickly, then refine it.
8. Keep files and edits usable without a Greign account or hosted service.
9. Optimize algorithm and memory traffic before adding parallelism.
10. Do not introduce a public plug-in ABI until the processing model is stable.

## Version 1 scope

### Required workflows

- Import referenced files in place or copy them into a selected destination.
- Read common RAW formats, prioritizing Canon R7 CR3 and then CR2, NEF, ARW,
  RAF, RW2, ORF, and DNG.
- Extract embedded previews and metadata without decoding the full mosaic.
- Browse a large folder or catalog using a responsive grid and filmstrip.
- Rate, flag, label, keyword, filter, sort, stack, and create collections.
- Apply non-destructive global edits, crop/rotate, lens correction, and local
  gradient/brush masks.
- Create virtual copies without duplicating source pixels.
- Undo and redo edits and recover cleanly after a crash.
- Copy/paste edits and batch-apply presets.
- Export color-managed JPEG and 16-bit TIFF, with resizing and output
  sharpening.
- Represent a film roll as a first-class catalog object.
- Convert an individual negative or a complete roll with a simple default
  action, shared roll analysis, and per-frame overrides.

### Core develop controls

- Camera and input profile selection
- White balance and tint
- Exposure and highlight reconstruction
- Black/white points, contrast, and a scene-to-display tone curve
- Shadows/highlights and highlight rolloff
- Parametric RGB and luminance curves
- Saturation, vibrance, and per-hue adjustments
- Crop, straighten, orientation, and perspective
- Lens distortion, vignetting, and lateral chromatic-aberration correction
- Denoising, capture sharpening, and output sharpening
- Histogram, clipping warnings, before/after, and zoomable full-resolution view

### Explicitly outside version 1

- Generative AI and semantic object replacement
- Cloud storage or cross-device synchronization
- Mobile or browser clients
- Face recognition
- Tethered capture
- Book, web-gallery, or print-layout designers
- Photoshop-style layers and arbitrary painting
- Public binary plug-in compatibility
- Bit-for-bit rendering compatibility with Lightroom
- Importing proprietary Lightroom adjustment semantics

## System architecture

The dependency direction is inward toward domain types. The UI, decoders,
storage, and GPU are adapters around a processing model that can be tested
headlessly.

```text
immutable source
      |
      +--> metadata / embedded-preview fast path --> catalog and thumbnail cache
      |
      +--> RAW decoder --> normalized sensor representation
                              |
versioned edit recipe --------+--> processing graph
                                      |          |
                               CPU reference   GPU executor
                                      |          |
                                      +----+-----+
                                           |
                                 preview / histogram / export

catalog <--> XMP sidecar <--> metadata, edit recipe, virtual copies, roll state
```

### Proposed Rust workspace

```text
crates/
  greign-core/       domain types, recipes, coordinates, errors, versioning
  greign-raw/        decoder interface, metadata and preview extraction
  greign-pipeline/   operation graph and CPU reference implementations
  greign-gpu/        wgpu device management, shaders, tiling, GPU caches
  greign-color/      camera transforms, working spaces, ICC/DCP integration
  greign-catalog/    SQLite schema, migrations, sidecars, search
  greign-film/       roll analysis and negative conversion
  greign-export/     resize, output profiles, JPEG/TIFF encoding
  greign-app/        desktop state, commands, and UI
  greign-cli/        inspection, rendering, corpus, and benchmark utilities
benchmarks/          benchmark drivers and checked-in result schemas
docs/                ADRs, file formats, color pipeline, contributor docs
fixtures/
  manifests/         public corpus URLs, licenses, hashes, expected properties
  synthetic/         small generated fixtures safe to commit
  private/           ignored local photographs, including Canon R7 CR3 files
artifacts/            ignored profiler dumps, traces, benchmark output
```

Create a crate only when it owns a meaningful invariant or subsystem. If two
crates merely forward calls between each other, merge them.

### Initial technology decisions

- Stable Rust, edition 2024, with the minimum supported Rust version recorded.
- `wgpu` with WGSL compute shaders; Vulkan is the primary Linux backend.
- A `RawDecoder` trait isolates decoding from the rest of Greign.
- Evaluate Rawler and LibRaw against the same corpus. Rawler offers a pure-Rust
  path and explicit CR3 support; LibRaw offers broader mature camera coverage.
  Keep both possible until the decoder gate produces evidence. FFI overhead is
  irrelevant compared with decode quality and coverage.
- SQLite in WAL mode with foreign keys, integrity checks, and transactional
  migrations.
- Little CMS for ICC transforms and Lensfun for open lens calibration data.
- `egui` plus `winit`/`wgpu` is the initial desktop stack because it permits
  direct display of GPU textures. UI state must not own imaging state.
- Serde-backed, explicitly versioned edit recipes. Unknown fields must survive
  read/write cycles when practical.
- Rayon is allowed after profiling demonstrates CPU-parallel work. Tokio is
  reserved for genuinely I/O-bound orchestration; it is not the processing
  scheduler.
- Python is allowed for color-science experiments and corpus utilities and must
  use UV for environments and dependencies. Production imaging remains Rust.
- The intended application license is GPL-3.0-or-later. Confirm the final name
  and trademark position before public release.

### Image and pipeline representation

- Decoded integer mosaics remain `u16`; scaled CPU reference data is `f32`.
- The CPU reference pipeline uses linear `f32` throughout.
- The GPU may use `f16` storage or arithmetic only after image-difference,
  out-of-range, and performance tests demonstrate that it is safe per operator.
- Begin with a documented linear, scene-referred Rec. 2020 working space at
  D65. Revisit only through an architecture decision with corpus evidence.
- Operators declare input/output domain, required halo, scale behavior,
  parameter schema, determinism, and cache invalidation boundary.
- Start with a linear ordered graph. Add branches only for masks, statistics,
  or operations that actually require them.
- Tiled operations must specify halo sizes so demosaic, blur, denoise,
  sharpening, and geometry do not produce seams.
- Preview, histogram, and export use the same operator implementations and
  recipe semantics. They may differ in resolution or approved quality tier.
- Every cache key includes source identity, decoder version, operator version,
  parameters, upstream key, resolution, tile coordinates, and color context.
- GPU results go directly to the display texture. Avoid GPU-to-CPU readback on
  the interactive path.

### Non-destructive storage model

- Source pixels are immutable and are never opened for writing.
- A recipe stores stable operator IDs, operator versions, parameters, order,
  crop geometry, masks, and the pipeline schema version.
- Catalog writes are transactional. Sidecars use write-to-temporary, `fsync`
  where appropriate, and atomic rename.
- Standard metadata uses XMP fields. Greign edits use a documented Greign XMP
  namespace. Preserve unknown XMP namespaces when rewriting a sidecar.
- Large raster mask payloads live in content-addressed companion storage rather
  than being expanded inside XML.
- The catalog is the fast query/index layer; sidecars are the portable recovery
  layer. Rebuilding a catalog from sources and sidecars must be supported.
- Record file identity separately from path so moves can be reconciled.
- Virtual copies point to a source identity and own independent recipes.
- Recipe and database migrations are forward-only, transactional, and tested
  against every historical fixture retained by the project.

## Performance contract

Performance tests report distributions, not a single best run. Record p50,
p95, peak resident memory, peak GPU allocation, CPU time, wall time, input
dimensions, build profile, commit, driver, and hardware.

### Hardware tiers

- CPU-only floor: four physical cores, 8 GB RAM, no supported discrete GPU.
- Older GPU floor: approximately 2 GB VRAM with a supported Vulkan driver.
- Main development system: Ryzen 9 7900X, 64 GB RAM, RTX 5070 12 GB.

Algorithms must adapt preview resolution and tile residency to the device. The
older tier is not expected to finish full-resolution export as quickly, but it
must remain responsive and avoid out-of-memory failures.

### Interactive budgets

- Input/event handling and UI layout should stay within 16.7 ms on the main
  system and 33 ms on the older tier.
- Pan and zoom must sustain 60 FPS when the requested tiles are resident.
- An embedded thumbnail should begin appearing without a full RAW decode.
- Slider interaction must produce an updated progressive preview within 50 ms
  on the main system and 100 ms on the older tier.
- Refinement work must be cancellable when a newer edit supersedes it.
- Catalog import, thumbnail generation, and export must never block the UI
  thread.
- No single preview allocation may assume the complete full-resolution image
  plus one full-sized intermediate per operation.
- Keep peak preview memory bounded by the configured cache budget. A 2 GB GPU
  must still edit through reduced-resolution and tiled processing.

Budgets are initial hypotheses. The benchmark harness may refine exact values,
but may not silently remove a performance contract because it is difficult.
Any benchmark regression greater than 5% requires investigation and either a
fix or a recorded justification based on repeated measurements.

### Profiling policy

1. Establish a reproducible benchmark and save its raw result.
2. Check algorithmic work and avoidable recomputation first.
3. Use `cargo flamegraph` and save the raw perf/flamegraph artifacts.
4. Inspect the localized hot path rather than dumping the complete profile to
   the terminal.
5. Use `perf stat` for cache misses, branch behavior, cycles, and memory-bound
   diagnosis when needed.
6. Consider data layout, fusion, auto-vectorization, and compiled kernels.
7. Add parallelism only when sufficient independent work remains.
8. Re-run correctness, memory, and benchmark suites after the change.

GPU profiling always requires elevated permission on the development machine.

## Test strategy

### Test corpus

Maintain three corpus layers:

1. Tiny synthetic files and arrays committed to Git for unit and edge tests.
2. Publicly redistributable RAW files described by manifests containing URL,
   license, SHA-256, camera, format mode, dimensions, and expected properties.
3. Private local fixtures ignored by Git, initially including Canon R7 CR3,
   C-RAW where available, high/low ISO, clipped highlights, underexposure,
   mechanical/electronic shutter cases, and camera-scanned negatives.

Use raw.pixls.us and other sources only when their terms allow the intended
test use. A fetch command must verify the license declaration and digest. CI
uses a curated small subset; scheduled or local compatibility runs use the
larger corpus.

Never encode absolute private fixture paths in tests. Resolve them through a
documented environment variable such as `GREIGN_PRIVATE_RAW_DIR`. Tests that
require private data must report a clear skip, not fail mysteriously.

### Test layers

- Unit tests for every domain invariant and numerical operator.
- Property tests for crop transforms, coordinate mapping, recipe round trips,
  tiling equivalence, parameter bounds, and catalog state transitions.
- Golden tests for decoded metadata, mosaic digests, thumbnails, color patches,
  film conversions, and exported files.
- Differential tests between CPU and GPU using per-operator absolute/relative
  tolerances plus perceptual metrics on final images.
- Cross-tile tests that compare tiled and untiled outputs and inspect seams.
- Integration tests from RAW through preview and export.
- Migration tests for each recipe and database schema version.
- Crash/fault tests around imports, sidecar writes, catalog transactions, cache
  eviction, disk-full behavior, and interrupted export.
- Fuzz tests for sidecars, recipes, metadata boundaries, file sniffing, and any
  parser Greign owns. Treat every imported file as untrusted.
- Benchmark tests for decode, embedded preview, demosaic, each pipeline class,
  cache behavior, histogram, export, catalog queries, and UI scheduling.
- Soak tests that repeatedly edit, cancel, switch images, and export while
  checking memory stability and stale-result suppression.

Target very high coverage in core, catalog, recipe, and pipeline logic, but do
not optimize for a vanity aggregate percentage. Every bug fix adds a regression
test that fails before the fix. Critical unsafe code requires targeted tests,
documented invariants, and Miri where applicable.

### Numerical validation

- Operators define their valid domain and behavior for NaN, infinity, negative
  scene values, and values above 1.0.
- Do not clamp intermediate scene-referred data merely to simplify a shader.
- CPU/GPU comparisons use explicit per-stage tolerances rather than hashes.
- Golden render manifests record the recipe, pipeline version, expected size,
  selected pixel probes, summary statistics, and perceptual comparison limit.
- Camera-profile tests include neutral patches, saturated patches, and values
  outside the output gamut.
- Export tests reopen the output and verify dimensions, bit depth, metadata,
  orientation, ICC profile, and representative pixels.

## Ordered validation gates

The following is the chronological implementation and testing order.

### Gate 0: repository and quality foundation

Build:

- Cargo workspace, formatting and lint policy, GPL license, README, and ADR
  template.
- CI commands for format, Clippy with warnings denied, unit tests, docs, and
  dependency/license auditing.
- Structured logging and build/version reporting.
- Fixture manifest schema and ignored private fixture directories.

Test first:

- A clean checkout can run the documented checks with one command.
- Release and debug builds report the same recipe/schema versions.
- CI does not require private photographs or GPU access.

Exit criteria:

- Formatting, Clippy, tests, and docs pass from a clean checkout.
- The repository contains no downloaded/private RAW files or profiler dumps.

### Gate 1: RAW corpus and decoder decision

Build:

- `greign-cli corpus verify` and manifest parsing.
- `RawDecoder` capability, metadata, preview, and full-mosaic interfaces.
- Rawler adapter first, plus a LibRaw experiment behind the same contract.
- `greign-cli raw inspect`, `preview`, and `digest` commands.

Test first:

- Canon R7 CR3 and C-RAW detection, dimensions, orientation, make/model,
  embedded preview, black/white levels, white balance, CFA, and mosaic digest.
- Corrupt, truncated, renamed, zero-length, enormous, and unsupported inputs.
- CR2, NEF, ARW, RAF, RW2, ORF, and DNG representatives.
- Metadata/preview latency separately from full decode latency.
- Peak memory and unnecessary whole-file copies.

Exit criteria:

- The chosen primary decoder correctly handles the R7 corpus and the curated
  public matrix.
- The alternative backend can remain as a fallback only if it adds meaningful
  coverage without leaking backend types into core APIs.
- Metadata and embedded-preview extraction do not decode the full mosaic.

### Gate 2: CPU reference pipeline

Build in this order:

1. Sensor black/white normalization.
2. Bad-pixel handling and active-area crop.
3. White balance.
4. Bilinear demosaic as the simplest reference.
5. Camera matrix/DNG profile to linear working RGB.
6. Exposure.
7. A simple scene-to-display tone curve.
8. Working-space to sRGB output transform.
9. JPEG/TIFF proof export through the CLI.

Test first:

- Tiny hand-computable mosaics, CFA rotations, odd dimensions, and crop offsets.
- Extreme black/white levels, saturated pixels, negative intermediate values,
  and non-finite parameters.
- Determinism across runs and thread counts.
- Camera-neutral and color-patch fixtures.
- End-to-end R7 render that can be reopened and inspected.

Exit criteria:

- A headless command renders an R7 CR3 to a recognizable, correctly oriented,
  color-managed image using only the reference path.
- Each operation is independently testable and no UI types appear in the
  pipeline.

### Gate 3: GPU runtime and primitive parity

Build:

- Adapter selection, capability reporting, limits, error scopes, and device-loss
  recovery.
- Reusable texture/buffer pool with explicit memory accounting.
- Shader dispatch infrastructure and timestamp instrumentation when supported.
- GPU normalization, white balance, exposure, tone curve, and color matrix.
- Direct presentation of a GPU result texture in a minimal window.

Test first:

- CPU/GPU parity for every primitive across randomized values and dimensions.
- Non-multiple workgroup sizes, small images, large images, and boundary pixels.
- Unsupported features and forced CPU fallback.
- Repeated device initialization, resize, suspend/resume, and simulated device
  loss.
- Peak allocations on 2 GB and 12 GB budget configurations.

Exit criteria:

- Supported primitives meet numerical tolerances and show a measured latency
  improvement for representative preview sizes.
- GPU failure produces a correct CPU result rather than an application crash.
- The display path does not read the completed preview back to CPU memory.

### Gate 4: demosaic quality and complete RAW rendering

Build:

- GPU bilinear demosaic for parity.
- A higher-quality edge-aware demosaic selected through corpus comparison.
- Highlight reconstruction and initial capture sharpening.
- DCP matrix/tone handling needed by the supported camera set.
- Orientation, crop, and full color-managed display path.

Test first:

- Resolution charts, diagonal edges, repeating detail, saturated highlights,
  false color, moire, and high-ISO noise.
- Bayer layouts and Fuji/non-Bayer inputs before claiming their support.
- CPU/GPU output tolerances and tiled/untiled identity.
- Comparison against documented reference renders without requiring Greign to
  mimic another editor's default look.

Exit criteria:

- The R7 corpus renders without visible tiling, orientation, highlight, or
  gross color errors at preview and export resolution.
- Format support is advertised per tested camera/mode, not merely by extension.

### Gate 5: scheduler, progressive rendering, and cache

Build:

- Generation-tagged cancellable render requests.
- Image pyramid and tile scheduler with halo-aware dependencies.
- Memory-budgeted CPU and GPU caches using deterministic keys.
- Interactive quality tiers: immediate reduced preview followed by refinement.
- Background metadata, thumbnail, histogram, preview, and export queues with
  priorities and bounded concurrency.

Test first:

- Rapid slider movement never presents a stale generation.
- Switching images cancels irrelevant work and promptly prioritizes the new
  image.
- Cache invalidation changes only downstream stages.
- Cache eviction remains within configured RAM/VRAM budgets.
- Tiled results match full-frame results at seams and borders.
- Repeated edit/cancel/switch workloads do not leak memory.

Exit criteria:

- Interactive budgets are met on the main system with documented results.
- Constrained configurations remain responsive without allocation failures.

### Gate 6: durable recipe, sidecar, and catalog

Build:

- Versioned edit recipe and operator parameter schemas.
- SQLite catalog with assets, file identities, metadata, recipes, virtual
  copies, history, collections, stacks, film rolls, and cache records.
- Standard XMP mapping and Greign namespace.
- Atomic sidecar writing, conflict detection, catalog rebuild, and backups.
- Undo/redo command history with coalescing for continuous slider input.

Test first:

- Recipe canonicalization and lossless round trips.
- Preservation of unknown XMP fields and namespaces.
- Database constraints, migration rollback, concurrent readers, and crash
  recovery.
- File move/relink, missing source, duplicate content, read-only directory,
  disk full, and interrupted sidecar replacement.
- Rebuild a new catalog from sources and sidecars and reproduce edits.

Exit criteria:

- An imported original is byte-identical after every workflow.
- Edits and virtual copies survive restart and catalog reconstruction.
- `PRAGMA integrity_check` passes after injected failures.

### Gate 7: desktop vertical slice

Build:

- Library grid, filmstrip, develop view, histogram, inspector, and task status.
- Folder import, selection, keyboard navigation, rating/flagging, and filtering.
- GPU preview surface with zoom, pan, fit, clipping overlay, and before/after.
- Controls for the Gate 2/4 processing operations.
- Autosave through the catalog/sidecar command layer.

Test first:

- App state transitions and keyboard commands without a live window.
- Huge folder virtualization; widget count must not scale with catalog size.
- Rapid navigation while thumbnails and previews are still arriving.
- HiDPI, fractional scaling, resize, Wayland, and X11.
- UI responsiveness while importing and exporting.
- Screenshot tests only for stable structural views; behavior tests carry more
  weight than brittle pixel-perfect UI snapshots.

Exit criteria:

- A normal digital shoot can be imported, culled, developed, revisited, and
  exported without the CLI.
- The UI never owns source pixels or duplicates a full preview unnecessarily.

### Gate 8: dedicated film-negative pipeline

Build in this order:

1. Film-roll catalog entity and shared/per-frame parameter inheritance.
2. Manual film-border sampling and robust multi-frame film-base estimate.
3. Sensor normalization and input profile before inversion.
4. Transmittance-to-optical-density conversion and base/fog subtraction.
5. Per-channel exposure/color normalization.
6. Positive tone mapping with highlight rolloff.
7. Robust automatic color and exposure using roll and frame statistics.
8. Batch conversion, synchronized edits, and per-frame overrides.
9. Film metadata and reusable camera/scanner setup profiles.
10. Automatic border/frame assistance only after the manual path is reliable.

Test first:

- Synthetic density wedges with analytically known output.
- Neutral negative, strongly color-biased scene, clipped channel, dense frame,
  thin frame, missing border, uneven illumination, and stray-light cases.
- Multiple stocks, light sources, cameras, macro lenses, and RAW exposure levels.
- Roll analysis stability when a scene is an outlier.
- Shared-setting propagation and per-frame override persistence.
- CPU/GPU parity and batch determinism.

Exit criteria:

- One action creates a useful positive for the private R7 film corpus.
- A whole roll has consistent color and tone without forcing identical per-frame
  exposure.
- Manual controls can recover when automation encounters an adversarial frame.
- The conversion is based on documented color/density math and original test
  work, not proprietary implementation details from commercial products.

### Gate 9: develop-tool breadth

Add and validate one operation family at a time:

1. Advanced tone and curves.
2. Hue/saturation/perceptual color tools.
3. Lens corrections and geometry.
4. Denoising.
5. Capture and output sharpening.
6. Gradient, radial, and brush masks.
7. Copy/paste, presets, and batch synchronization.

For every operation, test:

- Mathematical invariants and extreme parameters.
- CPU/GPU parity.
- Tile halos and seam absence.
- Cache invalidation boundaries.
- Interactive preview and full export behavior.
- Memory and latency before enabling it by default.

Exit criteria:

- Every advertised control is non-destructive, undoable, preset-compatible,
  sidecar-persistent, and covered by regression fixtures.

### Gate 10: library, export, and interoperability completion

Build:

- Fast metadata indexing, compound filters, collections, stacking, and batch
  rename.
- JPEG/TIFF export presets, resizing, output ICC, metadata policy, and output
  sharpening.
- Import standard XMP ratings, flags, keywords, labels, captions, and GPS where
  supported.
- Packaging for a portable Linux archive and one mainstream distribution path.

Test first:

- Large synthetic catalogs and realistic folder trees.
- Query plans and p95 filter/sort latency.
- Unicode paths, removable volumes, symlinks, permissions, duplicate names,
  and network-mounted sources where semantics permit.
- Export bit depth, ICC tags, EXIF/XMP policy, overwrite rules, cancellation,
  and partial failure.
- Clean installation without development libraries.

Exit criteria:

- Catalog size does not cause linear UI work per frame.
- Batch export is restartable/reportable and never silently overwrites files.

### Gate 11: compatibility, robustness, and performance closure

Run the complete matrix:

- Curated camera/format corpus.
- CPU-only, older Vulkan tier, and main development hardware.
- NVIDIA proprietary driver plus available AMD/Intel CI or contributor systems.
- Wayland and X11.
- Clean import, upgraded database, catalog rebuild, missing cache, and read-only
  source cases.
- Long edit/cancel/navigation and batch-export soak tests.
- Fuzz campaigns for all Greign-owned parsers.
- Saved flamegraphs/perf-stat results for representative decode, preview,
  interaction, film conversion, catalog, and export workloads.

Exit criteria:

- No known source-corruption or edit-loss path.
- No known crash on the supported curated corpus.
- Performance budgets pass or have explicit, hardware-specific documented
  exceptions with a viable fallback.
- Memory reaches a steady bound under soak workloads.
- All supported features have user-facing error handling and cancellation.

### Gate 12: version 1 release gate

Version 1 is ready only when all of the following are true:

- A Canon R7 digital RAW shoot can be imported, culled, edited, reopened, and
  exported entirely in Greign.
- A Canon R7 camera-scanned film roll can be batch-converted with a useful
  one-action default and refined non-destructively.
- Originals remain byte-identical.
- Catalog reconstruction from sources and sidecars reproduces edits.
- CPU fallback is functional and GPU failure is recoverable.
- Curated format, numerical, migration, fault, fuzz, soak, and benchmark suites
  pass.
- Installation and first launch work on a clean supported Linux system.
- License notices, third-party licenses, privacy behavior, file formats, and
  backup/recovery procedures are documented.

## Implementation discipline

- Record consequential choices as ADRs under `docs/adr/`.
- Keep commits aligned with a single gate or invariant.
- Do not mix numerical behavior changes with broad refactors.
- Every performance change reports before/after results and identifies the
  measured bottleneck.
- Every bug fix begins with or adds a reproducing test.
- Avoid `unsafe` unless profiling or FFI makes its value concrete; document the
  safety invariant locally.
- Dependencies must have a clear owner, purpose, license, and maintenance
  status. Prefer mature libraries for RAW parsing, ICC, metadata, codecs, and
  lens databases rather than reimplementing standards prematurely.
- Downloaded test data and generated profiles never enter Git unless their
  license, size, and permanence are deliberately approved.

## First implementation handoff

The implementing agent should begin at Gate 0 and stop before creating a GUI.
The first end-to-end milestone is deliberately headless:

```text
Canon R7 CR3
  -> inspect metadata
  -> extract embedded preview
  -> decode mosaic
  -> CPU reference normalization/WB/demosaic/color/exposure/tone
  -> export a color-managed image
  -> record correctness, wall time, and peak memory
```

Once that path is tested, add GPU parity stage by stage. Do not start with a
large UI shell, a general plug-in system, or a catalog schema disconnected from
a working render recipe.
