"""Plan extra photos on the background layer where template color still shows (mural v2)."""

from __future__ import annotations

import random

from PIL import Image, ImageDraw

from app.strip.gap_fill import _downscale, _pick_gap_anchor


def _underfill_box_size(cw: int, ch: int, *, fine: bool) -> tuple[int, int]:
    if cw >= 9000:
        if fine:
            bw = max(320, min(760, int(cw * 0.072)))
            bh = max(240, min(620, int(ch * 0.44)))
        else:
            bw = max(420, min(980, int(cw * 0.092)))
            bh = max(320, min(760, int(ch * 0.54)))
    else:
        if fine:
            bw = max(240, min(620, int(cw * 0.068)))
            bh = max(200, min(520, int(ch * 0.38)))
        else:
            bw = max(280, min(760, int(cw * 0.082)))
            bh = max(220, min(640, int(ch * 0.46)))
    return bw, bh


def _pick_edge_anchor(
    small: Image.Image,
    bg: tuple[int, int, int],
    tolerance: int,
) -> tuple[int, int] | None:
    """
    Prefer residual pockets near strip ends without hard-coding right-only behavior.
    We sample both edge bands and pick whichever has denser beige candidates.
    """
    sw, sh = small.size
    if sw < 8 or sh < 8:
        return None
    band = max(1, int(round(sw * 0.23)))
    left = small.crop((0, 0, min(sw, band), sh))
    right = small.crop((max(0, sw - band), 0, sw, sh))
    a_left = _pick_gap_anchor(left, bg, tolerance)
    a_right = _pick_gap_anchor(right, bg, tolerance)
    if a_left is None and a_right is None:
        return None
    if a_right is None:
        return a_left
    if a_left is None:
        return (a_right[0] + (sw - band), a_right[1])

    # Compare local beige hit density around each anchor; pick denser one.
    pix = small.convert("RGB").load()

    def score(cx: int, cy: int) -> int:
        r = 22
        x0, x1 = max(0, cx - r), min(sw - 1, cx + r)
        y0, y1 = max(0, cy - r), min(sh - 1, cy + r)
        s = 0
        for yy in range(y0, y1 + 1):
            for xx in range(x0, x1 + 1):
                p = pix[xx, yy]
                if abs(p[0] - bg[0]) + abs(p[1] - bg[1]) + abs(p[2] - bg[2]) <= tolerance:
                    s += 1
        return s

    lx, ly = a_left
    rx, ry = a_right[0] + (sw - band), a_right[1]
    return (rx, ry) if score(rx, ry) >= score(lx, ly) else (lx, ly)


def plan_background_underfill_boxes(
    base_rgb: Image.Image,
    bg: tuple[int, int, int],
    tolerance: int,
    n: int,
    layout_seed: int | None,
    boost_n: int = 0,
) -> list[tuple[int, int, int, int, float]]:
    """
    After a full strip compose, find up to ``n`` beige-heavy regions and return
    placement boxes (sx, sy, bw, bh) plus a small rotation (deg), full canvas coords.

    Two phases are used naturally (coarse then fine): early boxes are bigger and block wider
    areas; later boxes are smaller and target remaining pockets.
    """
    if n <= 0:
        return []
    seed = 0 if layout_seed is None else int(layout_seed)
    rng = random.Random(seed + 612_903)
    cw, ch = base_rgb.size
    work = base_rgb.convert("RGB").copy()
    placements: list[tuple[int, int, int, int, float]] = []

    total = int(n) + max(0, int(boost_n))
    n_coarse = max(1, int(round(max(1, n) * 0.6)))

    for i in range(total):
        in_boost = i >= int(n)
        fine = i >= n_coarse
        small = _downscale(work, 700 if fine else 620)
        sw, sh = small.size
        anchor = _pick_edge_anchor(small, bg, tolerance) if in_boost else _pick_gap_anchor(
            small, bg, tolerance
        )
        if anchor is None:
            break
        ax_s, ay_s = anchor
        cx_full = int(round(ax_s * cw / max(1, sw)))
        cy_full = int(round(ay_s * ch / max(1, sh)))
        bw, bh = _underfill_box_size(cw, ch, fine=fine)
        jitter_x = 32 if fine else 52
        jitter_y = 26 if fine else 44
        sx0 = max(0, min(cw - bw, cx_full - bw // 2 + rng.randint(-jitter_x, jitter_x)))
        sy0 = max(0, min(ch - bh, cy_full - bh // 2 + rng.randint(-jitter_y, jitter_y)))
        rot = rng.uniform(-1.55, 1.55) if in_boost else (rng.uniform(-1.95, 1.95) if fine else rng.uniform(-2.35, 2.35))
        placements.append((sx0, sy0, bw, bh, rot))

        dr = ImageDraw.Draw(work)
        block_pad = max(bw, bh) // (3 if fine else 2)
        x1 = max(0, cx_full - block_pad)
        y1 = max(0, cy_full - block_pad)
        x2 = min(cw - 1, cx_full + block_pad)
        y2 = min(ch - 1, cy_full + block_pad)
        dr.rectangle([x1, y1, x2, y2], fill=(0, 0, 0))

    return placements


def plan_background_underfill_repeat_boxes(
    base_rgb: Image.Image,
    bg: tuple[int, int, int],
    tolerance: int,
    n: int,
    layout_seed: int | None,
    *,
    beige_tolerance_bonus: int = 0,
) -> list[tuple[int, int, int, int, float]]:
    """
    Third pass: larger repeated-image boxes for stubborn residual pockets.
    ``beige_tolerance_bonus`` widens color matching so near-beige gaps still get targets.
    """
    if n <= 0:
        return []
    seed = 0 if layout_seed is None else int(layout_seed)
    rng = random.Random(seed + 918_221)
    cw, ch = base_rgb.size
    work = base_rgb.convert("RGB").copy()
    placements: list[tuple[int, int, int, int, float]] = []
    tol = int(tolerance) + max(0, int(beige_tolerance_bonus))

    for _ in range(n):
        small = _downscale(work, 760)
        sw, sh = small.size
        anchor = _pick_edge_anchor(small, bg, tol)
        if anchor is None:
            anchor = _pick_gap_anchor(small, bg, tol)
        if anchor is None:
            break
        ax_s, ay_s = anchor
        cx_full = int(round(ax_s * cw / max(1, sw)))
        cy_full = int(round(ay_s * ch / max(1, sh)))
        if cw >= 9000:
            bw = max(720, min(1980, int(cw * 0.178)))
            bh = max(520, min(min(1180, ch - 24), int(ch * 0.86)))
        else:
            bw = max(440, min(1180, int(cw * 0.125)))
            bh = max(340, min(900, int(ch * 0.62)))
        sx0 = max(0, min(cw - bw, cx_full - bw // 2 + rng.randint(-58, 58)))
        sy0 = max(0, min(ch - bh, cy_full - bh // 2 + rng.randint(-44, 44)))
        rot = rng.uniform(-1.15, 1.15)
        placements.append((sx0, sy0, bw, bh, rot))

        dr = ImageDraw.Draw(work)
        pad = int(max(bw, bh) * 0.68)
        x1 = max(0, cx_full - pad)
        y1 = max(0, cy_full - pad)
        x2 = min(cw - 1, cx_full + pad)
        y2 = min(ch - 1, cy_full + pad)
        dr.rectangle([x1, y1, x2, y2], fill=(0, 0, 0))

    return placements


def carousel_tail_band_x_range(
    canvas_width: int,
    slice_width: int,
    slice_count: int,
    overlap_px: int,
    tail_slice_count: int,
) -> tuple[int, int]:
    """
    Horizontal span [x0, x1) on the wide master that corresponds to the last ``tail_slice_count``
    carousel exports (right side). ``x1`` is ``canvas_width``.
    """
    if tail_slice_count <= 0:
        return 0, canvas_width
    ts = min(int(tail_slice_count), int(slice_count))
    if overlap_px == 0:
        stride = int(slice_width)
    else:
        stride = int(slice_width) - int(overlap_px)
    i0 = max(0, int(slice_count) - ts)
    x0 = i0 * stride
    x0 = max(0, min(x0, max(0, canvas_width - 1)))
    return x0, int(canvas_width)


def plan_background_tail_underfill_boxes(
    base_rgb: Image.Image,
    bg: tuple[int, int, int],
    tolerance: int,
    n: int,
    layout_seed: int | None,
    tail_x0: int,
    tail_x1: int,
    *,
    beige_tolerance_bonus: int = 0,
) -> list[tuple[int, int, int, int, float]]:
    """
    Like the repeat pass, but anchors and box centers are biased to the tail band only
    (last carousel slice columns), with wide boxes to cover that strip.
    """
    if n <= 0:
        return []
    cw, ch = base_rgb.size
    x0 = max(0, min(int(tail_x0), cw - 1))
    x1 = max(x0 + 1, min(int(tail_x1), cw))
    tail_w = x1 - x0
    if tail_w < 24:
        return []

    seed = 0 if layout_seed is None else int(layout_seed)
    rng = random.Random(seed + 441_977)
    work = base_rgb.convert("RGB").copy()
    placements: list[tuple[int, int, int, int, float]] = []
    tol = int(tolerance) + max(0, int(beige_tolerance_bonus))

    for _ in range(n):
        band = work.crop((x0, 0, x1, ch))
        small = _downscale(band, 820)
        sw, sh = small.size
        anchor = _pick_gap_anchor(small, bg, tol)
        if anchor is None:
            anchor = _pick_edge_anchor(small, bg, tol)
        if anchor is None:
            break
        ax_s, ay_s = anchor
        cx_band = int(round(ax_s * tail_w / max(1, sw)))
        cy_full = int(round(ay_s * ch / max(1, sh)))
        cx_full = x0 + cx_band

        if cw >= 9000:
            bw = max(920, min(2480, int(tail_w * 1.12)))
            bh = max(520, min(min(1220, ch - 16), int(ch * 0.92)))
        else:
            bw = max(520, min(1400, int(tail_w * 1.08)))
            bh = max(360, min(940, int(ch * 0.72)))

        bw = min(bw, cw)
        bh = min(bh, ch)
        # Last-column-only bands: do not paste a box much wider than the tail strip.
        if tail_w <= 1400:
            bw = min(bw, tail_w)
        sx0 = max(x0, min(x1 - bw, cx_full - bw // 2 + rng.randint(-48, 48)))
        sx0 = max(0, min(cw - bw, sx0))
        sy0 = max(0, min(ch - bh, cy_full - bh // 2 + rng.randint(-40, 40)))
        rot = rng.uniform(-1.05, 1.05)
        placements.append((sx0, sy0, bw, bh, rot))

        dr = ImageDraw.Draw(work)
        pad = int(max(bw, bh) * 0.72)
        x1b = max(0, cx_full - pad)
        y1b = max(0, cy_full - pad)
        x2b = min(cw - 1, cx_full + pad)
        y2b = min(ch - 1, cy_full + pad)
        dr.rectangle([x1b, y1b, x2b, y2b], fill=(0, 0, 0))

    return placements
