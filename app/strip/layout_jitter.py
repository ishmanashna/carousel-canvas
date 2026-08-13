"""Seeded layout variation: cover slot reserved, greedy placement into low-overlap areas."""

from __future__ import annotations

import random
import statistics


def _intersection_area(ax: int, ay: int, aw: int, ah: int, bx: int, by: int, bw: int, bh: int) -> float:
    ix0 = max(ax, bx)
    iy0 = max(ay, by)
    ix1 = min(ax + aw, bx + bw)
    iy1 = min(ay + ah, by + bh)
    if ix1 <= ix0 or iy1 <= iy0:
        return 0.0
    return float((ix1 - ix0) * (iy1 - iy0))


def _clone_slot(s, nx: int, ny: int):
    from app.strip.template_data import StripSlotDef

    return StripSlotDef(
        nx,
        ny,
        s.w,
        s.h,
        s.rotation_deg,
        s.z_index,
        s.fit,
        s.prefer_portrait,
        s.polaroid,
    )


def jitter_strip_slots(
    slots: tuple,
    jitter_px: int,
    seed: int,
    canvas_w: int,
    canvas_h: int,
    *,
    cover_slot_index: int | None = None,
    cover_max_jitter: int = 12,
    flagship_slot_index: int | None = None,
    flagship_max_jitter: int = 18,
    flagship_rim_indices: tuple[int, ...] = (),
    min_y_spread: float = 70.0,
    max_attempts: int = 48,
    vertical_bleed_top: int = 200,
    vertical_bleed_bottom: int = 220,
    horizontal_bleed: int = 520,
) -> tuple:
    if jitter_px <= 0 or not slots:
        return slots

    n = len(slots)

    def clamp_xy(s, nx: int, ny: int) -> tuple[int, int]:
        nx = max(
            -min(horizontal_bleed, s.w),
            min(canvas_w - s.w + min(horizontal_bleed, s.w // 2), nx),
        )
        ny = max(-vertical_bleed_top, min(canvas_h - s.h + vertical_bleed_bottom, ny))
        return nx, ny

    # --- Flagship last: jitter main board only, then hero + 1–2 rim cards anchored to it. ---
    if flagship_slot_index is not None and 0 <= flagship_slot_index < n:
        from app.strip.template_data import StripSlotDef

        defer = set(flagship_rim_indices) | {flagship_slot_index}
        indexed = [(i, slots[i]) for i in range(n) if i not in defer]
        m = len(indexed)
        rank_by: dict[int, int] = {}
        for rank, (orig_i, _) in enumerate(sorted(indexed, key=lambda t: t[1].y + t[1].h / 2.0)):
            rank_by[orig_i] = rank
        bias_amp = float(jitter_px) + 36.0

        def score_layout_local(tpl: tuple) -> float:
            cy = [s.y + s.h / 2.0 for s in tpl]
            cx = [s.x + s.w / 2.0 for s in tpl]
            span = max(cy) - min(cy) if cy else 0.0
            ps = statistics.pstdev(cy) if len(cy) >= 2 else 0.0
            if span + ps < 1e-6:
                return -1e9
            pen = 0.0
            dmin = 140.0
            for i in range(len(tpl)):
                for j in range(i + 1, len(tpl)):
                    d = ((cx[i] - cx[j]) ** 2 + (cy[i] - cy[j]) ** 2) ** 0.5
                    if d < dmin:
                        pen += dmin - d
            raw = span + 0.5 * ps - 0.28 * pen
            if ps < min_y_spread:
                raw -= (min_y_spread - ps) * 2.5
            return raw

        best_cand: list | None = None
        best_s = -1e18
        for attempt in range(max_attempts):
            rng = random.Random(seed + attempt * 1_000_003)
            cand: list = [None] * n
            for i in defer:
                s = slots[i]
                cand[i] = _clone_slot(s, s.x, s.y)
            for orig_i, s in indexed:
                rank = rank_by[orig_i]
                t = rank / max(1, m - 1)
                strat_y = int((t * 2.0 - 1.0) * bias_amp)
                dy = strat_y + rng.randint(-jitter_px, jitter_px)
                dx = rng.randint(-jitter_px, jitter_px)
                nx, ny = clamp_xy(s, s.x + dx, s.y + dy)
                cand[orig_i] = _clone_slot(s, nx, ny)
            sc = score_layout_local(tuple(cand))
            if sc > best_s:
                best_s = sc
                best_cand = list(cand)

        if best_cand is None:
            best_cand = [_clone_slot(slots[i], slots[i].x, slots[i].y) for i in range(n)]
        out = best_cand

        sf = slots[flagship_slot_index]
        rng_f = random.Random(seed + 602_167)
        fj = max(0, int(flagship_max_jitter))
        fdx = rng_f.randint(-fj, fj) if fj else 0
        fdy = rng_f.randint(-fj, fj) if fj else 0
        fx, fy = clamp_xy(sf, sf.x + fdx, sf.y + fdy)
        out[flagship_slot_index] = _clone_slot(sf, fx, fy)

        rims = [i for i in flagship_rim_indices if 0 <= i < n and i != flagship_slot_index]
        for ri, ridx in enumerate(rims):
            rim = slots[ridx]
            rng_r = random.Random(seed + 7711 + ri * 97)
            rx = rng_r.randint(-7, 7)
            ry = rng_r.randint(-7, 7)
            rrot = rng_r.uniform(-1.15, 1.15)
            if ri == 0:
                nx = fx + sf.w - rim.w + 48 + rx
                ny = fy + 62 + ry
            else:
                nx = fx - 42 + rx
                ny = fy + sf.h - rim.h - 28 + ry
            nx, ny = clamp_xy(rim, nx, ny)
            out[ridx] = StripSlotDef(
                nx,
                ny,
                rim.w,
                rim.h,
                float(rim.rotation_deg) + rrot,
                rim.z_index,
                rim.fit,
                rim.prefer_portrait,
                rim.polaroid,
            )

        return tuple(out)

    # --- Cover-aware greedy: others minimize axis-aligned overlap with already placed (cover first). ---
    if cover_slot_index is not None and 0 <= cover_slot_index < n:
        placed: list[tuple[int, int, int, int]] = []
        out: list = [None] * n

        s_cov = slots[cover_slot_index]
        rng0 = random.Random(seed + 404_231)
        cdx = rng0.randint(-cover_max_jitter, cover_max_jitter)
        cdy = rng0.randint(-cover_max_jitter, cover_max_jitter)
        cx, cy = clamp_xy(s_cov, s_cov.x + cdx, s_cov.y + cdy)
        placed.append((cx, cy, s_cov.w, s_cov.h))
        out[cover_slot_index] = _clone_slot(s_cov, cx, cy)

        others = [i for i in range(n) if i != cover_slot_index]
        others.sort(key=lambda i: slots[i].z_index, reverse=True)

        ranked = sorted(others, key=lambda i: slots[i].y + slots[i].h / 2.0)
        rank_of = {idx: r for r, idx in enumerate(ranked)}
        bias_amp = float(jitter_px) + 32.0
        denom = max(1, len(ranked) - 1)

        for orig_i in others:
            s = slots[orig_i]
            rank = rank_of[orig_i]
            t = rank / denom
            strat_y = int((t * 2.0 - 1.0) * bias_amp)
            best_sc = 1e30
            best_xy = (s.x, s.y)
            for k in range(56):
                rng = random.Random(seed + orig_i * 7919 + k * 131)
                dy = strat_y + rng.randint(-jitter_px, jitter_px)
                dx = rng.randint(-jitter_px, jitter_px)
                nx, ny = clamp_xy(s, s.x + dx, s.y + dy)
                oa = sum(
                    _intersection_area(nx, ny, s.w, s.h, px, py, pw, ph)
                    for px, py, pw, ph in placed
                )
                dist = 0.11 * (abs(nx - s.x) + abs(ny - s.y))
                sc = oa + dist
                if sc < best_sc:
                    best_sc = sc
                    best_xy = (nx, ny)
            out[orig_i] = _clone_slot(s, best_xy[0], best_xy[1])
            placed.append((best_xy[0], best_xy[1], s.w, s.h))

        return tuple(out)

    # --- Fallback: all slots stratified + global score (no dedicated cover). ---
    indexed = list(enumerate(slots))
    indexed.sort(key=lambda t: t[1].y + t[1].h / 2.0)
    rank_by_orig_index: dict[int, int] = {}
    for rank, (orig_i, _) in enumerate(indexed):
        rank_by_orig_index[orig_i] = rank

    bias_amp = float(jitter_px) + 36.0

    def clone_with_offsets(rng: random.Random) -> tuple:
        new_list: list = []
        for orig_i, s in enumerate(slots):
            rank = rank_by_orig_index[orig_i]
            t = rank / max(1, n - 1)
            strat_y = int((t * 2.0 - 1.0) * bias_amp)
            dy = strat_y + rng.randint(-jitter_px, jitter_px)
            dx = rng.randint(-jitter_px, jitter_px)
            nx, ny = clamp_xy(s, s.x + dx, s.y + dy)
            new_list.append(_clone_slot(s, nx, ny))
        return tuple(new_list)

    def score_layout(tpl: tuple) -> float:
        cy = [s.y + s.h / 2.0 for s in tpl]
        cx = [s.x + s.w / 2.0 for s in tpl]
        span = max(cy) - min(cy) if cy else 0.0
        ps = statistics.pstdev(cy) if len(cy) >= 2 else 0.0
        if span + ps < 1e-6:
            return -1e9
        pen = 0.0
        dmin = 140.0
        for i in range(len(tpl)):
            for j in range(i + 1, len(tpl)):
                d = ((cx[i] - cx[j]) ** 2 + (cy[i] - cy[j]) ** 2) ** 0.5
                if d < dmin:
                    pen += dmin - d
        raw = span + 0.5 * ps - 0.28 * pen
        if ps < min_y_spread:
            raw -= (min_y_spread - ps) * 2.5
        return raw

    best: tuple | None = None
    best_s = -1e18
    for attempt in range(max_attempts):
        rng = random.Random(seed + attempt * 1_000_003)
        cand = clone_with_offsets(rng)
        sc = score_layout(cand)
        if sc > best_s:
            best_s = sc
            best = cand
    return best if best is not None else slots
