# Folder import and named batch exports

Folder import reads direct supported image files, retains the source folder,
uses its name for the roll, and inherits persisted library export defaults.
It ignores non-image files, AppleDouble sidecars, and nested folders. Empty or
unreadable selections cannot create a roll. Individual-file import remains
available, as does adding a folder to an existing roll.

Exports reserve a new `roll name + suffix` directory beneath the chosen master
folder. Reservation uses atomic directory creation; collisions receive `(2)`,
`(3)`, etc. Path components retain Unicode names while replacing separators and
invalid filename characters, bounding UTF-8 length, and handling reserved names.
The output contains a frozen recipe and the IDs of the selected kept frames.

The app queues ready rolls sequentially with frozen settings. Readiness is
independent of whether the batch list is expanded. Missing conversion, empty
kept selections, and invalid capture dates are identified before submission.
Failed frames and failed destinations do not prevent subsequent rolls from
exporting. Frame retries use the original settings and directories, and do not
repeat successful frames. Destination-creation failures are named in the batch
summary and can be corrected before exporting that roll again.

Five new automated tests cover:

- Folder enumeration, naming, source-folder persistence, defaults inheritance,
  empty/error selections, and legacy library loading.
- Unique concurrent directory reservation, path containment, Unicode/reserved
  names, selection manifests, and preservation of pre-existing files.
- A real two-roll JPEG export with a missing frame, skipped unready/invalid-date
  rolls, rejected frames, frozen settings, and successful failed-only retry.
- Continuation after a destination-creation error without modifying the
  blocking file.
- Import and batch-dialog contents at 1080×720 and 1440×960, including master
  destination, suffix preview, skipped rolls, defaults, and export controls.

The repository check passes with 171 tests and 9 opt-in tests ignored, including
formatting, Clippy, documentation, and schema agreement. Native release build
and focused app checks also pass. Logs: `artifacts/folder-workflow-build.log`,
`artifacts/folder-workflow-repository-checks.log`,
`artifacts/folder-workflow-clippy.log`, and `artifacts/folder-workflow-tests.log`.

Native replay supplied two folder-picker results and committed each using the
visible Import roll button. Both rolls inherited the saved master destination
and contained exactly their two direct images; a nested image and text sidecars
were excluded. The native run exercised Use these settings for every roll,
one-click Export roll, and two complete Export ready rolls batches using GPU
finishing. The rejected-only roll was listed as skipped and was not exported.

The result was ten JPEGs in five folders: `Portra 01 - converted`, its `(2)`
and `(3)` siblings, and `Portra 02 - converted` with its `(2)` sibling. Recipes
and selection manifests matched each imported roll. SHA-256 checks confirmed
every source image and every file from the initial export remained unchanged.
The native completion status reported four frames across two rolls for the
batch. The OS folder chooser itself was not automated; replay provides the
same path result to the import handler.

Native evidence is under `artifacts/folder-workflow-native/`: `import-folder.png`,
`two-rolls.png`, `batch-settings.png`, `one-click-export.png`, `batch-exported.png`,
`batch-repeated.png`, their state snapshots, replay scripts, output folders,
and `verification.json`. The import and export dialogs are centered so pointer
interactions do not move them during settings changes.
