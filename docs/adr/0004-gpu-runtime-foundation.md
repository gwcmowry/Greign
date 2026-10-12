# ADR 0004: GPU runtime foundation before device binding

- Status: accepted
- Date: 2026-08-29
- Gate: 3

## Context

GPU access on the development system requires explicit elevated permission,
and the `wgpu` dependency was not initially available in the local Cargo cache
during the unattended portion of Gate 3. CI must remain GPU-independent.

## Decision

Establish the adapter-independent invariants first: ceil-divided and
bounds-guarded dispatch, exact 64-byte WGSL uniform layout, validated primitive
parameters, hard resource-budget accounting, generation-based device-loss
recovery, and correct CPU fallback semantics.

Keep the WGSL primitive kernel and Vulkan-first `wgpu` runtime in
`greign-gpu`. Default CI skips explicit GPU tests; permitted local runs validate
shader compilation, parity, initialization, timestamps, and resident textures.

## Consequences

On the RTX 5070, all five primitives match the CPU reference within declared
tolerances. A GPU-resident 1920×1080 color-matrix kernel measures roughly
0.09–0.10 ms p50 versus roughly 10–11 ms for the scalar CPU reference.

The runtime returns an RGBA32F texture with an allocation lease and no image
readback. A future window can present that texture without changing the
processing contract. Default checks continue to pass without a GPU.

## Alternatives considered

Adding uncompiled `wgpu` code was rejected because a dependency-resolution-only
change would provide no evidence. Advancing to later gates was rejected because
the implementation plan requires ordered exit criteria.
