"""Run rembg foreground models without downloading weights."""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
from cache import (
    ALLOWED_REMBG_MODELS,
    EXIT_MISSING_WEIGHTS,
    birefnet_portrait_fp16_path,
    rembg_model_paths,
)
from gpu import note_session_providers, patch_ort_sessions, preferred_providers, session_options
from gpu_lock import gpu_lock
from PIL import Image
from timing import Span, note


def _require_model(model_name: str) -> Path | None:
    if model_name not in ALLOWED_REMBG_MODELS:
        print(
            f"error: model must be one of: {', '.join(ALLOWED_REMBG_MODELS)}",
            file=sys.stderr,
        )
        return None
    paths = rembg_model_paths(model_name)
    missing = [path for path in paths if not path.is_file()]
    if missing:
        print(
            f"error: missing weights for {model_name}: {', '.join(str(p) for p in missing)}",
            file=sys.stderr,
        )
        return None
    return paths[0]


def _point_birefnet_at_fp16() -> bool:
    fp16 = birefnet_portrait_fp16_path()
    if not fp16.is_file():
        return False
    from rembg.sessions.birefnet_portrait import BiRefNetSessionPortrait

    def download(cls, *args, **kwargs) -> str:  # noqa: ARG001
        return str(fp16)

    BiRefNetSessionPortrait.download_models = classmethod(download)
    return True


def _empty_alpha(img: Image.Image) -> bool:
    if img.mode != "RGBA":
        return False
    alpha = np.array(img.getchannel("A"))
    return float((alpha > 8).mean()) < 0.01


def run_rembg(model_name: str, image_path: str, out_path: str) -> int:
    weight_path = _require_model(model_name)
    if weight_path is None:
        return EXIT_MISSING_WEIGHTS if model_name in ALLOWED_REMBG_MODELS else 1

    src = Path(image_path)
    out = Path(out_path)
    if not src.is_file():
        print(f"error: image not found: {src}", file=sys.stderr)
        return 1

    from rembg import new_session, remove

    patch_ort_sessions()
    used_fp16 = False
    if model_name == "birefnet-portrait":
        used_fp16 = _point_birefnet_at_fp16()

    with gpu_lock():
        load = Span()
        session = new_session(
            model_name,
            sess_opts=session_options(),
            providers=preferred_providers(),
        )
        note_session_providers(session)
        note("session_load_seconds", round(load.seconds(), 3))
        note("weights", "fp16" if used_fp16 else "fp32")
        infer = Span()
        try:
            with Image.open(src) as img:
                note("width", img.width)
                note("height", img.height)
                result = remove(img, session=session)
        except Exception as exc:  # noqa: BLE001 - ORT/DML surfaces as RuntimeError
            hint = ""
            if model_name == "birefnet-portrait" and not used_fp16:
                hint = " fp32 birefnet-portrait OOMs 4GB; convert to birefnet-portrait.fp16.onnx."
            print(f"error: rembg GPU run failed ({type(exc).__name__}).{hint}", file=sys.stderr)
            return 1
        note("infer_seconds", round(infer.seconds(), 3))
        note("model", model_name)
        if _empty_alpha(result):
            print("error: rembg returned an empty mask", file=sys.stderr)
            return 1
        out.parent.mkdir(parents=True, exist_ok=True)
        result.save(out)
        return 0
