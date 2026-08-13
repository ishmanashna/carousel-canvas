"""Organic polaroid layout: hero in slice 1, per-slice anchors, wild bleed + upside-down."""

from __future__ import annotations

import math
import random

from app.strip.template_data import StripSlotDef, StripTemplate


def _balanced_tilt_pool(rng: random.Random, count: int) -> list[float]:
    if count <= 0:
        return []
    n_pos = count // 2
    n_neg = count - n_pos
    angles: list[float] = []

    def draw_mag() -> float:
        tier = rng.random()
        if tier < 0.22:
            return rng.uniform(2.4, 5.2)
        if tier < 0.62:
            return rng.uniform(5.0, 10.5)
        return rng.uniform(9.0, 16.2)

    for _ in range(n_pos):
        angles.append(draw_mag())
    for _ in range(n_neg):
        angles.append(-draw_mag())
    flats = max(1, min(4, count // 5))
    for _ in range(flats):
        angles[rng.randrange(len(angles))] = rng.uniform(-2.8, 2.8)
    rng.shuffle(angles)
    for _ in range(max(8, count * 2)):
        for k in range(len(angles) - 1):
            if angles[k] * angles[k + 1] > 0 and abs(angles[k]) > 2.5 and abs(angles[k + 1]) > 2.5:
                if rng.random() < 0.42:
                    angles[k], angles[k + 1] = angles[k + 1], angles[k]
                break
    return angles


def _ensure_slice_coverage(
    rng: random.Random,
    xs: list[int],
    ys: list[int],
    *,
    card_w: int,
    card_h: int,
    slice_w: int,
    slice_count: int,
    ch: int,
    gy_lo: int,
    gy_hi: int,
    wild_indices: list[int],
    min_px: int = 160,
) -> None:
    """Nudge a wild card into any slice with too little horizontal overlap (no empty columns)."""
    n = len(xs)

    def max_horiz_in_slice(k: int) -> int:
        lo = k * slice_w
        hi = (k + 1) * slice_w
        best = 0
        for i in range(n):
            ix0 = max(lo, xs[i])
            ix1 = min(hi, xs[i] + card_w)
            if ix1 > ix0 and ys[i] < ch and ys[i] + card_h > 0:
                best = max(best, ix1 - ix0)
        return best

    for k in range(slice_count):
        if max_horiz_in_slice(k) >= min_px:
            continue
        if not wild_indices:
            break
        pick = rng.choice(wild_indices)
        margin = rng.randint(18, 50)
        lo = k * slice_w + margin
        hi = (k + 1) * slice_w - card_w - margin
        if hi < lo:
            hi = lo
        xs[pick] = rng.randint(lo, hi) if hi >= lo else lo
        ys[pick] = int(max(gy_lo, min(gy_hi, ys[pick] + rng.randint(-35, 35))))


def _separate_wild_centers(
    rng: random.Random,
    xs: list[int],
    ys: list[int],
    indices: list[int],
    *,
    card_w: int,
    card_h: int,
    x_lo: int,
    x_hi: int,
    gy_lo: int,
    gy_hi: int,
    min_center_dist: float,
    iters: int = 12,
) -> None:
    """Push non-bleed wild cards apart slightly to reduce accidental stacks."""
    if len(indices) < 2:
        return
    for _ in range(iters):
        rng.shuffle(indices)
        for a in indices:
            axc = xs[a] + card_w * 0.5
            ayc = ys[a] + card_h * 0.5
            for b in indices:
                if b <= a:
                    continue
                bxc = xs[b] + card_w * 0.5
                byc = ys[b] + card_h * 0.5
                dx, dy = axc - bxc, ayc - byc
                d = math.hypot(dx, dy)
                if d <= 1e-3 or d >= min_center_dist:
                    continue
                push = (min_center_dist - d) * 0.38
                ux = dx / d * push
                uy = dy / d * push
                xs[a] = int(round(xs[a] + ux))
                ys[a] = int(round(ys[a] + uy))
                xs[b] = int(round(xs[b] - ux))
                ys[b] = int(round(ys[b] - uy))
                xs[a] = max(x_lo, min(x_hi, xs[a]))
                xs[b] = max(x_lo, min(x_hi, xs[b]))
                ys[a] = int(max(gy_lo, min(gy_hi, ys[a])))
                ys[b] = int(max(gy_lo, min(gy_hi, ys[b])))


def resolve_organic_polaroid_slots(template: StripTemplate, seed: int) -> tuple[StripSlotDef, ...]:
    slots = template.slots
    n = len(slots)
    if n == 0:
        return slots
    cw, ch = template.canvas_width, template.canvas_height
    slice_w = template.slice_width
    slice_count = template.slice_count
    card_w, card_h = slots[0].w, slots[0].h
    for s in slots:
        if s.w != card_w or s.h != card_h:
            raise ValueError("organic_polaroid expects uniform slot width/height")

    rng = random.Random((seed ^ 0xC0FFEE71) & 0xFFFFFFFF)

    xs = [0] * n
    ys = [0] * n
    rots = [0.0] * n

    pad_y_bot = max(88, int(card_h * 0.14))
    pad_y_top = max(40, int(card_h * 0.05))
    gy_lo = max(8, pad_y_top)
    gy_hi = ch - card_h - pad_y_bot
    if gy_hi < gy_lo:
        gy_lo, gy_hi = 0, max(0, ch - card_h)

    # --- Slot 0: centered in first slice, fully inside slice 0 × canvas, gentle tilt ---
    hm = 22
    cx = slice_w / 2.0
    cy = ch / 2.0
    xs[0] = int(round(cx - card_w / 2.0 + rng.uniform(-10, 10)))
    ys[0] = int(round(cy - card_h / 2.0 + rng.uniform(-14, 14)))
    xs[0] = max(hm, min(slice_w - card_w - hm, xs[0]))
    ys[0] = max(hm, min(ch - card_h - hm, ys[0]))
    rots[0] = rng.uniform(-2.6, 2.6)

    hero_cy = ys[0] + card_h * 0.5

    # --- Slots 1..9: one anchor per carousel column; early columns hug hero vertical band ---
    n_anchor = min(9, n - 1)
    for k in range(1, 1 + n_anchor):
        lo = k * slice_w + rng.randint(22, 48)
        hi = (k + 1) * slice_w - card_w - rng.randint(22, 48)
        if hi < lo:
            hi = lo
        xs[k] = rng.randint(lo, hi) if hi >= lo else lo
        if k <= 5:
            if rng.random() < 0.68:
                half = rng.randint(55, 130)
                y_target = int(hero_cy - card_h * 0.5 + rng.randint(-half, half))
                ys[k] = int(max(gy_lo, min(gy_hi, y_target)))
            else:
                ys[k] = rng.randint(gy_lo, gy_hi)
        else:
            ys[k] = rng.randint(gy_lo, gy_hi)
        base = rng.uniform(5.0, 10.2) * (-1 if k % 2 == 0 else 1)
        rots[k] = base + rng.uniform(-2.0, 2.0)
        if k <= 3:
            rots[k] = max(-11.0, min(11.0, rots[k] * 0.82 + rng.uniform(-1.2, 1.2)))
        else:
            rots[k] = max(-14.0, min(14.0, rots[k]))

    wild = [i for i in range(n) if i > n_anchor]
    upside: set[int] = set()
    if len(wild) >= 2:
        upside = set(rng.sample(wild, 2))

    bleed_eligible = [i for i in wild if i not in upside]
    n_bleed = min(6, max(4, len(bleed_eligible) * 4 // 7))
    bleed_set: set[int] = set()
    if bleed_eligible:
        bleed_set = set(rng.sample(bleed_eligible, min(n_bleed, len(bleed_eligible))))

    tilt_n = len(wild) - len(upside)
    tilt_wild = _balanced_tilt_pool(rng, tilt_n)
    tw = 0
    x_lo = int(card_w * 0.04)
    x_hi = cw - card_w - int(card_w * 0.04)

    for i in wild:
        if i in upside:
            rots[i] = 180.0 + rng.uniform(-3.2, 3.2)
        else:
            rots[i] = tilt_wild[tw] + rng.uniform(-1.2, 1.2)
            tw += 1
            rots[i] = max(-16.5, min(16.5, rots[i]))

    wild_strat = [i for i in wild if i not in bleed_set]
    n_strat = len(wild_strat)
    n_bands = max(5, min(9, (n_strat + 2) // 2))
    band_order = list(range(n_bands))
    rng.shuffle(band_order)

    for j, i in enumerate(sorted(wild_strat)):
        xs[i] = rng.randint(x_lo, max(x_lo, x_hi))
        bi = band_order[j % n_bands]
        span = gy_hi - gy_lo + 1
        slice_y = gy_lo + (bi * span) // max(1, n_bands)
        slice_y2 = gy_lo + ((bi + 1) * span) // max(1, n_bands)
        slice_y2 = max(slice_y + int(card_h * 0.22), slice_y2)
        ys[i] = rng.randint(slice_y, max(slice_y, slice_y2 - card_h))
        ys[i] = int(max(gy_lo, min(gy_hi, ys[i])))
        if rng.random() < 0.34:
            bump = rng.choice((-1, 1)) * rng.randint(10, 44)
            ys[i] = int(max(gy_lo, min(gy_hi, ys[i] + bump)))

    for i in bleed_set:
        xs[i] = rng.randint(x_lo, max(x_lo, x_hi))
        y_top = -int(card_h * 0.32)
        y_bot = ch - int(card_h * 0.36)
        ys[i] = rng.randint(y_top, max(y_top, y_bot))

    wild_non_bleed = [i for i in wild if i not in bleed_set]
    min_d = 0.36 * float(min(card_w, card_h))
    _separate_wild_centers(
        rng,
        xs,
        ys,
        wild_non_bleed,
        card_w=card_w,
        card_h=card_h,
        x_lo=x_lo,
        x_hi=x_hi,
        gy_lo=gy_lo,
        gy_hi=gy_hi,
        min_center_dist=min_d,
        iters=11,
    )

    _ensure_slice_coverage(
        rng,
        xs,
        ys,
        card_w=card_w,
        card_h=card_h,
        slice_w=slice_w,
        slice_count=slice_count,
        ch=ch,
        gy_lo=gy_lo,
        gy_hi=gy_hi,
        wild_indices=wild,
    )

    out: list[StripSlotDef] = []
    for i, s in enumerate(slots):
        out.append(
            StripSlotDef(
                xs[i],
                ys[i],
                s.w,
                s.h,
                rotation_deg=rots[i],
                z_index=s.z_index,
                fit=s.fit,
                prefer_portrait=s.prefer_portrait,
                prefer_landscape=s.prefer_landscape,
                polaroid=s.polaroid,
                horizontal_center_band_frac=s.horizontal_center_band_frac,
                source_trim_left_frac=s.source_trim_left_frac,
                cover_height_first=s.cover_height_first,
            )
        )
    return tuple(out)
