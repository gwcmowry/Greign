# Export resolution and neutral re-analysis

The actual JPEGs in
`/media/grant/T7 Shield/261104_roll_001/2026_11_05 - converted` were checked
against their frozen `roll-recipe.json` and the Canon R7 RAW metadata. The 26
photographs are 11.535–14.655 MP (median 14.194 MP), typically about 4600×3100.
Every JPEG dimension matches its stored crop applied to the 6960×4640 usable
RAW image, allowing one pixel of rounding. The recipe has `long_edge: 0`.
Greign is exporting full cropped resolution, without a size-limit resize.

The camera mosaic includes optical-black margins; the usable crop is 32.2944
MP. A typical saved composition retains approximately 66% of its width and
67% of its height, or 44% of its pixels. Achieving 24 MP after cropping requires
retaining about 74% of that usable area when capturing/composing the scan.

Apple's current documentation distinguishes legacy Shared Albums (created in
version 26 or earlier, ordinary photos reduced to 2048 pixels on the long edge)
from version 27 albums, which retain full resolution and count toward the
owner's storage. Existing albums can be upgraded; new additions then use full
resolution. The reported ~2K album copies match the legacy limit, but the album's
software version was not inspected. Sources, checked November 7, 2026:
[limits](https://support.apple.com/en-us/148868) and
[upgrading](https://support.apple.com/en-us/108916).

## Brightness diagnosis and selected result

The exported roll still used calibration from before the narrow-border autocrop
fix. Its median density range was 2.921, versus 0.898 when re-analyzed using the
picture boundaries. Including the dark surround exaggerated that range and
made the positive dark. Existing manual corrections included +0.9 EV, contrast
+8, warmth +11, and shadows -15. Applying all those corrections to the refreshed
calibration overexposed many frames.

The user preferred the `neutral` comparison: fresh per-frame calibration with
lighting and color edits at zero, retaining composition. This is the shipped
reference. An experimental extra midtone lift was evaluated but was not adopted.
All 28 final neutral JPEG previews are byte-for-byte identical to the preferred
neutral baseline. No changes were made to the user's originals, existing JPEG
exports, or live library.

Re-analysis now defaults to **Neutral lighting and color** for individual-frame
proposals. Proposal controls appear above the contact sheet as well as in
Conversion. Preview changes neither saved calibration nor saved adjustments.
**Accept neutral conversion** saves the complete previous look, accepts the
calibration, and resets roll/frame lighting and color adjustments in one Undo
step. Crops, quarter turns, mirroring, dust repairs, metadata, review and
rejection are preserved. Unchecking Neutral keeps the existing corrections.
CPU previews, GPU uniforms, and histogram measurement share the same adjustment
selection. Accepted exports use the resulting saved roll as before.

Calibration version 4 identifies analysis after the narrow-border fix and
supports a visible refresh notice for older individual calibrations. The
neutral conversion mathematics match the preferred version-3 re-analysis;
accepted older analyses retain their original rendering until explicitly
replaced. New tests exercise preview-only behavior, neutral and keep-adjustments
acceptance, saved-version comparison, discard, atomic Undo/Redo, persistence,
and preservation of composition and repairs.

## Evidence

- `artifacts/261104-export-dimensions.json`, `261104-source-metadata.json`, and
  `261104-export-recipe.json` contain the resolution audit.
- `exports/brightness-261104/baseline/` and `final/` contain saved, re-analyzed
  with existing edits, and neutral comparisons, with corresponding sheets.
- `artifacts/brightness-261104-final.jsonl` contains per-frame calibration and
  linear-luminance measurements. These are diagnostics, not a universal target
  for scene brightness.
- `artifacts/neutral-app-tests.log`, `neutral-clippy.log`,
  `neutral-repository-checks.log`, and `neutral-release-final-build.log` record
  the test/check/build results. The repository suite passes 176 tests, with
  nine opt-in tests ignored.

Native replay on an isolated copy of the exported roll verified both contact
sheet and full-size neutral previews. Both left the saved library unchanged.
Acceptance matched the diagnostic calibration for all 26 kept frames, reset
lighting/color controls, and preserved every other frame field. A complete
before-conversion version was added; Undo and Redo exactly restored the two
library snapshots. Evidence is in `exports/brightness-261104/native/` and
`artifacts/neutral-ui-verify.log`. The film engine also passes its Rust 1.85
check (`artifacts/neutral-msrv.log`).
