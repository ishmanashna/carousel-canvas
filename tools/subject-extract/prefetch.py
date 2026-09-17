#!/usr/bin/env python3
"""Bootstrap venv, install pinned deps, and download required weights once."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

from cache import (  # noqa: E402
    BIREFNET_PORTRAIT_URL,
    REMBG_MODEL_FILES,
    REMBG_RELEASE_BASE,
    SAM2_CHECKPOINT_NAME,
    SAM2_CHECKPOINT_URL,
    SAM_VIT_B_DECODER,
    SAM_VIT_B_ENCODER,
    birefnet_portrait_fp16_path,
    carousel_models_dir,
    ensure_cache_dirs,
    prefetch_lock_path,
    rembg_model_path,
    sam2_checkpoint_path,
    set_cache_env,
    venv_python,
)

REQUIREMENTS = SCRIPT_DIR / "requirements.txt"
PREFETCH_STATE = ensure_cache_dirs()["root"] / ".prefetch.state"


def _run(cmd: list[str], *, cwd: Path | None = None) -> None:
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, cwd=cwd, check=True)


def _download(url: str, dest: Path, *, max_retries: int = 12) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(dest.suffix + ".part")
    if dest.is_file() and dest.stat().st_size > 0:
        print(f"  already present: {dest.name}")
        return

    resume_at = tmp.stat().st_size if tmp.is_file() else 0
    for attempt in range(max_retries):
        try:
            req = urllib.request.Request(url)
            if resume_at > 0:
                req.add_header("Range", f"bytes={resume_at}-")
            with urllib.request.urlopen(req, timeout=120) as resp:
                mode = "ab" if resume_at > 0 and resp.status in (206, 200) else "wb"
                if mode == "wb":
                    resume_at = 0
                with open(tmp, mode) as out:
                    while True:
                        chunk = resp.read(1024 * 1024)
                        if not chunk:
                            break
                        out.write(chunk)
            tmp.replace(dest)
            print(f"  downloaded: {dest.name}")
            return
        except urllib.error.HTTPError as exc:
            if exc.code == 429:
                wait = min(120, 2 ** attempt)
                print(f"  HTTP 429 for {url}; backing off {wait}s", flush=True)
                time.sleep(wait)
                continue
            if exc.code == 416 and tmp.is_file():
                tmp.replace(dest)
                print(f"  downloaded: {dest.name}")
                return
            raise
        except urllib.error.URLError:
            wait = min(60, 2 ** attempt)
            print(f"  network error; retry in {wait}s", flush=True)
            time.sleep(wait)
    raise RuntimeError(f"failed to download after retries: {url}")


def _link_or_copy(src: Path, dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.is_file():
        print(f"  already present: {dest.name}")
        return
    try:
        os.link(src, dest)
        print(f"  hardlinked: {dest.name} <- {src}")
    except OSError:
        shutil.copy2(src, dest)
        print(f"  copied: {dest.name} <- {src}")


class PrefetchLock:
    def __init__(self, path: Path) -> None:
        self.path = path
        self.fd: int | None = None

    def __enter__(self) -> PrefetchLock:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        while True:
            try:
                self.fd = os.open(self.path, os.O_CREAT | os.O_EXCL | os.O_RDWR)
                os.write(self.fd, str(os.getpid()).encode("ascii"))
                return self
            except FileExistsError:
                age = time.time() - self.path.stat().st_mtime
                if age > 6 * 3600:
                    self.path.unlink(missing_ok=True)
                    continue
                print("prefetch already running; waiting for lock...", flush=True)
                time.sleep(5)

    def __exit__(self, exc_type, exc, tb) -> None:
        if self.fd is not None:
            os.close(self.fd)
        self.path.unlink(missing_ok=True)


def _ensure_venv() -> Path:
    py = venv_python()
    if py.is_file():
        return py
    root = ensure_cache_dirs()["venv"]
    _run([sys.executable, "-m", "venv", str(root)])
    if not py.is_file():
        raise RuntimeError(f"venv creation failed: {py}")
    return py


def _pip_install(py: Path) -> None:
    _run([str(py), "-m", "pip", "install", "--upgrade", "pip"])
    _run([str(py), "-m", "pip", "install", "-r", str(REQUIREMENTS)])
    # rembg pulls CPU onnxruntime; replace with DirectML (same GPU path as crates/vision).
    _run([str(py), "-m", "pip", "uninstall", "-y", "onnxruntime"])
    _run([str(py), "-m", "pip", "install", "onnxruntime-directml==1.20.1"])


def _ensure_birefnet_fp16() -> None:
    dest = birefnet_portrait_fp16_path()
    if dest.is_file() and dest.stat().st_size > 0:
        print(f"  already present: {dest.name}")
        return
    src = rembg_model_path("birefnet-portrait")
    if not src.is_file():
        print("  skip fp16: birefnet-portrait.onnx missing")
        return
    try:
        import onnx
        from onnxconverter_common import float16
    except ImportError:
        print("  skip fp16: onnxconverter-common not in venv; GPU portrait needs birefnet-portrait.fp16.onnx")
        return
    print(f"  converting {src.name} -> {dest.name} (fp16)", flush=True)
    model = onnx.load(str(src))
    converted = float16.convert_float_to_float16(model, keep_io_types=True)
    dest.parent.mkdir(parents=True, exist_ok=True)
    onnx.save(converted, str(dest))
    print(f"  wrote: {dest.name}")


def _download_weights() -> None:
    set_cache_env()

    isnet_shared = carousel_models_dir() / REMBG_MODEL_FILES["isnet-general-use"][0]
    isnet_dest = rembg_model_path("isnet-general-use")
    if isnet_shared.is_file():
        _link_or_copy(isnet_shared, isnet_dest)
    else:
        _download(
            f"{REMBG_RELEASE_BASE}/{REMBG_MODEL_FILES['isnet-general-use'][0]}",
            isnet_dest,
        )

    _download(BIREFNET_PORTRAIT_URL, rembg_model_path("birefnet-portrait"))
    _ensure_birefnet_fp16()
    _download(
        f"{REMBG_RELEASE_BASE}/{SAM_VIT_B_ENCODER}",
        rembg_model_path("sam").parent / SAM_VIT_B_ENCODER,
    )
    _download(
        f"{REMBG_RELEASE_BASE}/{SAM_VIT_B_DECODER}",
        rembg_model_path("sam").parent / SAM_VIT_B_DECODER,
    )
    _download(SAM2_CHECKPOINT_URL, sam2_checkpoint_path())


def _write_state() -> None:
    PREFETCH_STATE.write_text(f"completed_at={time.time()}\n", encoding="utf-8")


def main() -> int:
    set_cache_env()
    lock_path = prefetch_lock_path()
    with PrefetchLock(lock_path):
        print("subject-extract prefetch")
        py = _ensure_venv()
        _pip_install(py)
        set_cache_env()
        _download_weights()
        _write_state()
        print("prefetch complete")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
