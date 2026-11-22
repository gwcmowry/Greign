"""Install optional Linux CUDA inference libraries locally using UV.

Python is used for installation only; Greign loads the native runtime directly.
No GPU initialization occurs here. Run Greign afterward to validate the device.
"""
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess
import sys

if sys.platform != "linux" or platform.machine() != "x86_64":
    raise SystemExit("The optional CUDA setup currently supports Linux x86-64.")
root = Path(os.environ.get("GREIGN_WORKSPACE", Path(__file__).resolve().parent.parent)).resolve()
uv = shutil.which("uv")
if not uv:
    raise SystemExit("Install UV before running this setup script.")
env = root / ".greign" / "upright-gpu"
env.parent.mkdir(parents=True, exist_ok=True)
if not (env / "bin" / "python").exists():
    subprocess.run([uv, "venv", "--python", "3.12", str(env)], check=True)
python = env / "bin" / "python"
subprocess.run([uv, "pip", "install", "--python", str(python), "-r", str(Path(__file__).with_name("upright-gpu-requirements.txt"))], check=True)
site = Path(subprocess.check_output([str(python), "-c", "import sysconfig; print(sysconfig.get_path('platlib'))"], text=True).strip())
capi = site / "onnxruntime" / "capi"
libraries = sorted(capi.glob("libonnxruntime.so.*"))
if len(libraries) != 1:
    raise SystemExit(f"Expected one ONNX Runtime library in {capi}, found {libraries}")
directories = [capi, *sorted((site / "nvidia").glob("*/lib"))]
config = {"library": str(libraries[0].relative_to(root)), "library_dirs": [str(p.relative_to(root)) for p in directories]}
target = root / ".greign" / "upright-gpu.json"
pending = target.with_suffix(".pending.json")
pending.write_text(json.dumps(config, indent=2) + "\n")
pending.replace(target)
with (env / "installed-requirements.txt").open("w") as output:
    subprocess.run([uv, "pip", "freeze", "--python", str(python)], stdout=output, check=True)
print(f"Installed local CUDA runtime. Restart Greign. Configuration: {target}")
