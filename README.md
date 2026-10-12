# Greign

Greign is a Linux-first, GPU-accelerated, non-destructive RAW developer and
photographic library. Its defining workflow is fast, roll-aware conversion of
camera-scanned film negatives.

For Mac installation and downloads, see [Greign for macOS](docs/macos.md).

The project is under active, gate-driven development. See [PLAN.md](PLAN.md)
for the product scope, architecture, performance contract, and ordered exit
criteria.

This repository root contains the original standalone Greign editor: its Rust
workspace, scripts, documentation, RAW fixtures, and render comparisons.

The separate [darktable modification project](darktable-mods/README.md) contains
the darktable fork, Negadoctor/film-roll work, Python conversion prototype,
film fixtures, research, and profiling outputs. It is not part of the Rust
workspace. Greign's independent darktable render oracle remains here because
it tests Greign's image quality.

## Requirements

- Rust 1.85 or newer
- A POSIX shell for the repository check script
- `cargo-audit` and `cargo-deny` only when running dependency audits locally

No GPU or private photographs are required for the default checks.

## Native film workspace

Launch the standalone desktop application (no browser or local web server):

```sh
./scripts/launch-greign.sh
```

Once built, `./target/release/greign-app` starts it directly. The window uses
eframe/egui with OpenGL and requires a graphical desktop session.

The prototype provides roll import and metadata, shared film analysis,
positive contact sheets, whole-roll and per-frame adjustments, manual dust
healing, crop/rotation, undo/redo, and JPEG or 16-bit TIFF export. Originals
are referenced, not overwritten. Edits live in `.greign/rolls.json`; default
exports go under `exports/film-rolls/` in this repository.

The workspace also includes multi-selection with relative edits, selective
copy/paste, named versions and hold-to-compare, preview/accept re-analysis,
conversion locks, review flags, export presets and failed-frame retries.
Lighting includes highlights and shadows; an eight-band color mixer provides
hue, saturation and brightness adjustments. See the
[workflow and controls guide](docs/native-workflow.md).

The native GPU preview keeps the active frame's full-resolution converted
source on the GPU during slider edits, with CPU fallback for unsupported paths.
Ctrl-scroll zooms around the cursor in Single frame. See
[GPU preview and benchmarks](docs/gpu-preview.md) and
[interactive preview and zoom](docs/interactive-preview.md).

For further editing elsewhere, export an active frame or selection as a
32-bit linear DNG. See [DNG handoff](docs/dng-handoff.md) for what is baked in,
the darktable neutral-import check, and the remaining Lightroom validation.

When the local R7 comparison images are available, a new library starts with
a clearly labeled simulated-negative demo roll. This tests the workflow, not
real film color accuracy; the conversion baseline still needs validation on
actual camera scans. See the [native UI validation checklist](docs/testing/native-ui.md)
for automated coverage and a short hands-on test.

## Check a clean checkout

Run the same non-audit checks used by continuous integration:

```sh
./scripts/check.sh
```

The command formats-checks the workspace, runs Clippy with warnings denied,
runs all tests, builds API documentation with warnings denied, and verifies
that debug and release builds report identical recipe and catalog schema
versions.

Dependency and license audits can be run separately after installing their
Cargo subcommands:

```sh
cargo audit
cargo deny check
```

## Private test photographs

Private RAW photographs belong under `fixtures/private/`, which is ignored by
Git. Set `GREIGN_PRIVATE_RAW_DIR` to use a different location. Tests needing
private files must skip clearly when the directory is absent.

Downloaded public corpus files belong under `fixtures/downloads/` and are also
ignored. Only manifests, licenses, hashes, and deliberately approved tiny
synthetic fixtures may be committed.

Run the opt-in private Canon R7 compatibility checks with:

```sh
GREIGN_PRIVATE_RAW_DIR=/path/to/raws cargo test -p greign-raw --test private_r7 -- --nocapture
```

The tests report explicit skips when the directory, a C-RAW, or a full RAW is
unavailable. Useful Gate 1 CLI commands include:

```sh
cargo run --release -p greign-cli -- raw inspect photo.CR3
cargo run --release -p greign-cli -- raw preview photo.CR3 preview.jpg
cargo run --release -p greign-cli -- raw digest photo.CR3
cargo run --release -p greign-cli -- corpus verify fixtures/manifests/empty.json
cargo run --release -p greign-cli -- raw render photo.CR3 output.jpg --quality
cargo run --release -p greign-cli -- raw render photo.CR3 preview.jpg --quality \
  --preview-dimension 960
```

See [camera and mode compatibility](docs/compatibility.md) for the distinction
between render-supported, decode-tested, and explicitly decode-only inputs.
Persisted edit and portability formats are documented under
[docs/formats](docs/formats/recipe-v1.md).

For image-quality work, Darktable provides an optional independent render
oracle so CPU/GPU parity cannot bless a defect shared by both Greign paths.
See [the Darktable oracle guide](docs/testing/darktable-oracle.md).
The [2026-09-05 color-noise investigation](docs/quality/demosaic-diagnosis-2026-09-05.md)
records the decoder comparison, R7 saturation-metadata defect, and remaining
render-quality limitations.
The [R7 rendering correction](docs/quality/r7-render-fix.md) documents the
implemented fixes, controls, validation, and stage timings. Local comparison
exports are available in [exports/r7-render-fix/](exports/r7-render-fix/index.html).

## License

Greign is licensed under the GNU General Public License, version 3 or (at your
option) any later version. See [LICENSE](LICENSE).
