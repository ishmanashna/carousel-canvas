"""Detect template-colored (beige) gaps on a composed strip and paste extra photo layers."""

from __future__ import annotations

import random
from collections.abc import Callable
from pathlib import Path

from PIL import Image

from app.image_ops import process_image_panned


def _color_distance_sum(rgb: tuple[int, ...], bg: tuple[int, int, int]) -> int:
    r, g, b = rgb[:3]
    br, bg_, bb = bg
    return abs(r - br) + abs(g - bg_) + abs(b - bb)


def beige_area_ratio(
    img: Image.Image,
    bg: tuple[int, int, int],
    tolerance: int,
    *,
    max_width: int = 400,
    margin_cells: int = 1,
) -> float:
    """Approximate fraction of interior pixels matching background within tolerance."""
    im = img.convert("RGB")
    small = im
    w0, h0 = im.size
    if w0 > max_width:
        nh = max(1, int(round(h0 * max_width / w0)))
        small = im.resize((max_width, nh), Image.Resampling.BOX)
    sw, sh = small.size
    pix = small.load()
    mx = max(0, min(sw // 8, margin_cells * (sw // 24)))
    my = max(0, min(sh // 8, margin_cells * (sh // 24)))
    inner = 0
    beige = 0
    for y in range(my, max(my + 1, sh - my)):
        for x in range(mx, max(mx + 1, sw - mx)):
            inner += 1
            if _color_distance_sum(pix[x, y], bg) <= tolerance:
                beige += 1
    return beige / max(1, inner)


def _downscale(im: Image.Image, max_w: int) -> Image.Image:
    w, h = im.size
    if w <= max_w:
        return im
    nh = max(1, int(round(h * max_w / w)))
    return im.resize((max_w, nh), Image.Resampling.BOX)


def _pick_gap_anchor(
    im_rgb: Image.Image,
    bg: tuple[int, int, int],
    tolerance: int,
    gw: int = 22,
    gh: int = 5,
) -> tuple[int, int] | None:
    sw, sh = im_rgb.size
    pix = im_rgb.load()
    buckets: dict[tuple[int, int], int] = {}
    sx = sw / float(gw)
    sy = sh / float(gh)
    min_hits = max(16, (sw * sh) // 6000)
    for y in range(sh):
        for x in range(sw):
            if _color_distance_sum(pix[x, y], bg) <= tolerance:
                bx = min(gw - 1, int(x / sx))
                by = min(gh - 1, int(y / sy))
                key = (bx, by)
                buckets[key] = buckets.get(key, 0) + 1
    if not buckets:
        return None
    best_k = max(buckets, key=buckets.__getitem__)
    if buckets[best_k] < min_hits:
        return None
    bx, by = best_k
    cx = int((bx + 0.5) * sx)
    cy = int((by + 0.5) * sy)
    return cx, cy


def apply_beige_gap_fill(
    base_rgb: Image.Image,
    *,
    bg: tuple[int, int, int],
    paths: list[Path],
    layout_seed: int | None,
    max_layers: int,
    tolerance: int,
    stop_ratio: float,
    panned_loader: Callable[[Path, int, int], Image.Image | None] | None = None,
) -> Image.Image:
    """Repeatedly find a dense beige tile and paste a rotated cover card until ratio is low or cap hit."""
    if max_layers <= 0 or not paths:
        return base_rgb
    load: Callable[[Path, int, int], Image.Image | None]
    if panned_loader is not None:
        load = panned_loader
    else:

        def load(p: Path, bw: int, bh: int) -> Image.Image | None:
            return process_image_panned(
                p, bw, bh, "mixed", 0.0, 0.0, flip_h=False, grayscale=False
            )

    seed = 0 if layout_seed is None else int(layout_seed)
    rng = random.Random(seed + 51_023)
    cw, ch = base_rgb.size
    small_ref = _downscale(base_rgb, 540)
    scale = cw / float(max(1, small_ref.size[0]))

    work = base_rgb.convert("RGBA")
    for _ in range(max_layers):
        flat = work.convert("RGB")
        if beige_area_ratio(flat, bg, tolerance) <= stop_ratio:
            break
        small = _downscale(flat, 540)
        anchor = _pick_gap_anchor(small, bg, tolerance)
        if anchor is None:
            break
        ax_s, ay_s = anchor
        cx_full = int(round(ax_s * scale))
        cy_full = int(round(ay_s * scale))
        if cw >= 9000:
            bw = max(380, min(920, int(cw * 0.086)))
            bh = max(300, min(720, int(ch * 0.5)))
        else:
            bw = max(260, min(720, int(cw * 0.078)))
            bh = max(220, min(620, int(ch * 0.44)))
        sx0 = max(0, min(cw - bw, cx_full - bw // 2 + rng.randint(-48, 48)))
        sy0 = max(0, min(ch - bh, cy_full - bh // 2 + rng.randint(-42, 42)))
        p = paths[rng.randrange(len(paths))]
        img = load(p, bw, bh)
        if img is None:
            continue
        img = img.convert("RGBA")
        rot = rng.uniform(-2.9, 2.9)
        if abs(rot) > 1e-6:
            img = img.rotate(
                -rot,
                expand=True,
                resample=Image.Resampling.BICUBIC,
                fillcolor=(0, 0, 0, 0),
            )
        px = int(round(sx0 + bw / 2.0 - img.width / 2.0))
        py = int(round(sy0 + bh / 2.0 - img.height / 2.0))
        work.paste(img, (px, py), img)

    out = Image.new("RGB", work.size, bg)
    out.paste(work, (0, 0), work.split()[3])
    return out
