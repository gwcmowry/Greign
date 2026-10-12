# ADR 0007: Catalog, recipe, and sidecar durability

- Status: Accepted
- Date: 2026-08-30

## Context

Greign must never modify a RAW original, while edits and virtual copies must
survive restart, database loss, file moves, and catalog reconstruction. XMP
files may contain fields and namespaces owned by other applications.

## Decision

Use canonical versioned JSON for typed global edit recipes. Reject unsupported
schema/operator versions, duplicate current global operators, non-finite
values, and invalid parameter domains. Normalize negative zero before
serialization. Continuous commands coalesce by gesture key into one undo step.

Use bundled SQLite through `rusqlite` for the catalog. Enable WAL, foreign
keys, a busy timeout, transactional migrations, and schema checks. Content
hash identifies an asset independently of its current path. Separate tables
own file identities, metadata, base recipes, virtual copies, history,
collections, stacks, film rolls, membership, and cache records. Online backup
uses SQLite's backup API.

Use the namespace `https://greign.org/ns/1.0/` in XMP. Standard rating, label,
and Dublin Core subject fields remain visible in ordinary XMP markup. The
complete canonical Greign payload is JSON in one marker-delimited owned RDF
description. Rewriting removes only that owned block, leaving all unknown XML
outside it byte-for-byte intact.

Write sidecars to a temporary file in the destination directory, flush and
`fsync` it, atomically rename it, then `fsync` the directory. A SHA-256 snapshot
provides optimistic conflict detection before replacement.

## Consequences

Original files are only read and path-referenced. Duplicate content, missing
paths, and relinks do not change asset identity. Recipes and virtual copies
survive restart and backup, and a new catalog can reconstruct them from sources
and sidecars. Injected migration, storage, and pre-rename failures leave the
previous durable state intact. SQLite `PRAGMA integrity_check` passes after all
fault tests.
