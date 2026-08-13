from __future__ import annotations

import random
from collections.abc import Callable
from datetime import datetime
from pathlib import Path

from app.io import CAROUSEL_SLICE_MAX_BYTES, save_optimized
from app.strip.composer import CardEdge, StripFill, compose_strip_wide
from app.strip.slicer import slice_wide_to_carousel
from app.strip.assign import expand_paths_cyclic, pick_smart_fills
from app.strip.template_data import (
    StripTemplate,
    effective_slot_fill_required,
    get_default_template,
    strip_effective_fill_required,
    strip_image_slot_count,
)

def count_unique_image_paths(paths: list[Path]) -> int:
    """Distinct files (resolved), for strip source-folder checks."""
    return len({p.resolve() for p in paths})


def count_unique_photos_in_fills(
    fills: list[StripFill | None],
    template: StripTemplate,
    *,
    layout_seed: int | None = None,
) -> int:
    """How many distinct image files appear in required (image) slots."""
    req = strip_effective_fill_required(template, layout_seed)
    seen: set[Path] = set()
    for i, f in enumerate(fills):
        if i >= len(req) or not req[i] or f is None:
            continue
        seen.add(Path(f.path).resolve())
    return len(seen)


def validate_strip_unique_sources(
    template: StripTemplate,
    *,
    source_paths: list[Path] | None = None,
    fills: list[StripFill | None] | None = None,
    allow_repeats: bool = False,
    layout_seed: int | None = None,
) -> None:
    """
    By default, require as many distinct photos as the template has image-required slots.
    Otherwise the engine reuses files (and gap-fill pulls from the same pool), which looks
    like “the same image everywhere” when the folder only has one file.
    """
    if allow_repeats:
        return
    need = strip_image_slot_count(template, layout_seed)
    if source_paths is not None:
        u = count_unique_image_paths(source_paths)
    elif fills is not None:
        u = count_unique_photos_in_fills(fills, template, layout_seed=layout_seed)
    else:
        raise TypeError("pass source_paths or fills")
    if u < need:
        raise ValueError(
            f"Strip template {template.id!r} needs {need} different photos (one per image slot). "
            f"You only have {u} distinct file(s). Add more images, choose a folder that is only your "
            f"album (not the app root with a stray JPG), or allow repeats "
            f"(GUI: check “Allow repeating photos”; CLI: --strip-allow-repeats)."
        )


def export_strip_carousel(
    fills: list[StripFill | None],
    output_dir: str | Path,
    template: StripTemplate | None = None,
    *,
    progress_callback: Callable[[int, int], None] | None = None,
    layout_seed: int | None = None,
    card_edge: CardEdge = "borderless",
    border_rgb: tuple[int, int, int] | None = None,
    border_width_px: int = 8,
    underfill_paths: list[Path] | None = None,
) -> tuple[Path, list[Path]]:
    """
    Compose wide canvas, slice to ``template.slice_count`` JPEGs in ``output_dir``.
    Also writes one unsliced master: ``carousel_YYYYMMDD_HHMMSS_RRR_wide.jpg``.
    Filenames for slides: ``carousel_…_01.jpg`` … ``_NN.jpg``.
    """
    template = template or get_default_template()
    req = strip_effective_fill_required(template, layout_seed)
    slot_fills = list(fills[: len(req)])
    for i, need in enumerate(req):
        if need and (i >= len(slot_fills) or slot_fills[i] is None):
            raise ValueError(f"Strip slot {i + 1} requires an image (template {template.id}).")
    if len(fills) < len(req):
        raise ValueError(f"Need {len(req)} slot entries.")

    wide = compose_strip_wide(
        slot_fills,
        template,
        layout_seed=layout_seed,
        card_edge=card_edge,
        border_rgb=border_rgb,
        border_width_px=border_width_px,
        underfill_paths=underfill_paths,
    )
    out_dir = Path(output_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    rnd = random.randint(100, 999)
    wide_path = out_dir / f"carousel_{stamp}_{rnd}_wide.jpg"
    # Wide master is an internal source for slicing; do not enforce social upload size cap here.
    save_optimized(wide, wide_path, max_file_size=None)

    slices = slice_wide_to_carousel(
        wide,
        template.slice_width,
        template.slice_height,
        template.slice_count,
        template.overlap_px,
    )
    n = len(slices)
    paths: list[Path] = []
    for i, sl in enumerate(slices):
        p = out_dir / f"carousel_{stamp}_{rnd}_{i + 1:02d}.jpg"
        save_optimized(
            sl,
            p,
            max_file_size=CAROUSEL_SLICE_MAX_BYTES,
            jpeg_quality_first=98,
            jpeg_quality_min=86,
            jpeg_quality_max=98,
        )
        paths.append(p)
        if progress_callback is not None:
            progress_callback(i + 1, n)
    return wide_path, paths


def pick_random_fills(paths: list[Path], n: int, rng: random.Random | None = None) -> list[Path]:
    """Shuffle and return ``n`` paths, cycling if ``len(paths) < n``."""
    r = rng or random.Random()
    return expand_paths_cyclic(paths, n, r)


def pick_strip_fills(
    paths: list[Path],
    template: StripTemplate,
    *,
    smart: bool,
    rng: random.Random | None = None,
    layout_seed: int | None = None,
) -> list[Path | None]:
    """Return one path (or None) per effective strip slot."""
    r = rng or random.Random()
    req_full = strip_effective_fill_required(template, layout_seed)
    need = sum(1 for x in req_full if x)
    if smart:
        return pick_smart_fills(paths, template, rng=r, layout_seed=layout_seed)
    if not paths:
        raise ValueError("Need at least one image path")
    if template.id == "strip_seamless_mosaic_v1":
        pool = expand_paths_cyclic(paths, need, r)
        r.shuffle(pool)
        return [pool[i] for i in range(need)]
    req_base = effective_slot_fill_required(template)
    pool = expand_paths_cyclic(paths, need, r)
    r.shuffle(pool)
    out: list[Path | None] = [None] * len(req_full)
    pick_i = 0
    for si in range(template.num_slots):
        if not req_base[si]:
            continue
        out[si] = pool[pick_i]
        pick_i += 1
    optional_order = sorted(
        (i for i in range(template.num_slots) if not req_base[i]),
        key=lambda i: template.slots[i].z_index,
    )
    for si in optional_order:
        if pick_i >= len(pool):
            break
        out[si] = pool[pick_i]
        pick_i += 1
    return out
