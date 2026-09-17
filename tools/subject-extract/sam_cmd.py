"""rembg SAM commands with JSON prompt validation."""

from __future__ import annotations

import json
import sys
from pathlib import Path

import jsonschema
from cache import EXIT_INVALID_PROMPTS, EXIT_MISSING_WEIGHTS, rembg_model_paths
from gpu import note_session_providers, patch_ort_sessions, preferred_providers, session_options
from gpu_lock import gpu_lock
from PIL import Image
from timing import Span, note

SCHEMA_PATH = Path(__file__).resolve().parent / "schemas" / "prompts.schema.json"


def _load_schema() -> dict:
    return json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))


def _load_prompts(prompts_path: str) -> tuple[dict | None, int | None]:
    path = Path(prompts_path)
    if not path.is_file():
        print(f"error: prompts file not found: {path}", file=sys.stderr)
        return None, 1
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        print(f"error: invalid JSON in {path}: {exc}", file=sys.stderr)
        return None, EXIT_INVALID_PROMPTS
    schema = _load_schema()
    try:
        jsonschema.validate(instance=data, schema=schema)
    except jsonschema.ValidationError as exc:
        print(f"error: prompts.json failed schema validation: {exc.message}", file=sys.stderr)
        return None, EXIT_INVALID_PROMPTS
    return data, None


def _require_sam_weights() -> bool:
    paths = rembg_model_paths("sam")
    missing = [path for path in paths if not path.is_file()]
    if not missing:
        return True
    print(
        f"error: missing SAM weights: {', '.join(str(p) for p in missing)}",
        file=sys.stderr,
    )
    return False


def _run_sam(image_path: str, out_path: str, extra: dict) -> int:
    if not _require_sam_weights():
        return EXIT_MISSING_WEIGHTS

    src = Path(image_path)
    out = Path(out_path)
    if not src.is_file():
        print(f"error: image not found: {src}", file=sys.stderr)
        return 1

    from rembg import new_session, remove

    patch_ort_sessions()
    with gpu_lock():
        load = Span()
        session = new_session(
            "sam",
            sess_opts=session_options(),
            providers=preferred_providers(),
        )
        note_session_providers(session)
        note("session_load_seconds", round(load.seconds(), 3))
        infer = Span()
        with Image.open(src) as img:
            note("width", img.width)
            note("height", img.height)
            result = remove(img, session=session, **extra)
        note("infer_seconds", round(infer.seconds(), 3))
        note("model", "sam")
        out.parent.mkdir(parents=True, exist_ok=True)
        result.save(out)
        return 0


def run_sam_points(prompts_path: str, image_path: str, out_path: str) -> int:
    data, err = _load_prompts(prompts_path)
    if err is not None:
        return err
    assert data is not None
    points = [item for item in data["sam_prompt"] if item.get("type") == "point"]
    if not points:
        print("error: sam-points requires at least one point prompt", file=sys.stderr)
        return EXIT_INVALID_PROMPTS
    return _run_sam(image_path, out_path, {"sam_prompt": data["sam_prompt"]})


def run_sam_box(
    xyxy: str,
    prompts_path: str,
    image_path: str,
    out_path: str,
) -> int:
    data, err = _load_prompts(prompts_path)
    if err is not None:
        return err
    assert data is not None

    points = [item for item in data["sam_prompt"] if item.get("type") == "point"]
    if not points:
        print("error: sam-box requires point prompts in the prompts file", file=sys.stderr)
        return EXIT_INVALID_PROMPTS

    try:
        parts = [float(v.strip()) for v in xyxy.split(",")]
    except ValueError:
        print("error: --xyxy must be x1,y1,x2,y2 numbers", file=sys.stderr)
        return 1
    if len(parts) != 4:
        print("error: --xyxy must contain exactly four values", file=sys.stderr)
        return 1

    box = {"type": "rectangle", "data": parts, "label": 1}
    prompt = [box] + data["sam_prompt"]
    return _run_sam(image_path, out_path, {"sam_prompt": prompt})
