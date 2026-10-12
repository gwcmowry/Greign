"""Exercise the relocated macOS bundle with synthetic images and CPU/GPU previews."""
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib


def png_chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def synthetic_png(path):
    width, height = 320, 240
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            rows.extend((30 + x * 190 // width, 30 + y * 190 // height, 90 + (x + y) % 100))
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + png_chunk(b"IDAT", zlib.compress(rows))
        + png_chunk(b"IEND", b"")
    )


source = Path(sys.argv[1]).resolve()
proof = Path("artifacts/macos-smoke").resolve()
proof.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix="greign-macos-") as directory:
    temporary = Path(directory)
    app = temporary / "Relocated Greign.app"
    shutil.copytree(source, app)
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
    binary = app / "Contents/MacOS/greign-app"
    for mode in ("gpu", "cpu"):
        workspace = temporary / mode
        images = workspace / "exports/r7-render-fix"
        images.mkdir(parents=True)
        sample = images / "sample.png"
        synthetic_png(sample)
        subprocess.run([
            "sips", "-s", "format", "jpeg", str(sample),
            "--out", str(images / "152A0123-after.jpg"),
        ], check=True, capture_output=True)
        env = dict(os.environ, GREIGN_WORKSPACE=str(workspace))
        env.pop("GREIGN_LIBRARY_PATH", None)
        env.pop("GREIGN_CPU_PREVIEW", None)
        if mode == "gpu":
            args = [str(binary), "--gpu-ui-check"]
            screenshots = [workspace / "exports/gpu-preview" / name for name in ("native-window.png", "native-zoom.png")]
        else:
            env["GREIGN_CPU_PREVIEW"] = "1"
            screenshot = workspace / "cpu-preview.png"
            replay = workspace / "replay.json"
            replay.write_text(json.dumps([{"action": "capture", "path": str(screenshot)}]))
            args = [str(binary), "--smoke-script", str(replay)]
            screenshots = [screenshot]
        with (proof / f"{mode}.log").open("w") as log:
            subprocess.run(args, cwd=temporary, env=env, stdout=log, stderr=log, timeout=90, check=True)
        for screenshot in screenshots:
            data = screenshot.read_bytes()
            assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 10_000, screenshot
            width, height = struct.unpack(">II", data[16:24])
            assert width >= 1080 and height >= 720, (width, height)
            shutil.copy2(screenshot, proof / f"{mode}-{screenshot.name}")
        library = workspace / ("artifacts/gpu-ui-library.json" if mode == "gpu" else ".greign/rolls.json")
        saved = json.loads(library.read_text())
        assert saved["rolls"] and saved["rolls"][0]["frames"], "No imported synthetic scan"
        shutil.copy2(library, proof / f"{mode}-library.json")
        print(f"{mode} packaged app startup, preview, screenshot and library save passed")
