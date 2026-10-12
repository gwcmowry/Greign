# ADR 0001: Headless Rust workspace foundation

- Status: accepted
- Date: 2026-08-29
- Gate: 0

## Context

Greign needs domain and decoder contracts that remain independent of any UI.
The first milestone is a headless RAW-to-export path, and crates should exist
only when they own a meaningful invariant.

## Decision

Begin with three crates: `greign-core` owns cross-subsystem version invariants,
`greign-raw` owns fixture manifests and decoder contracts, and `greign-cli`
owns command-line orchestration and process-level logging. Add pipeline, color,
GPU, catalog, film, export, and app crates only as their gates begin.

Use Rust edition 2024 with Rust 1.85 as the initial minimum supported version.

## Consequences

The workspace is small enough to validate quickly while establishing inward
dependency direction. The minimum Rust version must be tested in CI and raised
only through a new decision.

## Alternatives considered

Creating every proposed crate immediately was rejected because empty wrapper
crates would obscure ownership. A single crate was rejected because decoder
adapters and command-line concerns should not become dependencies of domain
types.

