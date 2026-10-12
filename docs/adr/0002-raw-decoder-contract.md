# ADR 0002: Rawler as the initial primary decoder

- Status: provisional
- Date: 2026-08-29
- Gate: 1

## Context

Greign requires independent metadata, embedded-preview, and integer-mosaic
operations, with Canon R7 CR3 and C-RAW as its first compatibility target.
Backend types must not escape into pipeline code.

Rawler 0.7.0 supports Rust 1.85, recognizes the R7 C-RAW samples, and decodes
their 7128×4732 mosaics. Its CR3 adapter does not expose preview methods and
hardcodes `RawImage` orientation to normal, though its parsed CR3 structure and
separate EXIF metadata contain the required information.

The evaluated `rsraw` 0.1.1 LibRaw wrapper requires loading the complete file
into a `Vec`, does not expose the unprocessed mosaic through its safe API, and
bundles a large C++ build. Those limitations prevent it from satisfying the
same contract without unsafe access to binding internals.

## Decision

Use a backend-neutral `RawDecoder` trait and make Rawler 0.7 the provisional
primary backend. Normalize EXIF orientation, finite RGB white balance, the D65
XYZ-to-camera matrix, levels, CFA, and recording mode inside the adapter.

For R7 CR3, extract the encoded JPEG directly from the parsed CCTP preview
track. Current R7 firmware labels that track type 16; older CR3 files may use
type 2. Do not decode and re-encode the JPEG on the import path.

Do not add `rsraw` as a workspace dependency unless corpus evidence shows
meaningful additional coverage and a maintained safe mosaic interface exists.
This decision remains provisional until the complete Gate 1 public/private
matrix passes.

## Consequences

The R7 metadata and preview paths avoid full mosaic decompression and downstream
code sees no Rawler types or non-finite fourth WB coefficient. Rawler still
maps files with eager population, and C-RAW full decode currently peaks near
243 MiB for a roughly 67 MiB output mosaic; both require continued measurement.

CR3 structural fallback code is localized in the Rawler adapter and covered by
real-corpus tests. Supporting another backend requires implementing the same
contract rather than weakening it.

## Alternatives considered

Using Rawler values directly was rejected because its hardcoded orientation,
deprecated zero matrix field, and unused NaN WB coefficient violate Greign's
domain invariants. Adding `rsraw` immediately was rejected for its whole-file
copy, incomplete safe contract, and native build cost.

