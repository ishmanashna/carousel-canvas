"""SAM2 automatic masks, apply, and refine commands."""

from __future__ import annotations

import json
import math
import multiprocessing as mp
import sys
from pathlib import Path

import cv2
import numpy as np
from PIL import Image, ImageDraw, ImageFont, ImageOps

from cache import EXIT_MISSING_WEIGHTS, EXIT_TIMEOUT, SAM2_CONFIG_NAME, sam2_checkpoint_path
from checker_cmd import compose_checker


def _load_rgb(path: Path) -> tuple[np.ndarray, tuple[int, int]]:
    with Image.open(path) as img:
        img = ImageOps.exif_transpose(img)
        full_size = img.size
        rgb = np.array(img.convert("RGB"))
    return rgb, full_size


def _resize_long_side(rgb: np.ndarray, max_side: int) -> tuple[np.ndarray, float]:
    h, w = rgb.shape[:2]
    long_side = max(h, w)
    scale = min(1.0, max_side / long_side)
    if scale >= 1.0:
        return rgb, 1.0
    new_w = max(1, int(round(w * scale)))
    new_h = max(1, int(round(h * scale)))
    resized = cv2.resize(rgb, (new_w, new_h), interpolation=cv2.INTER_AREA)
    return resized, scale


def _require_sam2_checkpoint() -> Path | None:
    ckpt = sam2_checkpoint_path()
    if ckpt.is_file():
        return ckpt
    print(f"error: missing SAM2 checkpoint: {ckpt}", file=sys.stderr)
    return None


def _build_sam2_model(checkpoint: Path):
    import torch
    from sam2.build_sam import build_sam2

    device = "cpu"
    model = build_sam2(SAM2_CONFIG_NAME, str(checkpoint), device=device)
    return model, device


def _mask_to_uint8(mask: np.ndarray) -> np.ndarray:
    if mask.dtype == bool:
        return (mask.astype(np.uint8) * 255)
    return (mask > 0).astype(np.uint8) * 255


def _write_cutout_and_checker(
    rgb_full: np.ndarray,
    mask_full: np.ndarray,
    out_dir: Path,
) -> None:
    alpha = _mask_to_uint8(mask_full)
    rgba = np.dstack([rgb_full, alpha])
    cutout_path = out_dir / "cutout.png"
    Image.fromarray(rgba, mode="RGBA").save(cutout_path)
    compose_checker(str(cutout_path), str(out_dir / "cutout_checker.png"))


def _generate_worker(
    image_path: str,
    out_dir: str,
    max_side: int,
    points_per_side: int,
    crop_n_layers: int,
    max_masks: int,
    queue: mp.Queue,
) -> None:
    try:
        from cache import set_cache_env

        set_cache_env()
        rgb_full, full_size = _load_rgb(Path(image_path))
        work_rgb, work_scale = _resize_long_side(rgb_full, max_side)
        work_h, work_w = work_rgb.shape[:2]
        print(f"working size={work_w}x{work_h}", flush=True)

        checkpoint = sam2_checkpoint_path()
        if not checkpoint.is_file():
            queue.put({"ok": False, "error": "missing_sam2", "path": str(checkpoint)})
            return

        from sam2.automatic_mask_generator import SAM2AutomaticMaskGenerator

        model, _device = _build_sam2_model(checkpoint)
        generator = SAM2AutomaticMaskGenerator(
            model,
            points_per_side=points_per_side,
            crop_n_layers=crop_n_layers,
        )
        masks = generator.generate(work_rgb)
        if len(masks) > max_masks:
            masks = sorted(masks, key=lambda m: float(m.get("predicted_iou", 0)), reverse=True)[
                :max_masks
            ]
        print(f"Got {len(masks)} masks", flush=True)

        out = Path(out_dir)
        out.mkdir(parents=True, exist_ok=True)

        mask_stack = np.stack([m["segmentation"].astype(np.uint8) for m in masks], axis=0)
        np.savez_compressed(out / "masks.npz", masks=mask_stack)

        meta = {
            "full_width": full_size[0],
            "full_height": full_size[1],
            "work_width": work_w,
            "work_height": work_h,
            "work_scale": work_scale,
            "max_side": max_side,
            "points_per_side": points_per_side,
            "crop_n_layers": crop_n_layers,
            "mask_count": len(masks),
        }
        (out / "meta.json").write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")

        cols = max(1, int(math.ceil(math.sqrt(len(masks)))))
        thumb = 128
        sheet = Image.new("RGB", (cols * thumb, ((len(masks) + cols - 1) // cols) * thumb), (32, 32, 32))
        draw = ImageDraw.Draw(sheet)
        font = ImageFont.load_default()

        work_pil = Image.fromarray(work_rgb)
        for idx, ann in enumerate(masks):
            seg = ann["segmentation"]
            overlay = np.array(work_pil).copy()
            color = np.array([255, 64, 64], dtype=np.uint8)
            overlay[seg] = (overlay[seg] * 0.45 + color * 0.55).astype(np.uint8)
            ov_img = Image.fromarray(overlay)
            draw_ov = ImageDraw.Draw(ov_img)
            draw_ov.text((4, 4), str(idx), fill=(255, 255, 0))
            ov_long = max(ov_img.size)
            if ov_long > 1024:
                s = 1024 / ov_long
                ov_img = ov_img.resize(
                    (max(1, int(ov_img.width * s)), max(1, int(ov_img.height * s))),
                    Image.Resampling.LANCZOS,
                )
            ov_img.save(out / f"overlay_{idx:02d}.png")

            m = Image.fromarray(_mask_to_uint8(seg)).resize((thumb, thumb), Image.Resampling.NEAREST)
            row, col = divmod(idx, cols)
            sheet.paste(m.convert("RGB"), (col * thumb, row * thumb))
            draw.text((col * thumb + 4, row * thumb + 4), str(idx), fill=(255, 255, 0), font=font)
        sheet.save(out / "contact_sheet.png")
        queue.put({"ok": True, "mask_count": len(masks)})
    except Exception as exc:  # noqa: BLE001 - worker boundary
        queue.put({"ok": False, "error": str(exc)})


def run_sam2_generate(
    image_path: str,
    out_dir: str,
    *,
    max_side: int = 1024,
    points_per_side: int = 16,
    crop_n_layers: int = 0,
    timeout_seconds: int = 0,
    max_masks: int = 32,
) -> int:
    if _require_sam2_checkpoint() is None:
        return EXIT_MISSING_WEIGHTS

    src = Path(image_path)
    if not src.is_file():
        print(f"error: image not found: {src}", file=sys.stderr)
        return 1

    ctx = mp.get_context("spawn")
    queue: mp.Queue = ctx.Queue()
    proc = ctx.Process(
        target=_generate_worker,
        args=(
            image_path,
            out_dir,
            max_side,
            points_per_side,
            crop_n_layers,
            max_masks,
            queue,
        ),
    )
    proc.start()
    if timeout_seconds and timeout_seconds > 0:
        proc.join(timeout_seconds)
        if proc.is_alive():
            proc.terminate()
            proc.join(timeout=5)
            if proc.is_alive():
                proc.kill()
                proc.join(timeout=5)
            print(f"error: sam2-generate timed out after {timeout_seconds}s", file=sys.stderr)
            return EXIT_TIMEOUT
    else:
        print("sam2-generate: waiting until finished (no timeout)", flush=True)
        proc.join()

    if queue.empty():
        print("error: sam2-generate produced no result", file=sys.stderr)
        return 1

    result = queue.get()
    if not result.get("ok"):
        if result.get("error") == "missing_sam2":
            print(f"error: missing SAM2 checkpoint: {result.get('path')}", file=sys.stderr)
            return EXIT_MISSING_WEIGHTS
        print(f"error: sam2-generate failed: {result.get('error')}", file=sys.stderr)
        return 1
    return 0


def _load_masks(out_dir: Path) -> tuple[np.ndarray, dict]:
    npz_path = out_dir / "masks.npz"
    meta_path = out_dir / "meta.json"
    if not npz_path.is_file() or not meta_path.is_file():
        raise FileNotFoundError(f"missing masks.npz or meta.json in {out_dir}")
    data = np.load(npz_path)
    masks = data["masks"]
    meta = json.loads(meta_path.read_text(encoding="utf-8"))
    return masks, meta


def _upscale_mask_nearest(mask: np.ndarray, full_size: tuple[int, int]) -> np.ndarray:
    full_w, full_h = full_size
    return cv2.resize(mask.astype(np.uint8), (full_w, full_h), interpolation=cv2.INTER_NEAREST) > 0


def run_sam2_apply(image_path: str, out_dir: str, mask_ids: list[int]) -> int:
    if _require_sam2_checkpoint() is None:
        return EXIT_MISSING_WEIGHTS

    src = Path(image_path)
    out = Path(out_dir)
    if not src.is_file():
        print(f"error: image not found: {src}", file=sys.stderr)
        return 1
    if not out.is_dir():
        print(f"error: out-dir not found: {out}", file=sys.stderr)
        return 1

    try:
        masks, meta = _load_masks(out)
    except FileNotFoundError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    if not mask_ids:
        print("error: --ids must list at least one mask id", file=sys.stderr)
        return 1

    combined = np.zeros(masks.shape[1:], dtype=bool)
    for mid in mask_ids:
        if mid < 0 or mid >= masks.shape[0]:
            print(f"error: mask id out of range: {mid}", file=sys.stderr)
            return 1
        combined |= masks[mid].astype(bool)

    rgb_full, full_size = _load_rgb(src)
    if full_size != (meta["full_width"], meta["full_height"]):
        print(
            "warning: image size differs from meta.json; using current image size",
            file=sys.stderr,
        )
    mask_full = _upscale_mask_nearest(combined.astype(np.uint8), full_size)
    _write_cutout_and_checker(rgb_full, mask_full, out)
    return 0


def _parse_points(raw: str) -> list[list[float]]:
    if not raw.strip():
        return []
    data = json.loads(raw)
    if not isinstance(data, list):
        raise ValueError("points must be a JSON array")
    points: list[list[float]] = []
    for item in data:
        if not isinstance(item, (list, tuple)) or len(item) != 2:
            raise ValueError("each point must be [x, y]")
        points.append([float(item[0]), float(item[1])])
    return points


def run_sam2_refine(
    image_path: str,
    out_dir: str,
    include_raw: str,
    exclude_raw: str,
) -> int:
    checkpoint = _require_sam2_checkpoint()
    if checkpoint is None:
        return EXIT_MISSING_WEIGHTS

    src = Path(image_path)
    out = Path(out_dir)
    if not src.is_file():
        print(f"error: image not found: {src}", file=sys.stderr)
        return 1
    out.mkdir(parents=True, exist_ok=True)

    try:
        include_pts = _parse_points(include_raw)
        exclude_pts = _parse_points(exclude_raw)
    except (json.JSONDecodeError, ValueError) as exc:
        print(f"error: invalid include/exclude points: {exc}", file=sys.stderr)
        return 1

    if not include_pts and not exclude_pts:
        print("error: provide at least one include or exclude point", file=sys.stderr)
        return 1

    rgb_full, full_size = _load_rgb(src)
    work_rgb, work_scale = _resize_long_side(rgb_full, 1024)

    def to_work(points: list[list[float]]) -> np.ndarray:
        if work_scale == 1.0:
            arr = np.array(points, dtype=np.float32)
        else:
            arr = np.array(
                [[p[0] * work_scale, p[1] * work_scale] for p in points],
                dtype=np.float32,
            )
        return arr

    point_coords = []
    point_labels = []
    for pt in include_pts:
        point_coords.append(to_work([pt])[0])
        point_labels.append(1)
    for pt in exclude_pts:
        point_coords.append(to_work([pt])[0])
        point_labels.append(0)

    from sam2.sam2_image_predictor import SAM2ImagePredictor

    model, device = _build_sam2_model(checkpoint)
    predictor = SAM2ImagePredictor(model)
    predictor.set_image(work_rgb)
    masks, scores, _logits = predictor.predict(
        point_coords=np.array(point_coords, dtype=np.float32),
        point_labels=np.array(point_labels, dtype=np.int32),
        multimask_output=len(point_coords) > 1,
    )
    best_idx = int(np.argmax(scores))
    work_mask = masks[best_idx]
    mask_full = _upscale_mask_nearest(work_mask.astype(np.uint8), full_size)
    _write_cutout_and_checker(rgb_full, mask_full, out)
    return 0
