# Interactive preview and mouse-centered zoom

The [GPU preview](gpu-preview.md) is now the default for eligible active
positive frames: it keeps a full-resolution source resident during editing.
The adaptive CPU lanes and measurements below describe the fallback path,
initial placeholders, and non-active thumbnails. Frames with manual healing
and original-negative display remain on that CPU path.

Rebuild/start with `./scripts/launch-greign.sh`, or run the rebuilt native
binary with `./target/release/greign-app`. Close any older running window first.

## Zoom

In Single frame, **Ctrl-scroll on Linux** zooms around the mouse. The input
handler also accepts the platform Command modifier. Ordinary scrolling pans;
drag with the viewing tool to pan, or middle-drag while another tool is active.
Fit resets zoom/pan. 100% detail requests the original-resolution active frame
and sets one image pixel per physical display pixel when it arrives.

Image zoom is independent of application/UI scaling and of draft texture
dimensions. Refining a preview does not reset its zoom or pan. Borders are
allowed during zoom so cursor anchoring also works on letterboxed/portrait
images. Returning all the way to Fit recenters the image. There is a 32× fit
zoom limit, and panning keeps part of the image reachable.

## Rendering changes

- While a slider is held, render only the active frame at a 480-pixel longest
  edge. On release, refresh affected 960-pixel thumbnails and render the
  active frame at full source resolution in the background.
- Separate latest-request mailboxes for fast and full-quality work prevent a
  slow full-resolution load/render from holding up the next draft. Each
  mailbox holds only the newest pending request, not a backlog of slider ticks.
- Generation checks cancel obsolete conversion/adjustment chunks and reject
  stale results before publishing. Decoder calls, allocations, and some output
  conversion stages are not preemptible mid-call; they run off the UI thread.
- Cache the negative conversion and camera-color transform *before* creative
  adjustments. Moving lighting/color sliders no longer repeats them.
- Per-frame render keys include source identity, calibration, adjustments,
  crop, rotation, repairs and quality. Unchanged neighbors are skipped, and
  the active frame is processed first. Changed thumbnails follow after release.
- Full-resolution source and conversion caches live in the full lane and are
  bounded to one source/scene. They never replace the UI's small scan cache.
  The fast lane retains at most 24 prepared images across preview qualities.
- Undo snapshots are no longer cloned repeatedly during an active gesture.

Full-resolution refinement applies to the active frame, not every frame on
every drag release. Exports still render each kept/exported frame at full
resolution (subject to the selected JPEG/TIFF export size). Drafts are only
for interaction, never export. The existing CPU/export pipeline is retained;
this change does not add GPU image processing.

## Validation and measurements

32 ordinary tests pass across app, film and export; one additional opt-in
performance test passes separately. Tests include:

- native egui Command/Ctrl-wheel input without changing UI scale;
- cursor-coordinate invariance, including a letterboxed image;
- unchanged-frame skipping, separate draft/full dimensions, newest-request
  completion after a burst, and stale-result rejection;
- cached/direct render equality and cooperative cancellation;
- existing crop/heal, saved-version comparison, export and DNG checks.

No OS-level desktop automation or GPU access was used. All native window
interactions still benefit from a hands-on check.

The private eight-frame simulated-negative demo benchmark gave:

| Measurement | Result |
| --- | ---: |
| Previously observed whole-roll preview batches | 1.19 s mean |
| Cached 480×320 draft worker render | 2.9 ms mean, 3.8 ms maximum |
| Current draft ready at 16 ms polling cadence | 120 / 120 ticks across two runs |
| Eight released 960-pixel thumbnail updates | about 100 ms |
| First 6960×4640 full refinement, including source load | about 2.8 s |
| Warm full refinement, with conversion cached | 789 ms (one sample) |
| Concurrent draft while that full refinement runs | 3.1 ms (one sample) |

These measure different workloads intentionally: the improvement comes from
doing much less work during interaction and keeping refinement separate, not
claiming the same full-resolution work became hundreds of times faster.
The remaining expensive step is full-resolution rendering/output preparation,
plus source loading on a cold refinement. The benchmark includes RGB8 packing
and delivery, but not native GPU texture upload or display latency. It does
not establish real camera-scan color quality.

Raw dumps are `artifacts/slider-latency-observed.jsonl`,
`artifacts/interactive-worker-profile.jsonl`, and
`artifacts/interactive-latency.jsonl`. Visible draft/full outputs are in
`exports/interactive-preview/`. Reproduce the CPU benchmark with:

```sh
cargo test --offline --release -p greign-app profile_interactive_lanes -- --ignored
```

This opt-in test requires the existing local R7 comparison JPEGs, appends
timing records, and refreshes its two proof images. Routine checks are:

```sh
cargo test --offline -p greign-app -p greign-film -p greign-export
cargo clippy --offline -p greign-app -p greign-film -p greign-export --all-targets -- -D warnings
```

Build/test logs use `artifacts/interactive-{tests,clippy,native-build}.log`.
