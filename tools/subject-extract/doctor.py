"""Health check for venv, imports, weights, and disk space."""

from __future__ import annotations

import importlib.util
import shutil
import sys
from pathlib import Path

from cache import (
    EXIT_DOCTOR_FAILED,
    REMBG_MODEL_FILES,
    SAM2_CHECKPOINT_NAME,
    birefnet_portrait_fp16_path,
    cache_root,
    sam2_checkpoint_path,
    venv_python,
)
from gpu import accelerated, available_providers, provider_names


def _check_import(module_name: str) -> tuple[bool, str]:
    if importlib.util.find_spec(module_name) is None:
        return False, "missing"
    return True, "ok"


def _disk_free_gb(path: Path) -> float:
    usage = shutil.disk_usage(path)
    return usage.free / (1024**3)


def run_doctor() -> int:
    ok = True
    lines: list[str] = []

    def mark(name: str, passed: bool, detail: str) -> None:
        nonlocal ok
        if not passed:
            ok = False
        status = "OK" if passed else "MISSING"
        lines.append(f"  [{status}] {name}: {detail}")

    root = cache_root()
    py = venv_python()
    mark("venv python", py.is_file(), str(py))

    if py.is_file():
        for mod in ("rembg", "torch", "sam2", "PIL", "numpy", "cv2", "jsonschema"):
            present, detail = _check_import(mod)
            mark(f"import {mod}", present, detail)
    else:
        for mod in ("rembg", "torch", "sam2"):
            mark(f"import {mod}", False, "venv missing")

    for model_name, filenames in REMBG_MODEL_FILES.items():
        for filename in filenames:
            path = cache_root() / "rembg" / filename
            mark(f"weight {filename}", path.is_file(), str(path))

    sam2_ckpt = sam2_checkpoint_path()
    mark(f"weight {SAM2_CHECKPOINT_NAME}", sam2_ckpt.is_file(), str(sam2_ckpt))

    fp16 = birefnet_portrait_fp16_path()
    fp16_ok = fp16.is_file()
    lines.append(
        f"  [{'OK' if fp16_ok else 'SKIP'}] weight birefnet-portrait.fp16.onnx: "
        f"{fp16 if fp16_ok else 'missing — fp32 portrait OOMs 4GB; GPU portrait needs this file'}"
    )

    providers = available_providers()
    wanted = provider_names()
    gpu_ok = accelerated() and wanted[0] in providers
    mark(
        "onnx gpu (DirectML/CUDA)",
        gpu_ok,
        f"preferred={','.join(wanted)} available={','.join(providers)}",
    )

    try:
        free_gb = _disk_free_gb(root if root.exists() else Path.home())
        mark("disk free", free_gb >= 2.0, f"{free_gb:.1f} GB free")
    except OSError as exc:
        mark("disk free", False, str(exc))

    print("subject-extract doctor")
    print(f"cache root: {root}")
    for line in lines:
        print(line)

    if not ok:
        print("\nRun: python tools/subject-extract/prefetch.py", file=sys.stderr)
        return EXIT_DOCTOR_FAILED
    print("\nAll checks passed.")
    return 0
