"""Cheap token composes to pick a layout seed before full-res photo export (mural v2)."""

from __future__ import annotations

import logging
import random

from app.io import parse_color
from app.strip.composer import compose_strip_wide

logger = logging.getLogger(__name__)

MURAL_V2_LAYOUT_RETRY_ATTEMPTS = 5
TOKEN_COMPOSE_SCALE = 0.25
LAYOUT_RETRY_SEED_STEP = 1_000_003


def strip_layout_token_retry_enabled(template_id: str, card_edge: str) -> bool:
    return template_id == "strip_mural_v2" and card_edge == "borderless"


def score_token_strip_layout(
    wide_rgb,
    *,
    canvas_w: int,
    canvas_h: int,
    slice_w: int,
    slice_h: int,
    slice_count: int,
    overlap_px: int,
    bg_rgb: tuple[int, int, int],
    bg_tol: int = 48,
    quant: int = 18,
) -> float:
    """
    Higher is better: reward distinct token colors per slice; penalize one color dominating a slice.
    ``wide_rgb`` may be downscaled vs full canvas; geometry is inferred from actual pixel size.
    """
    from PIL import Image

    im = wide_rgb.convert("RGB")
    iw, ih = im.size
    if iw < 8 or ih < 8:
        return -1e9
    scale_x = iw / float(max(1, canvas_w))
    sw = max(2, int(round(slice_w * scale_x)))
    sh = min(ih, max(2, int(round(slice_h * (ih / float(max(1, canvas_h)))))))
    stride = max(1, sw // 64)

    def bg_dist(px: tuple[int, ...]) -> int:
        return abs(px[0] - bg_rgb[0]) + abs(px[1] - bg_rgb[1]) + abs(px[2] - bg_rgb[2])

    total_score = 0.0
    for k in range(int(slice_count)):
        x0 = int(round(k * (slice_w - overlap_px) * scale_x))
        if x0 + sw > iw:
            x0 = max(0, iw - sw)
        crop = im.crop((x0, 0, min(iw, x0 + sw), sh))
        cw_, ch_ = crop.size
        pix = crop.load()
        buckets: dict[tuple[int, int, int], int] = {}
        fg = 0
        for y in range(0, ch_, stride):
            for x in range(0, cw_, stride):
                p = pix[x, y]
                if bg_dist(p) <= bg_tol:
                    continue
                key = (p[0] // quant, p[1] // quant, p[2] // quant)
                buckets[key] = buckets.get(key, 0) + 1
                fg += 1
        if fg < 12:
            total_score -= 40.0
            continue
        distinct = len(buckets)
        dom = max(buckets.values()) / float(fg)
        slice_sc = float(min(40, distinct))
        if distinct < 5:
            slice_sc -= 18.0
        if dom > 0.88:
            slice_sc -= 22.0
        total_score += slice_sc
    return total_score


def underfill_rng_from_layout_seed(layout_seed: int) -> random.Random:
    """Match GUI underfill shuffle for a given effective layout seed."""
    return random.Random((int(layout_seed) & 0xFFFFFFFF) ^ 0x1B873F91)


def pick_best_layout_seed_with_token_retry(
    fills: list,
    template,
    base_layout_seed: int | None,
    *,
    card_edge: str,
    attempts: int = MURAL_V2_LAYOUT_RETRY_ATTEMPTS,
    token_scale: float = TOKEN_COMPOSE_SCALE,
) -> int:
    """
    Try ``attempts`` layout seeds (token composes at ``token_scale``); return seed with best score.
    No-op (returns base seed) if retry not enabled for this template/card mode.
    """
    if not strip_layout_token_retry_enabled(template.id, card_edge):
        return 0 if base_layout_seed is None else int(base_layout_seed)
    bs = 0 if base_layout_seed is None else int(base_layout_seed)
    bg = parse_color(template.background)
    best_seed = bs
    best_score = -1e18
    for i in range(max(1, int(attempts))):
        cand = bs + i * LAYOUT_RETRY_SEED_STEP
        img = compose_strip_wide(
            fills,
            template,
            layout_seed=cand,
            compose_scale=float(token_scale),
            card_edge="borderless",
            fill_mode="tokens",
            underfill_paths=None,
        )
        sc = score_token_strip_layout(
            img,
            canvas_w=int(template.canvas_width),
            canvas_h=int(template.canvas_height),
            slice_w=int(template.slice_width),
            slice_h=int(template.slice_height),
            slice_count=int(template.slice_count),
            overlap_px=int(template.overlap_px),
            bg_rgb=bg,
        )
        if sc > best_score:
            best_score = sc
            best_seed = cand
    if best_seed != bs:
        logger.info("strip_mural_v2: layout_seed %s -> %s (token retry, score=%.1f)", bs, best_seed, best_score)
    return best_seed
