# Native darktable Negadoctor integration

The darktable fork now analyzes the input of Negadoctor directly in the preview
pixelpipe. It does not route pixels through the Python reference implementation.

## User workflow

1. Open a camera scan in darkroom.
2. Enable **Negadoctor** and choose color or B&W film.
3. On **film properties**, wait for **analyze current frame** to become active.
4. Press it once.

The action updates only the technical controls:

- film base (`D min`);
- film density range (`D max`);
- scanner exposure bias (`offset`);
- highlight channel balance;
- shadow channel balance is returned to neutral.

Paper black, paper grade, gloss/soft clipping, and print exposure are deliberately
left unchanged. This keeps objective measurement separate from rendering taste.

The complete parameter change is added with `dt_dev_add_history_item()` as one
ordinary enabled Negadoctor history item. It therefore participates in undo,
history replay, XMP sidecars, copy/paste, and styles without a parallel Greign
edit format.

## Confidence and safeguards

The status below the button reports confidence and whether film base came from
a usable border or was inferred from the frame. Uneven borders are not labeled
as clean rebate.

The common analyzer accepts an optional known input ceiling and rejects film
base at that ceiling. Darktable working RGB is unbounded and can legitimately
exceed `1.0`, so the pixelpipe deliberately leaves the ceiling unspecified
rather than inventing a clipping warning. Sensor-domain clipping metadata is a
future input to this check.

Analysis is tagged with the image ID of the preview that produced it, so a
result cannot be accidentally applied after switching images. CPU and OpenCL
preview paths feed the same native analyzer. The OpenCL path reads back only
the preview input; exports do not perform analysis or incur that transfer.

## Selected-roll workflow

The lighttable right panel now includes **film roll**:

1. Select only scans belonging to one physical roll and one unchanged capture
   setup.
2. Choose color negative or black-and-white negative.
3. Press **analyze selected roll**.

A cancellable background job renders a bounded 1600-pixel float preview up to,
but not through, Negadoctor for each image. It then aggregates base/color/range
statistics, retains per-frame luminance placement, and appends one enabled
Negadoctor history item to every usable image. The operation forms one grouped
lighttable undo action, synchronizes sidecars, invalidates affected thumbnails,
and reports analysis and history-write time separately.

No blank frame is required. Clean visible rebates win when available; otherwise
three or more frames use the lower-confidence roll-envelope mode. Explicit
reference-patch and borrowed-prior selection are still available in the
executable reference but are not yet exposed in the lighttable panel.

## Build and validate

Configure and build the fork in its isolated build directory:

```sh
cmake -S darktable -B darktable/build-separated \
  -DCMAKE_BUILD_TYPE=RelWithDebInfo \
  -DBUILD_TESTING=ON
cmake --build darktable/build-separated -j12
ctest --test-dir darktable/build-separated --output-on-failure \
  -R 'film_(analysis|roll_analysis)'
```

Run the fork without replacing the system installation:

```sh
darktable/build-separated/bin/darktable
```

The headless development harness exercises the same proxy and history bridge
against an isolated in-memory library:

```sh
XDG_CONFIG_HOME=/tmp/greign-roll-config \
XDG_CACHE_HOME=/tmp/greign-roll-cache \
darktable/build-separated/bin/darktable-film-roll-cli scan-01.CR2 scan-02.CR2
```
