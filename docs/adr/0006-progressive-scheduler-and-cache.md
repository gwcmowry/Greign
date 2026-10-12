# ADR 0006: Progressive scheduler and cache contract

- Status: Accepted
- Date: 2026-08-30

## Context

Interactive edits must never present stale work, and background metadata,
thumbnail, histogram, refinement, and export jobs must not crowd out the next
visible preview. Preview memory also has to remain useful on the 8 GB CPU and
2 GB GPU floor configurations.

## Decision

Tag every interactive request with one atomic generation. Beginning a new edit
or selecting another source invalidates all older requests; the presentation
slot accepts only the current generation. Workers cooperatively inspect the
same token before presenting or beginning another unit of work.

Use one bounded priority queue for all work classes. Immediate previews outrank
metadata, thumbnails, histograms, refinement, and export. When the pending
bound is full, an urgent job may replace the lowest-priority background job.
In-flight concurrency is independently bounded.

Use byte-accounted deterministic LRU caches. Keys include source identity,
decoder and operator versions, parameters, upstream identity, resolution,
tile coordinates, stage, and color context. Parameter changes invalidate the
changed stage and downstream stages only. CPU and GPU owners instantiate the
same cache policy with separate budgets; GPU allocations remain additionally
guarded by `greign-gpu`'s resource budget.

Prepare a reduced Bayer preview once, cache its scene-linear Rec. 2020 pixels,
and apply exposure, sharpening, tone, and display conversion for each slider
generation. Full-quality refinement uses the same operation semantics.

## Consequences

Stale results cannot replace a newer preview. A 1,000-edit switch/cancel soak
keeps queue depth and cache bytes bounded. The public R7 870×580 cached preview
updates well inside the main-system 50 ms budget. Initial preparation remains
background work and will benefit from later GPU stage fusion; it does not block
the presentation of the embedded preview.
