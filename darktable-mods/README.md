# Darktable film-workflow modifications

This folder contains the darktable-based project, separate from the original
standalone Greign Rust editor in the parent directory.

The GitHub repository stores the local modifications in
`patches/film-workflow.patch`, pinned to the upstream revision in
`patches/upstream-commit.txt`. The checkout, build outputs and downloaded
photographs are excluded. On a fresh clone, restore the source with:

```sh
./darktable-mods/scripts/restore-darktable.sh
```

The script leaves an existing checkout intact. This independent project is
not included in the Greign macOS application.

- `darktable/`: the fork, including its Git history, submodules, native
  Negadoctor analysis, film-roll panel/CLI, tests, and existing build outputs.
- `scripts/`: the Python automatic-conversion reference and correctness harness.
- `docs/`: film workflow research, integration notes, calibration, and timings.
- `fixtures/`: film-negative manifests, downloaded scans, sidecars, and exports.
- `artifacts/`: conversion results, references, benchmark inputs, and raw profiles.

Unless stated otherwise, commands in these documents run from this folder:

```sh
cd darktable-mods
./scripts/check-film-auto.sh
```

See the [workflow direction](docs/research/darktable-film-direction.md),
[conversion prototype](docs/testing/film-auto-spike.md),
[film-base calibration](docs/testing/film-base-calibration.md),
[native integration](docs/testing/darktable-negadoctor-integration.md), and
[batch timings](docs/performance/film-roll-batch.md).

The source and associated files were moved together on 2026-09-05. Existing
build directories and historical reports are preserved; their generated
absolute paths may still reference the former location. Use a fresh CMake
build directory (`darktable/build-separated`) for builds at this location.
Historical report paths describe the original runs and are not rewritten.

Greign's darktable oracle scripts and Canon R7 comparisons remain in the parent
project: they validate Greign and are not fork modifications.
