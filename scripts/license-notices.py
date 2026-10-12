"""Collect dependency license files, including bundled C codecs, for the app."""
import json
from pathlib import Path
import subprocess
import sys

metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--locked", "--format-version", "1",
    "--filter-platform", sys.argv[1],
]))
with Path(sys.argv[2]).open("w") as output:
    output.write("Greign dependency license notices\n\n")
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        output.write(f"\n{'=' * 72}\n{package['name']} {package['version']}\n")
        output.write(f"License: {package.get('license') or 'See files below'}\n")
        output.write(f"Source: {package.get('repository') or package.get('source') or 'Greign workspace'}\n")
        root = Path(package["manifest_path"]).parent
        for path in sorted(root.rglob("*")):
            if path.is_file() and path.name.upper().startswith(("LICENSE", "COPYING", "NOTICE", "COPYRIGHT")):
                output.write(f"\n--- {path.relative_to(root)} ---\n")
                output.write(path.read_text(errors="replace"))
                output.write("\n")
