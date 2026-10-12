"""Independent metadata check. Run with uv run --with exifread."""
from pathlib import Path
import exifread

paths = sorted(Path("exports/capture-date-check").glob("dated.*"))
assert len(paths) == 3, "Generate the three dated fixtures first"
for path in paths:
    with path.open("rb") as handle:
        tags = exifread.process_file(handle, details=False)
    capture = str(tags.get("EXIF DateTimeOriginal"))
    assert capture == "1998:07:03 12:00:00", (path, capture)
    assert str(tags.get("Image DateTime")) == capture
    assert "EXIF OffsetTimeOriginal" not in tags
    print(f"{path}: DateTimeOriginal={capture}; no invented time zone")
