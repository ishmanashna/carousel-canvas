"""Compose cutout on a checkerboard preview."""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
from PIL import Image


def _checkerboard(size: tuple[int, int], square: int = 32) -> Image.Image:
    width, height = size
    rows = (height + square - 1) // square
    cols = (width + square - 1) // square
    grid = np.zeros((rows, cols), dtype=np.uint8)
    grid[0::2, 0::2] = 1
    grid[1::2, 1::2] = 1
    pattern = np.kron(grid, np.ones((square, square), dtype=np.uint8))[:height, :width]
    light = np.full((height, width, 3), 220, dtype=np.uint8)
    dark = np.full((height, width, 3), 160, dtype=np.uint8)
    rgb = np.where(pattern[..., None], light, dark)
    return Image.fromarray(rgb, mode="RGB")


def _resize_long_side(img: Image.Image, max_long: int) -> Image.Image:
    w, h = img.size
    long_side = max(w, h)
    if long_side <= max_long:
        return img
    scale = max_long / long_side
    new_w = max(1, int(round(w * scale)))
    new_h = max(1, int(round(h * scale)))
    return img.resize((new_w, new_h), Image.Resampling.LANCZOS)


def compose_checker(
    cutout_path: str,
    out_path: str,
    *,
    full_res: bool = False,
    preview_long_side: int = 1600,
) -> int:
    src = Path(cutout_path)
    out = Path(out_path)
    if not src.is_file():
        print(f"error: cutout not found: {src}", file=sys.stderr)
        return 1

    with Image.open(src) as cutout:
        cutout = cutout.convert("RGBA")
        board = _checkerboard(cutout.size)
        board.paste(cutout, (0, 0), cutout)
        if full_res:
            out.parent.mkdir(parents=True, exist_ok=True)
            board.save(out)
            preview_path = out.with_name(f"{out.stem}_preview{out.suffix}")
            preview = _resize_long_side(board, preview_long_side)
            preview.save(preview_path)
        else:
            preview = _resize_long_side(board, preview_long_side)
            out.parent.mkdir(parents=True, exist_ok=True)
            preview.save(out)
            preview_path = out.with_name(f"{out.stem}_preview{out.suffix}")
            if preview_path != out:
                preview.save(preview_path)
    return 0
