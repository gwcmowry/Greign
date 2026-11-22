"""Run the explicit native CUDA check using the locally installed runtime.

The runner only prepares the native loader environment; inference is Rust/CUDA.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parent.parent
workspace = Path(os.environ.get("GREIGN_WORKSPACE", root)).resolve()
config = json.loads((workspace / ".greign/upright-gpu.json").read_text())
env = os.environ.copy()
paths = [str(workspace / p) for p in config["library_dirs"]]
if env.get("LD_LIBRARY_PATH"):
    paths.append(env["LD_LIBRARY_PATH"])
env["LD_LIBRARY_PATH"] = os.pathsep.join(paths)
env["GREIGN_UPRIGHT_GPU_LIBRARY"] = str(workspace / config["library"])
(root / "artifacts/perf").mkdir(parents=True, exist_ok=True)
env["GREIGN_UPRIGHT_CUDA_PROFILE"] = str(root / "artifacts/perf/upright-native-cuda")
command = [str(root / "target/release/examples/upright_check"), *sys.argv[1:]]
raise SystemExit(subprocess.run(command, cwd=root, env=env, check=False).returncode)
