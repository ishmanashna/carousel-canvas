"""Heuristic assignment of photos to strip slots (orientation vs slot aspect)."""

from __future__ import annotations

import random
from pathlib import Path

from app.io import get_image_aspect_hint
from app.strip.template_data import (
    StripSlotDef,
    StripTemplate,
    effective_slot_fill_required,
    strip_effective_fill_required,
)


def expand_paths_cyclic(paths: list[Path], n: int, rng: random.Random) -> list[Path]:
    """Repeat/shuffle ``paths`` in a cycle until length ``n`` (for fixed slot counts)."""
    if not paths:
        raise ValueError("Need at least one image path")
    base = list(paths)
    rng.shuffle(base)
    return [base[i % len(base)] for i in range(n)]


def _dedupe_paths(paths: list[Path]) -> list[Path]:
    seen: set[Path] = set()
    out: list[Path] = []
    for p in paths:
        k = p.resolve()
        if k in seen:
            continue
        seen.add(k)
        out.append(p)
    return out


def assignment_pool(paths: list[Path], n: int, rng: random.Random) -> list[Path]:
    """
    If there are at least ``n`` unique files, return ``n`` distinct paths (shuffled).
    Otherwise expand with cycling (may repeat).
    """
    uniq = _dedupe_paths(paths)
    rng.shuffle(uniq)
    if len(uniq) >= n:
        return uniq[:n]
    return expand_paths_cyclic(uniq, n, rng)


def pick_underfill_paths(
    paths: list[Path],
    main_slot_paths: list[Path | None],
    n: int,
    rng: random.Random,
) -> list[Path]:
    """
    Pick ``n`` paths for background underfill, preferring files not already used in main slots.
    """
    if n <= 0 or not paths:
        return []
    used = {Path(p).resolve() for p in main_slot_paths if p is not None}
    uniq = _dedupe_paths(paths)
    avail = [p for p in uniq if p.resolve() not in used]
    rng.shuffle(avail)
    if len(avail) >= n:
        return avail[:n]
    out = list(avail)
    rest = [p for p in uniq if p.resolve() in used]
    rng.shuffle(rest)
    for p in rest:
        if len(out) >= n:
            break
        out.append(p)
    if len(out) >= n:
        return out[:n]
    return expand_paths_cyclic(uniq, n, rng)


def _slot_preference(slot: StripSlotDef) -> str:
    if slot.prefer_portrait:
        return "portrait"
    if slot.prefer_landscape:
        return "landscape"
    ar = slot.w / max(1, slot.h)
    if ar >= 1.2:
        return "landscape"
    if ar <= 0.9:
        return "portrait"
    return "any"


def _match_score(pref: str, cls: str) -> int:
    if pref == "any":
        return 0
    if pref == "landscape":
        if cls == "landscape":
            return 2
        if cls == "square":
            return 1
        return 0
    if pref == "portrait":
        if cls == "portrait":
            return 2
        if cls == "square":
            return 1
        return 0
    return 0


def _fill_order_indices(slots: tuple[StripSlotDef, ...]) -> list[int]:
    """Assign picky slots first: portrait, explicit landscape, aspect-landscape, portrait, any."""
    keyed: list[tuple[tuple[int, float, int], int]] = []
    for i, s in enumerate(slots):
        pref = _slot_preference(s)
        ar = s.w / max(1, s.h)
        if s.prefer_portrait:
            keyed.append(((-1, 0.0, i), i))
        elif s.prefer_landscape:
            # Left-to-right so early wide-looking slots (e.g. mosaic col 2) get landscape picks first.
            keyed.append(((-0.5, float(i), i), i))
        elif pref == "landscape":
            keyed.append(((0, -ar, i), i))
        elif pref == "portrait":
            keyed.append(((1, ar, i), i))
        else:
            keyed.append(((2, 0.0, i), i))
    keyed.sort()
    return [i for _, i in keyed]


def _pick_best_match(
    remaining: list[Path],
    classes: dict[Path, str],
    pref: str,
    rng: random.Random,
) -> Path:
    best = max(_match_score(pref, classes.get(p, "square")) for p in remaining)
    tier = [p for p in remaining if _match_score(pref, classes.get(p, "square")) == best]
    return tier[rng.randrange(len(tier))]


def pick_smart_fills(
    paths: list[Path],
    template: StripTemplate,
    rng: random.Random | None = None,
    *,
    layout_seed: int | None = None,
    resolved_slots: tuple[StripSlotDef, ...] | None = None,
) -> list[Path | None]:
    """
    Pick one path per image-required slot (None where ``slot_fill_required`` is False).
    If there are fewer unique paths than required slots, paths are **repeated** in a random cycle.
    Wide slots prefer landscape sources, tall slots prefer portrait; ``prefer_portrait`` / ``prefer_landscape`` win first.

    ``resolved_slots``: when set, must match ``strip_effective_fill_required`` length (preview geometry).
    Otherwise geometry comes from ``resolve_strip_slots_for_preview`` using ``layout_seed``.
    """
    from app.strip.composer import resolve_strip_slots_for_preview

    r = rng or random.Random()
    req = strip_effective_fill_required(template, layout_seed)
    need = sum(1 for x in req if x)
    if not paths:
        raise ValueError("Need at least one image path")
    slots_for_prefs = (
        resolved_slots
        if resolved_slots is not None
        else resolve_strip_slots_for_preview(template, layout_seed)
    )
    if len(slots_for_prefs) != len(req):
        raise ValueError(
            f"resolved_slots length {len(slots_for_prefs)} must match strip effective slots {len(req)}"
        )
    classes = {p: get_image_aspect_hint(p) for p in paths}
    remaining = assignment_pool(paths, need, r)
    fo = _fill_order_indices(slots_for_prefs)
    order = [i for i in fo if req[i]]
    out: list[Path | None] = [None] * len(req)
    for idx in order:
        slot = slots_for_prefs[idx]
        pref = _slot_preference(slot)
        if slot.prefer_portrait:
            portraits = [p for p in remaining if classes.get(p) == "portrait"]
            if portraits:
                chosen = portraits[r.randrange(len(portraits))]
            else:
                chosen = _pick_best_match(remaining, classes, pref, r)
        elif slot.prefer_landscape:
            landscapes = [p for p in remaining if classes.get(p) == "landscape"]
            if landscapes:
                chosen = landscapes[r.randrange(len(landscapes))]
            else:
                chosen = _pick_best_match(remaining, classes, pref, r)
        else:
            chosen = _pick_best_match(remaining, classes, pref, r)
        remaining.remove(chosen)
        out[idx] = chosen

    base_req = effective_slot_fill_required(template)
    if template.id == "strip_seamless_mosaic_v1":
        optional_indices = []
    else:
        optional_indices = sorted(
            (i for i in range(len(template.slots)) if not base_req[i]),
            key=lambda i: slots_for_prefs[i].z_index,
        )
    for idx in optional_indices:
        if not remaining:
            break
        slot = slots_for_prefs[idx]
        pref = _slot_preference(slot)
        if slot.prefer_portrait:
            portraits = [p for p in remaining if classes.get(p) == "portrait"]
            if portraits:
                chosen = portraits[r.randrange(len(portraits))]
            else:
                chosen = _pick_best_match(remaining, classes, pref, r)
        elif slot.prefer_landscape:
            landscapes = [p for p in remaining if classes.get(p) == "landscape"]
            if landscapes:
                chosen = landscapes[r.randrange(len(landscapes))]
            else:
                chosen = _pick_best_match(remaining, classes, pref, r)
        else:
            chosen = _pick_best_match(remaining, classes, pref, r)
        remaining.remove(chosen)
        out[idx] = chosen
    return out
