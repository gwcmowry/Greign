# Edit recipe schema 1

Recipe schema 1 is canonical compact JSON with these top-level fields in
order: `schema_version`, `pipeline_version`, and ordered `operators`.

Each operator has an implementation `version`, an `enabled` flag, a stable
snake-case `operator` tag, and typed parameters. Schema 1 defines:

- `white_balance`: positive RGB `coefficients`
- `highlight`: positive normalized `threshold`
- `chroma_denoise`: `strength` from 0 (off) through 1; default 0.5
- `exposure`: finite `ev` in stops
- `tone`: non-negative `strength`
- `capture_sharpen`: `amount` from 0 through 2
- `crop`: normalized `x`, `y`, `width`, and `height` contained in the source

Only one instance of each current global operator is accepted. Operator order
is meaningful and is never sorted during canonicalization. Negative zero is
serialized as zero; NaN and infinity are rejected.

Pipeline behavior version 2 introduces corrected R7 saturation metadata,
regional highlight-color reconstruction, conservative chroma denoising,
luminance-only sharpening, and a white-anchored quality-render shoulder.
Version-1 recipes are rejected explicitly rather than silently reinterpreted;
schema and catalog versions remain 1. Existing files are not rewritten.
