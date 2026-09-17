"""Draw a 12-cell overlay. Landscape = 4x3, portrait = 3x4."""

from __future__ import annotations

import json
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont, ImageOps


def grid_layout(width: int, height: int) -> tuple[int, int]:
    if width >= height:
        return 4, 3
    return 3, 4


def cell_boxes(width: int, height: int) -> list[dict[str, int]]:
    cols, rows = grid_layout(width, height)
    cells: list[dict[str, int]] = []
    n = 1
    for row in range(rows):
        for col in range(cols):
            x1 = col * width // cols
            x2 = (col + 1) * width // cols
            y1 = row * height // rows
            y2 = (row + 1) * height // rows
            cells.append({"id": n, "x1": x1, "y1": y1, "x2": x2, "y2": y2})
            n += 1
    return cells


def _load_font(size: int) -> ImageFont.ImageFont:
    for name in ("arial.ttf", "Arial.ttf", "DejaVuSans.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


def run_grid(image_path: str, out_path: str, *, grid_long_side: int = 2048) -> int:
    src = Path(image_path)
    out = Path(out_path)
    if not src.is_file():
        print(f"error: image not found: {src}", file=sys.stderr)
        return 1

    with Image.open(src) as img:
        img = ImageOps.exif_transpose(img)
        full_w, full_h = img.size
        long_side = max(full_w, full_h)
        scale = grid_long_side / long_side
        grid_w = max(1, int(round(full_w * scale)))
        grid_h = max(1, int(round(full_h * scale)))
        work = img.resize((grid_w, grid_h), Image.Resampling.LANCZOS).convert("RGBA")

    cells = cell_boxes(full_w, full_h)
    cols, rows = grid_layout(full_w, full_h)
    overlay = Image.new("RGBA", work.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(overlay)
    big = _load_font(max(28, grid_long_side // 24))
    small = _load_font(max(14, grid_long_side // 64))

    for cell in cells:
        gx1 = cell["x1"] * scale
        gy1 = cell["y1"] * scale
        gx2 = max(gx1 + 1, cell["x2"] * scale - 1)
        gy2 = max(gy1 + 1, cell["y2"] * scale - 1)
        draw.rectangle([(gx1, gy1), (gx2, gy2)], outline=(255, 220, 0, 230), width=3)
        label = str(cell["id"])
        bbox = draw.textbbox((0, 0), label, font=big)
        tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
        cx = (gx1 + gx2) / 2
        cy = (gy1 + gy2) / 2
        draw.text((cx - tw / 2 + 1, cy - th / 2 + 1), label, fill=(0, 0, 0, 180), font=big)
        draw.text((cx - tw / 2, cy - th / 2), label, fill=(255, 255, 255, 240), font=big)
        xyxy = f'{cell["x1"]},{cell["y1"]} {cell["x2"]},{cell["y2"]}'
        draw.text((gx1 + 6, gy1 + 6), xyxy, fill=(255, 255, 0, 230), font=small)

    composed = Image.alpha_composite(work, overlay).convert("RGB")
    out.parent.mkdir(parents=True, exist_ok=True)
    composed.save(out)

    meta_path = out.with_name(f"{out.stem}_meta.json")
    meta = {
        "full_width": full_w,
        "full_height": full_h,
        "grid_long_side": grid_long_side,
        "scale": scale,
        "cols": cols,
        "rows": rows,
        "cells": cells,
    }
    meta_path.write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")
    print(f"grid {cols}x{rows} -> {out}")
    return 0
