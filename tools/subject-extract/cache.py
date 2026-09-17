"""Cache paths and environment pinning for subject-extract."""

from __future__ import annotations

import os
from pathlib import Path

EXIT_MISSING_WEIGHTS = 10
EXIT_TIMEOUT = 11
EXIT_INVALID_PROMPTS = 12
EXIT_DOCTOR_FAILED = 2
EXIT_OTHER = 1
EXIT_OK = 0

REMBG_MODEL_FILES: dict[str, list[str]] = {
    "isnet-general-use": ["isnet-general-use.onnx"],
    "birefnet-portrait": ["birefnet-portrait.onnx"],
    "sam": [
        "sam_vit_b_01ec64.encoder.onnx",
        "sam_vit_b_01ec64.decoder.onnx",
    ],
}

ALLOWED_REMBG_MODELS = ("isnet-general-use", "birefnet-portrait")

SAM2_CHECKPOINT_NAME = "sam2.1_hiera_tiny.pt"
SAM2_CONFIG_NAME = "configs/sam2.1/sam2.1_hiera_t.yaml"

REMBG_RELEASE_BASE = "https://github.com/danielgatis/rembg/releases/download/v0.0.0"
SAM_VIT_B_ENCODER = "sam_vit_b_01ec64.encoder.onnx"
SAM_VIT_B_DECODER = "sam_vit_b_01ec64.decoder.onnx"
BIREFNET_PORTRAIT_URL = (
    "https://github.com/danielgatis/rembg/releases/download/v0.0.0/"
    "BiRefNet-portrait-epoch_150.onnx"
)
SAM2_CHECKPOINT_URL = (
    "https://dl.fbaipublicfiles.com/segment_anything_2/092824/sam2.1_hiera_tiny.pt"
)


def local_app_data() -> Path:
    base = os.environ.get("LOCALAPPDATA")
    if not base:
        raise RuntimeError("LOCALAPPDATA is not set")
    return Path(base)


def cache_root() -> Path:
    return local_app_data() / "CarouselCanvas" / "subject-extract"


def carousel_models_dir() -> Path:
    return local_app_data() / "CarouselCanvas" / "models"


def venv_python() -> Path:
    if os.name == "nt":
        return cache_root() / "venv" / "Scripts" / "python.exe"
    return cache_root() / "venv" / "bin" / "python"


def rembg_model_paths(model_name: str) -> list[Path]:
    return [cache_root() / "rembg" / filename for filename in REMBG_MODEL_FILES[model_name]]


def rembg_model_path(model_name: str) -> Path:
    return rembg_model_paths(model_name)[0]


def birefnet_portrait_fp16_path() -> Path:
    return cache_root() / "rembg" / "birefnet-portrait.fp16.onnx"


def sam2_checkpoint_path() -> Path:
    return cache_root() / "sam2" / SAM2_CHECKPOINT_NAME


def prefetch_lock_path() -> Path:
    return cache_root() / ".prefetch.lock"


def ensure_cache_dirs() -> dict[str, Path]:
    root = cache_root()
    dirs = {
        "root": root,
        "venv": root / "venv",
        "rembg": root / "rembg",
        "hf": root / "hf",
        "torch": root / "torch",
        "sam2": root / "sam2",
        "lock": prefetch_lock_path(),
    }
    for key, path in dirs.items():
        if key != "lock":
            path.mkdir(parents=True, exist_ok=True)
    return dirs


def set_cache_env() -> None:
    """Pin rembg / HF / torch caches under the subject-extract root."""
    dirs = ensure_cache_dirs()
    os.environ["U2NET_HOME"] = str(dirs["rembg"])
    os.environ["HF_HOME"] = str(dirs["hf"])
    os.environ["HF_HUB_CACHE"] = str(dirs["hf"] / "hub")
    os.environ["TORCH_HOME"] = str(dirs["torch"])
    os.environ["XDG_CACHE_HOME"] = str(dirs["root"])
