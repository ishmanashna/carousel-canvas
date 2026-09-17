"""Propose missing-subject regions from the person-cutter mask; optional auto-fill."""

from __future__ import annotations

import json
import sys
from pathlib import Path

import cv2
import numpy as np
from PIL import Image, ImageDraw

from compose_cmd import run_clip_alpha, run_union
from sam_cmd import run_sam_box

ALPHA_KEEP = 16
MIN_HOLE_FRAC = 0.0008
MIN_HOLE_PX = 350
MAX_HOLE_FRAC = 0.03
PAD_FRAC = 0.12
TOUCH_KERNEL = 31
MIN_TOUCH_PX = 80


def _alpha(path: str) -> np.ndarray | None:
    p = Path(path)
    if not p.is_file():
        print(f"error: file not found: {p}", file=sys.stderr)
        return None
    with Image.open(p) as img:
        return np.array(img.convert("RGBA"))[:, :, 3]


def _largest_person(alpha: np.ndarray) -> np.ndarray:
    mask = (alpha >= ALPHA_KEEP).astype(np.uint8) * 255
    n, labels, stats, _ = cv2.connectedComponentsWithStats(mask, connectivity=8)
    if n <= 1:
        return mask
    areas = stats[1:, cv2.CC_STAT_AREA]
    idx = 1 + int(np.argmax(areas))
    return np.where(labels == idx, 255, 0).astype(np.uint8)


def _hull_holes(person: np.ndarray) -> np.ndarray:
    contours, _ = cv2.findContours(person, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
    hull = np.zeros_like(person)
    if not contours:
        return hull
    biggest = max(contours, key=cv2.contourArea)
    poly = cv2.convexHull(biggest)
    cv2.fillConvexPoly(hull, poly, 255)
    holes = cv2.bitwise_and(hull, cv2.bitwise_not(person))
    kernel = np.ones((5, 5), np.uint8)
    return cv2.morphologyEx(holes, cv2.MORPH_OPEN, kernel)


def _candidates(person: np.ndarray, holes: np.ndarray) -> list[dict]:
    h, w = person.shape
    min_area = max(MIN_HOLE_PX, int(MIN_HOLE_FRAC * h * w))
    max_area = int(MAX_HOLE_FRAC * h * w)
    n, _labels, stats, cents = cv2.connectedComponentsWithStats(holes, connectivity=8)
    items: list[dict] = []
    for i in range(1, n):
        area = int(stats[i, cv2.CC_STAT_AREA])
        if area < min_area or area > max_area:
            continue
        x = int(stats[i, cv2.CC_STAT_LEFT])
        y = int(stats[i, cv2.CC_STAT_TOP])
        bw = int(stats[i, cv2.CC_STAT_WIDTH])
        bh = int(stats[i, cv2.CC_STAT_HEIGHT])
        if bw > 0.5 * w and bh > 0.35 * h:
            continue
        pad = max(8, int(PAD_FRAC * max(bw, bh)))
        x1 = max(0, x - pad)
        y1 = max(0, y - pad)
        x2 = min(w, x + bw + pad)
        y2 = min(h, y + bh + pad)
        cx = int(round(float(cents[i][0])))
        cy = int(round(float(cents[i][1])))
        ex, ey = x1 + 4, y1 + 4
        if person[min(h - 1, ey), min(w - 1, ex)] > 0:
            ex, ey = min(w - 5, x2 - 4), min(h - 5, y2 - 4)
        items.append(
            {
                "id": len(items),
                "xyxy": [x1, y1, x2, y2],
                "area": area,
                "include": [cx, cy],
                "exclude": [[ex, ey]],
            }
        )
    items.sort(key=lambda c: int(c["area"]), reverse=True)
    for i, item in enumerate(items):
        item["id"] = i
    return items


def _write_overlay(cutout_path: str, candidates: list[dict], overlay_path: str) -> None:
    with Image.open(cutout_path) as img:
        overlay = img.convert("RGBA")
    draw = ImageDraw.Draw(overlay)
    colors = [(255, 64, 64, 255), (64, 220, 64, 255), (64, 140, 255, 255), (255, 220, 64, 255)]
    for item in candidates:
        x1, y1, x2, y2 = (int(v) for v in item["xyxy"])
        color = colors[int(item["id"]) % len(colors)]
        draw.rectangle([x1, y1, x2, y2], outline=color, width=4)
        cx, cy = item["include"]
        draw.ellipse([cx - 6, cy - 6, cx + 6, cy + 6], fill=color)
        draw.text((x1 + 6, y1 + 6), f"gap {item['id']}", fill=color)
    out = Path(overlay_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    overlay.save(out)


def run_propose(cutout_png: str, out_json: str, overlay_png: str) -> int:
    alpha = _alpha(cutout_png)
    if alpha is None:
        return 1
    person = _largest_person(alpha)
    holes = _hull_holes(person)
    candidates = _candidates(person, holes)
    payload = {
        "cutout": cutout_png,
        "candidate_count": len(candidates),
        "candidates": candidates,
    }
    dest = Path(out_json)
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    _write_overlay(cutout_png, candidates, overlay_png)
    print(f"propose {len(candidates)} gap(s) -> {dest}")
    return 0


def _donor_touches_person(seed_alpha: np.ndarray, donor_path: str) -> bool:
    donor = _alpha(donor_path)
    if donor is None or donor.shape != seed_alpha.shape:
        return False
    seed = (seed_alpha >= ALPHA_KEEP).astype(np.uint8)
    don = (donor >= ALPHA_KEEP).astype(np.uint8)
    kernel = np.ones((TOUCH_KERNEL, TOUCH_KERNEL), np.uint8)
    dilated = cv2.dilate(seed, kernel)
    return int((don & dilated).sum()) >= MIN_TOUCH_PX


def run_fill_gaps(
    image_path: str,
    cutout_png: str,
    out_png: str,
    max_candidates: int,
) -> int:
    if max_candidates < 1:
        print("error: --max must be >= 1", file=sys.stderr)
        return 1
    cutout = Path(cutout_png)
    out_dir = cutout.parent
    json_path = out_dir / "candidates.json"
    overlay_path = out_dir / "gaps_overlay.png"
    code = run_propose(str(cutout), str(json_path), str(overlay_path))
    if code != 0:
        return code
    payload = json.loads(json_path.read_text(encoding="utf-8"))
    candidates = list(payload.get("candidates") or [])[:max_candidates]
    dest = Path(out_png)
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.resolve() != cutout.resolve():
        Image.open(cutout).save(dest)
    if not candidates:
        print("fill-gaps: no hull gaps; leaving cutout as-is")
        return 0

    accepted = 0
    for item in candidates:
        xyxy = ",".join(str(int(v)) for v in item["xyxy"])
        prompts = {
            "sam_prompt": [
                {"type": "point", "data": [int(item["include"][0]), int(item["include"][1])], "label": 1},
                {
                    "type": "point",
                    "data": [int(item["exclude"][0][0]), int(item["exclude"][0][1])],
                    "label": 0,
                },
            ]
        }
        prompts_path = out_dir / f"prompts_gap{item['id']}.json"
        prompts_path.write_text(json.dumps(prompts, indent=2) + "\n", encoding="utf-8")
        donor_path = out_dir / f"donor_gap{item['id']}.png"
        seed_alpha = _alpha(str(dest))
        if seed_alpha is None:
            return 1
        sam_code = run_sam_box(xyxy, str(prompts_path), image_path, str(donor_path))
        if sam_code != 0:
            print(f"fill-gaps: gap {item['id']} sam-box failed ({sam_code}); skip")
            continue
        clip_code = run_clip_alpha(str(donor_path), xyxy, str(donor_path))
        if clip_code != 0:
            continue
        if not _donor_touches_person(seed_alpha, str(donor_path)):
            print(f"fill-gaps: gap {item['id']} donor does not touch person; skip")
            continue
        union_code = run_union(image_path, str(dest), str(donor_path), str(dest))
        if union_code != 0:
            return union_code
        accepted += 1
        print(f"fill-gaps: accepted gap {item['id']} xyxy={xyxy}")
    print(f"fill-gaps accepted {accepted}/{len(candidates)}")
    return 0
