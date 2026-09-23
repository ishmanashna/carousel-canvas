"""Seed copy, alpha union/subtract, clip, erase, crop-rembg, grow-color, morphology."""

from __future__ import annotations

import json
import shutil
import sys
import tempfile
from collections import deque
from pathlib import Path

import cv2
import numpy as np
from PIL import Image
from rembg_cmd import run_rembg


def parse_xyxy(raw: str) -> tuple[int, int, int, int] | None:
    parts = [p.strip() for p in raw.replace(" ", "").split(",") if p.strip()]
    if len(parts) != 4:
        print("error: --xyxy must be x1,y1,x2,y2 (quote it in PowerShell)", file=sys.stderr)
        return None
    try:
        x1, y1, x2, y2 = (int(float(p)) for p in parts)
    except ValueError:
        print("error: --xyxy values must be numbers", file=sys.stderr)
        return None
    if x2 <= x1 or y2 <= y1:
        print("error: --xyxy needs x2>x1 and y2>y1", file=sys.stderr)
        return None
    return x1, y1, x2, y2


def parse_point(raw: str) -> tuple[int, int] | None:
    parts = [p.strip() for p in raw.replace(" ", "").split(",") if p.strip()]
    if len(parts) != 2:
        print("error: --point must be x,y (quote it in PowerShell)", file=sys.stderr)
        return None
    try:
        return int(float(parts[0])), int(float(parts[1]))
    except ValueError:
        print("error: --point values must be numbers", file=sys.stderr)
        return None


def _open_rgba(path: str) -> Image.Image | None:
    p = Path(path)
    if not p.is_file():
        print(f"error: file not found: {p}", file=sys.stderr)
        return None
    return Image.open(p).convert("RGBA")


def _clamp_box(
    box: tuple[int, int, int, int], width: int, height: int
) -> tuple[int, int, int, int]:
    x1, y1, x2, y2 = box
    x1 = max(0, min(width - 1, x1))
    y1 = max(0, min(height - 1, y1))
    x2 = max(x1 + 1, min(width, x2))
    y2 = max(y1 + 1, min(height, y2))
    return x1, y1, x2, y2


def _save_from_original(
    original: Image.Image, alpha: np.ndarray, out_path: str
) -> int:
    rgb = np.array(original.convert("RGB"), dtype=np.uint8)
    if alpha.shape[:2] != rgb.shape[:2]:
        print("error: alpha size does not match original image", file=sys.stderr)
        return 1
    rgba = np.dstack([rgb, alpha])
    out = Path(out_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(rgba, mode="RGBA").save(out)
    return 0


def run_seed(from_path: str, out_dir: str) -> int:
    src = Path(from_path)
    if src.is_dir():
        src = src / "cutout.png"
    if not src.is_file():
        print(f"error: seed cutout not found: {src}", file=sys.stderr)
        return 1
    dest_dir = Path(out_dir)
    dest_dir.mkdir(parents=True, exist_ok=True)
    dest = dest_dir / "cutout.png"
    shutil.copy2(src, dest)
    print(f"seeded {dest} from {src}")
    return 0


def run_crop(image_path: str, xyxy: str, out_path: str) -> int:
    box = parse_xyxy(xyxy)
    if box is None:
        return 1
    img = _open_rgba(image_path)
    if img is None:
        return 1
    x1, y1, x2, y2 = _clamp_box(box, img.width, img.height)
    out = Path(out_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    img.crop((x1, y1, x2, y2)).save(out)
    print(f"cropped {x1},{y1},{x2},{y2} -> {out}")
    return 0


def run_union(image_path: str, a_path: str, b_path: str, out_path: str) -> int:
    original = _open_rgba(image_path)
    a = _open_rgba(a_path)
    b = _open_rgba(b_path)
    if original is None or a is None or b is None:
        return 1
    if a.size != original.size or b.size != original.size:
        print("error: union inputs must match original width/height", file=sys.stderr)
        return 1
    alpha = np.maximum(np.array(a)[:, :, 3], np.array(b)[:, :, 3])
    code = _save_from_original(original, alpha, out_path)
    if code == 0:
        print(f"union -> {out_path}")
    return code


def run_subtract(image_path: str, cutout_path: str, donor_path: str, out_path: str) -> int:
    original = _open_rgba(image_path)
    cutout = _open_rgba(cutout_path)
    donor = _open_rgba(donor_path)
    if original is None or cutout is None or donor is None:
        return 1
    if cutout.size != original.size or donor.size != original.size:
        print("error: subtract inputs must match original width/height", file=sys.stderr)
        return 1
    cut_a = np.array(cutout)[:, :, 3]
    don_a = np.array(donor)[:, :, 3]
    removed = int(np.sum((cut_a >= 128) & (don_a >= 128)))
    alpha = np.where(don_a >= 128, np.uint8(0), cut_a)
    code = _save_from_original(original, alpha, out_path)
    if code == 0:
        print(f"subtract removed {removed} opaque pixels -> {out_path}")
    return code


def run_wipe_islands(
    cutout_path: str,
    out_path: str,
    min_person_frac: float = 0.08,
    link_px: int = 16,
) -> int:
    """Keep person-sized blobs and bits within link_px of them; drop floating junk."""
    img = _open_rgba(cutout_path)
    if img is None:
        return 1
    arr = np.array(img)
    opaque = arr[:, :, 3] > 8
    if not bool(opaque.any()):
        print("error: wipe-islands: empty alpha", file=sys.stderr)
        return 1
    n_labels, labels, stats, _centroids = cv2.connectedComponentsWithStats(
        opaque.astype(np.uint8),
        connectivity=8,
    )
    areas = {i: int(stats[i, cv2.CC_STAT_AREA]) for i in range(1, n_labels)}
    if not areas:
        print("error: wipe-islands: no components", file=sys.stderr)
        return 1
    max_area = max(areas.values())
    kept: set[int] = {
        i for i, area in areas.items() if area >= min_person_frac * max_area
    }
    if not kept:
        kept.add(max(areas, key=areas.get))
    changed = True
    while changed:
        changed = False
        kept_mask = np.isin(labels, list(kept)).astype(np.uint8)
        dist = cv2.distanceTransform((1 - kept_mask), cv2.DIST_L2, 5)
        for i, _area in areas.items():
            if i in kept:
                continue
            ys, xs = np.where(labels == i)
            if ys.size == 0:
                continue
            if float(dist[ys, xs].min()) <= link_px:
                kept.add(i)
                changed = True
    keep_mask = np.isin(labels, list(kept))
    removed = int(np.sum(opaque & ~keep_mask))
    arr[:, :, 3] = np.where(keep_mask, arr[:, :, 3], np.uint8(0))
    out = Path(out_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(arr, mode="RGBA").save(out)
    dropped = sorted(i for i in areas if i not in kept)
    print(
        f"wipe-islands kept {len(kept)} dropped {len(dropped)} "
        f"removed {removed} px (min_person_frac={min_person_frac} link_px={link_px}) -> {out}"
    )
    return 0


def run_erase_alpha(cutout_path: str, xyxy: str, out_path: str) -> int:
    box = parse_xyxy(xyxy)
    if box is None:
        return 1
    img = _open_rgba(cutout_path)
    if img is None:
        return 1
    x1, y1, x2, y2 = _clamp_box(box, img.width, img.height)
    arr = np.array(img)
    before = arr[y1:y2, x1:x2, 3]
    removed = int(np.sum(before >= 128))
    arr[y1:y2, x1:x2, 3] = 0
    out = Path(out_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(arr, mode="RGBA").save(out)
    print(f"erase-alpha {x1},{y1},{x2},{y2} removed {removed} opaque pixels -> {out}")
    return 0


def run_clip_alpha(cutout_path: str, xyxy: str, out_path: str) -> int:
    box = parse_xyxy(xyxy)
    if box is None:
        return 1
    img = _open_rgba(cutout_path)
    if img is None:
        return 1
    x1, y1, x2, y2 = _clamp_box(box, img.width, img.height)
    arr = np.array(img)
    mask = np.zeros(arr.shape[:2], dtype=bool)
    mask[y1:y2, x1:x2] = True
    arr[:, :, 3] = np.where(mask, arr[:, :, 3], 0)
    out = Path(out_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(arr, mode="RGBA").save(out)
    print(f"clipped alpha to {x1},{y1},{x2},{y2} -> {out}")
    return 0


def run_crop_rembg(
    model_name: str, image_path: str, xyxy: str, out_path: str
) -> int:
    box = parse_xyxy(xyxy)
    if box is None:
        return 1
    original = _open_rgba(image_path)
    if original is None:
        return 1
    x1, y1, x2, y2 = _clamp_box(box, original.width, original.height)
    crop = original.crop((x1, y1, x2, y2)).convert("RGB")
    tmp_in = tmp_out = None
    try:
        with tempfile.NamedTemporaryFile(suffix=".png", delete=False) as handle:
            tmp_in = Path(handle.name)
        with tempfile.NamedTemporaryFile(suffix=".png", delete=False) as handle:
            tmp_out = Path(handle.name)
        crop.save(tmp_in)
        code = run_rembg(model_name, str(tmp_in), str(tmp_out))
        if code != 0:
            return code
        piece = Image.open(tmp_out).convert("RGBA")
        canvas = Image.new("RGBA", original.size, (0, 0, 0, 0))
        canvas.paste(piece, (x1, y1), piece)
        alpha = np.array(canvas)[:, :, 3]
        return _save_from_original(original, alpha, out_path)
    finally:
        for path in (tmp_in, tmp_out):
            if path is not None:
                path.unlink(missing_ok=True)


def run_alpha_close(cutout_path: str, out_path: str, radius: int) -> int:
    if radius < 1:
        print("error: --radius must be >= 1", file=sys.stderr)
        return 1
    img = _open_rgba(cutout_path)
    if img is None:
        return 1
    arr = np.array(img)
    kernel = cv2.getStructuringElement(
        cv2.MORPH_ELLIPSE, (radius * 2 + 1, radius * 2 + 1)
    )
    arr[:, :, 3] = cv2.morphologyEx(arr[:, :, 3], cv2.MORPH_CLOSE, kernel)
    out = Path(out_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(arr, mode="RGBA").save(out)
    print(f"alpha-close radius={radius} -> {out}")
    return 0


def run_grow_color(
    image_path: str,
    seed_path: str,
    out_path: str,
    point_raw: str,
    xyxy: str,
    tolerance: float,
) -> int:
    point = parse_point(point_raw)
    box = parse_xyxy(xyxy)
    if point is None or box is None:
        return 1
    original = _open_rgba(image_path)
    seed = _open_rgba(seed_path)
    if original is None or seed is None:
        return 1
    if seed.size != original.size:
        print("error: seed size must match original", file=sys.stderr)
        return 1
    x1, y1, x2, y2 = _clamp_box(box, original.width, original.height)
    px, py = point
    if not (x1 <= px < x2 and y1 <= py < y2):
        print("error: --point must lie inside --xyxy", file=sys.stderr)
        return 1

    bgr = cv2.cvtColor(np.array(original.convert("RGB")), cv2.COLOR_RGB2BGR)
    lab = cv2.cvtColor(bgr, cv2.COLOR_BGR2LAB).astype(np.float32)
    target = lab[py, px]
    alpha = np.array(seed)[:, :, 3].copy()
    visited = np.zeros(alpha.shape, dtype=bool)
    queue: deque[tuple[int, int]] = deque()

    def lab_ok(x: int, y: int) -> bool:
        delta = lab[y, x] - target
        return float(np.sqrt(float(np.dot(delta, delta)))) <= tolerance

    if lab_ok(px, py):
        queue.append((px, py))
        visited[py, px] = True
    for y in range(y1, y2):
        for x in range(x1, x2):
            if alpha[y, x] < 128 or not lab_ok(x, y):
                continue
            if not visited[y, x]:
                queue.append((x, y))
                visited[y, x] = True
    if not queue:
        print(
            "error: no seed-opaque pixels matched --point color; click remaining subject material already in the cutout",
            file=sys.stderr,
        )
        return 1

    added = 0
    while queue:
        x, y = queue.popleft()
        if not lab_ok(x, y):
            continue
        if alpha[y, x] < 255:
            alpha[y, x] = 255
            added += 1
        for nx, ny in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
            if nx < x1 or nx >= x2 or ny < y1 or ny >= y2:
                continue
            if visited[ny, nx]:
                continue
            visited[ny, nx] = True
            queue.append((nx, ny))
    print(f"grow-color added {added} pixels around {px},{py} tol={tolerance}")
    return _save_from_original(original, alpha, out_path)


def _work_size(full_w: int, full_h: int, max_side: int) -> tuple[int, int, float]:
    long_side = max(full_w, full_h)
    if long_side <= max_side:
        return full_w, full_h, 1.0
    scale = max_side / long_side
    return max(1, int(round(full_w * scale))), max(1, int(round(full_h * scale))), scale


def run_work_prep(
    image_path: str, seed_path: str | None, out_dir: str, max_side: int
) -> int:
    if max_side < 256:
        print("error: --max-side must be >= 256", file=sys.stderr)
        return 1
    original = _open_rgba(image_path)
    if original is None:
        return 1
    seed = None
    if seed_path:
        seed = _open_rgba(seed_path)
        if seed is None:
            return 1
        if seed.size != original.size:
            print("error: seed size must match original", file=sys.stderr)
            return 1
    work_w, work_h, scale = _work_size(original.width, original.height, max_side)
    dest = Path(out_dir)
    dest.mkdir(parents=True, exist_ok=True)
    work_orig = original.resize((work_w, work_h), Image.Resampling.LANCZOS).convert("RGB")
    work_orig.save(dest / "work_orig.png")
    if seed is not None:
        seed_a = seed.split()[3].resize((work_w, work_h), Image.Resampling.NEAREST)
        work_rgba = Image.merge("RGBA", (*work_orig.split(), seed_a))
        work_rgba.save(dest / "cutout.png")
    meta = {
        "full_width": original.width,
        "full_height": original.height,
        "work_width": work_w,
        "work_height": work_h,
        "max_side": max_side,
        "scale": scale,
        "seed": bool(seed_path),
    }
    (dest / "work_meta.json").write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")
    print(
        f"work-prep {original.width}x{original.height} -> {work_w}x{work_h} "
        f"scale={scale:.6f} -> {dest}"
    )
    return 0


def run_lift_alpha(
    image_path: str, work_cutout_path: str, out_path: str, meta_path: str
) -> int:
    original = _open_rgba(image_path)
    work = _open_rgba(work_cutout_path)
    if original is None or work is None:
        return 1
    meta_file = Path(meta_path)
    if not meta_file.is_file():
        print(f"error: work_meta.json not found: {meta_file}", file=sys.stderr)
        return 1
    try:
        meta = json.loads(meta_file.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        print(f"error: invalid work_meta.json: {exc}", file=sys.stderr)
        return 1
    full_w = int(meta["full_width"])
    full_h = int(meta["full_height"])
    if (full_w, full_h) != original.size:
        print("error: --image size does not match work_meta full size", file=sys.stderr)
        return 1
    if work.size != (int(meta["work_width"]), int(meta["work_height"])):
        print("error: work cutout size does not match work_meta", file=sys.stderr)
        return 1
    alpha = work.split()[3].resize((full_w, full_h), Image.Resampling.NEAREST)
    code = _save_from_original(original, np.array(alpha), out_path)
    if code == 0:
        print(f"lift-alpha {work.size} -> {full_w}x{full_h} {out_path}")
    return code


def run_paste(image_path: str, piece_path: str, xyxy: str, out_path: str) -> int:
    box = parse_xyxy(xyxy)
    if box is None:
        return 1
    original = _open_rgba(image_path)
    piece = _open_rgba(piece_path)
    if original is None or piece is None:
        return 1
    x1, y1, x2, y2 = _clamp_box(box, original.width, original.height)
    box_w, box_h = x2 - x1, y2 - y1
    if piece.size != (box_w, box_h):
        piece = piece.resize((box_w, box_h), Image.Resampling.NEAREST)
    canvas = Image.new("RGBA", original.size, (0, 0, 0, 0))
    canvas.paste(piece, (x1, y1), piece)
    alpha = np.array(canvas)[:, :, 3]
    code = _save_from_original(original, alpha, out_path)
    if code == 0:
        print(f"paste {box_w}x{box_h} at {x1},{y1} -> {out_path}")
    return code


def _parse_poly(raw: str) -> np.ndarray | None:
    chunks = [c.strip() for c in raw.split(";") if c.strip()]
    pts: list[list[int]] = []
    for chunk in chunks:
        parts = [p.strip() for p in chunk.split(",") if p.strip()]
        if len(parts) != 2:
            print("error: --poly points are x,y separated by semicolons", file=sys.stderr)
            return None
        try:
            pts.append([int(float(parts[0])), int(float(parts[1]))])
        except ValueError:
            print("error: --poly values must be numbers", file=sys.stderr)
            return None
    if len(pts) < 3:
        print("error: --poly needs at least 3 points", file=sys.stderr)
        return None
    return np.array(pts, dtype=np.int32)


def _shift_poly(pts: np.ndarray, origin: str | None) -> np.ndarray | None:
    if not origin:
        return pts
    point = parse_point(origin)
    if point is None:
        return None
    ox, oy = point
    return pts + np.array([ox, oy], dtype=np.int32)


def run_lasso(cutout_path: str, poly: str, out_path: str, origin: str | None = None) -> int:
    pts = _parse_poly(poly)
    if pts is None:
        return 1
    pts = _shift_poly(pts, origin)
    if pts is None:
        return 1
    img = _open_rgba(cutout_path)
    if img is None:
        return 1
    arr = np.array(img)
    mask = np.zeros(arr.shape[:2], dtype=np.uint8)
    cv2.fillPoly(mask, [pts], 255)
    before = arr[:, :, 3] >= 128
    removed = int(np.sum(before & (mask > 0)))
    arr[:, :, 3] = np.where(mask > 0, 0, arr[:, :, 3])
    out = Path(out_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(arr, mode="RGBA").save(out)
    print(f"lasso removed {removed} opaque pixels -> {out}")
    return 0


def run_lasso_preview(
    image_path: str, poly: str, out_path: str, origin: str | None = None
) -> int:
    pts = _parse_poly(poly)
    if pts is None:
        return 1
    pts = _shift_poly(pts, origin)
    if pts is None:
        return 1
    img = _open_rgba(image_path)
    if img is None:
        return 1
    bgr = cv2.cvtColor(np.array(img), cv2.COLOR_RGBA2BGR)
    overlay = bgr.copy()
    cv2.fillPoly(overlay, [pts], (0, 0, 255))
    painted = cv2.addWeighted(overlay, 0.35, bgr, 0.65, 0)
    cv2.polylines(painted, [pts], isClosed=True, color=(0, 255, 0), thickness=2)
    out = Path(out_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    if not cv2.imwrite(str(out), painted):
        print(f"error: could not write {out}", file=sys.stderr)
        return 1
    print(f"lasso-preview {len(pts)} points -> {out}")
    return 0
