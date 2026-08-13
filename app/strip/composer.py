from __future__ import annotations

import random
from pathlib import Path
from typing import Literal, Protocol

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageOps

from app.io import parse_color
from app.image_ops import process_image_contained_panned, process_image_panned
from app.strip.gap_fill import apply_beige_gap_fill
from app.strip.underfill import (
    carousel_tail_band_x_range,
    plan_background_underfill_boxes,
    plan_background_underfill_repeat_boxes,
    plan_background_tail_underfill_boxes,
)
from app.strip.layout_jitter import jitter_strip_slots
from app.strip.polaroid_card import (
    POLAROID_INNER_FILL,
    build_polaroid_rgba_supersampled,
    flatten_rgba_over_rgb,
    polaroid_margins,
    polaroid_supersample_factor,
    scale_down_rotated_polaroid,
)
from app.strip.procedural_backgrounds import render_procedural_background
from app.strip.template_data import StripTemplate
from app.strip.token_fill import token_rgb_for_path

CardEdge = Literal["borderless", "wedges", "border"]
FillMode = Literal["photos", "tokens"]


class StripFill(Protocol):
    path: Path
    pan_x: float
    pan_y: float
    flip_h: bool
    grayscale: bool


def _reflection_from_card(
    img: Image.Image,
    *,
    height_frac: float = 0.32,
    opacity: float = 0.08,
    fade_stops: float = 0.15,
) -> Image.Image | None:
    """Vertical flip + gradient mask for table reflection. Paste just below card (py + img.height - 2)."""
    if img.mode != "RGBA" or img.height < 20:
        return None
    rh = max(8, int(round(img.height * height_frac)))
    ref = img.transpose(Image.Transpose.FLIP_TOP_BOTTOM).resize(
        (img.width, rh), resample=Image.Resampling.BILINEAR
    )
    mask = ref.split()[3]
    grad = Image.new("L", (ref.width, ref.height))
    gd = ImageDraw.Draw(grad)
    for y in range(ref.height):
        t = y / max(1, ref.height)
        v = int(255 * opacity * (1.0 - t / max(0.01, fade_stops)))
        v = max(0, min(255, v))
        gd.line([(0, y), (ref.width, y)], fill=v)
    new_a = ImageChops.multiply(mask, grad)
    ref.putalpha(new_a)
    return ref


def _contact_shadow_from_alpha(
    img: Image.Image,
    *,
    offset: tuple[int, int],
    blur: float,
    color: tuple[int, int, int, int],
) -> Image.Image:
    """Build contact shadow from card alpha: offset down-right, blur, tint. Paste at same (px, py) as card."""
    if img.mode != "RGBA":
        return Image.new("RGBA", img.size, (0, 0, 0, 0))
    a = img.split()[3]
    dx, dy = offset
    br = int(blur)
    w = img.width + dx + 2 * br
    h = img.height + dy + 2 * br
    shad = Image.new("L", (w, h), 0)
    shad.paste(a, (dx, dy))
    shad = shad.filter(ImageFilter.GaussianBlur(radius=blur))
    out = Image.new("RGBA", (w, h), (*color[:3], 0))
    out.putalpha(Image.eval(shad, lambda x: min(255, (x * color[3]) // 128)))
    return out


def resolve_strip_slots_for_preview(
    template: StripTemplate,
    layout_seed: int | None,
) -> tuple:
    """Same slot geometry ``compose_strip_wide`` uses (jitter when enabled). For GUI hit-testing overlays."""
    slots = template.slots
    eff_seed = 0 if layout_seed is None else int(layout_seed)
    if template.layout_placer == "organic_polaroid":
        from app.strip.polaroid_scatter import resolve_organic_polaroid_slots

        base = resolve_organic_polaroid_slots(template, eff_seed)
    elif template.layout_jitter_px <= 0 or layout_seed is None:
        base = slots
    else:
        base = jitter_strip_slots(
            slots,
            template.layout_jitter_px,
            eff_seed,
            template.canvas_width,
            template.canvas_height,
            cover_slot_index=template.layout_cover_slot_index,
            cover_max_jitter=max(0, int(template.layout_cover_max_jitter)),
            flagship_slot_index=template.layout_flagship_slot_index,
            flagship_max_jitter=max(0, int(template.layout_flagship_max_jitter)),
            flagship_rim_indices=tuple(template.layout_flagship_rim_slot_indices),
        )
    if template.id == "strip_seamless_mosaic_v1":
        from app.strip.template_data import build_seamless_mosaic_v1_slots

        return build_seamless_mosaic_v1_slots(eff_seed)
    return base


def _layout_seed_int(layout_seed: int | None) -> int:
    return 0 if layout_seed is None else int(layout_seed)


def _mtime_ns(path: Path) -> int:
    try:
        return path.stat().st_mtime_ns
    except OSError:
        return 0


def _init_strip_canvas(
    template: StripTemplate,
    cw: int,
    ch: int,
    bg: tuple[int, int, int],
    card_edge: CardEdge,
    layout_seed: int | None,
) -> Image.Image:
    seed = _layout_seed_int(layout_seed)
    if template.procedural_background:
        base = render_procedural_background(template.procedural_background, cw, ch, seed)
        if card_edge == "borderless":
            return base.convert("RGBA")
        return base
    if card_edge == "borderless":
        return Image.new("RGBA", (cw, ch), (*bg, 255))
    return Image.new("RGB", (cw, ch), color=bg)


def compose_strip_wide(
    fills: list[StripFill | None],
    template: StripTemplate,
    *,
    compose_scale: float = 1.0,
    layout_seed: int | None = None,
    card_edge: CardEdge = "borderless",
    border_rgb: tuple[int, int, int] | None = None,
    border_width_px: int = 8,
    resolved_slots: tuple | None = None,
    underfill_paths: list[Path] | None = None,
    fill_mode: FillMode = "photos",
) -> Image.Image:
    """Paint slots in z_index order. Rotation: positive ``rotation_deg`` = clockwise (viewer).

    ``card_edge``:
      - borderless: rotate with transparent corners, composite so layers underneath show through.
      - wedges: rotate with opaque template background in expanded corners (classic).
      - border: uniform border in ``border_rgb`` before rotate; wedge fill matches border.

    ``compose_scale`` scales canvas and slot boxes (preview). Export uses ``1.0``.
    ``resolved_slots``: if set, must match ``resolve_strip_slots_for_preview`` for the same
    ``layout_seed`` (avoids duplicate jitter/placer work — e.g. GUI preview + hit-test).
    ``underfill_paths``: optional extra photos pasted *under* all slots where the template
    background still shows after a trial compose (see ``background_underfill_layers``).
    ``fill_mode="tokens"``: solid color per source path (no JPEG decode); for cheap layout scoring.
    Underfill and top gap-fill are skipped when ``fill_mode="tokens"``.
    """
    s = float(compose_scale)
    if s <= 0:
        raise ValueError("compose_scale must be positive")

    bg = parse_color(template.background)
    br = border_rgb if border_rgb is not None else bg
    use_full = abs(s - 1.0) < 1e-9
    if use_full:
        cw, ch = template.canvas_width, template.canvas_height
    else:
        cw = max(1, int(round(template.canvas_width * s)))
        ch = max(1, int(round(template.canvas_height * s)))

    slots = resolved_slots if resolved_slots is not None else resolve_strip_slots_for_preview(
        template, layout_seed
    )
    if len(slots) != len(fills):
        raise ValueError(f"Template has {len(slots)} slots, got {len(fills)} fills")

    load_cache: dict[tuple, Image.Image] = {}

    def cached_contained(
        p: Path,
        tw: int,
        th: int,
        pan_x: float,
        pan_y: float,
        *,
        flip_h: bool,
        fill_rgb: tuple[int, int, int],
        source_trim_left_frac: float | None = None,
        source_center_band_frac: float | None = None,
    ) -> Image.Image | None:
        key = (
            "c",
            str(p.resolve()),
            _mtime_ns(p),
            tw,
            th,
            round(float(pan_x), 3),
            round(float(pan_y), 3),
            bool(flip_h),
            fill_rgb,
            None if source_trim_left_frac is None else round(float(source_trim_left_frac), 4),
            None if source_center_band_frac is None else round(float(source_center_band_frac), 4),
        )
        hit = load_cache.get(key)
        if hit is not None:
            return hit
        out = process_image_contained_panned(
            p,
            tw,
            th,
            "mixed",
            pan_x,
            pan_y,
            flip_h=flip_h,
            grayscale=False,
            fill_rgb=fill_rgb,
            source_trim_left_frac=source_trim_left_frac,
            source_center_band_frac=source_center_band_frac,
        )
        if out is not None:
            load_cache[key] = out
        return out

    def cached_panned(
        p: Path,
        tw: int,
        th: int,
        pan_x: float,
        pan_y: float,
        *,
        flip_h: bool,
        source_trim_left_frac: float | None = None,
        source_center_band_frac: float | None = None,
        cover_height_first: bool = False,
    ) -> Image.Image | None:
        key = (
            "p",
            str(p.resolve()),
            _mtime_ns(p),
            tw,
            th,
            round(float(pan_x), 3),
            round(float(pan_y), 3),
            bool(flip_h),
            None if source_trim_left_frac is None else round(float(source_trim_left_frac), 4),
            None if source_center_band_frac is None else round(float(source_center_band_frac), 4),
            bool(cover_height_first),
        )
        hit = load_cache.get(key)
        if hit is not None:
            return hit
        out = process_image_panned(
            p,
            tw,
            th,
            "mixed",
            pan_x,
            pan_y,
            flip_h=flip_h,
            grayscale=False,
            source_trim_left_frac=source_trim_left_frac,
            source_center_band_frac=source_center_band_frac,
            cover_height_first=cover_height_first,
        )
        if out is not None:
            load_cache[key] = out
        return out

    def gap_panned(p: Path, bw: int, bh: int) -> Image.Image | None:
        return cached_panned(p, bw, bh, 0.0, 0.0, flip_h=False)

    def slot_dims(slot) -> tuple[int, int, int, int]:
        if use_full:
            return slot.x, slot.y, slot.w, slot.h
        return (
            int(round(slot.x * s)),
            int(round(slot.y * s)),
            max(1, int(round(slot.w * s))),
            max(1, int(round(slot.h * s))),
        )

    order = sorted(range(len(slots)), key=lambda i: slots[i].z_index)
    n_under = max(0, int(getattr(template, "background_underfill_layers", 0) or 0))
    n_under_boost = max(0, int(getattr(template, "background_underfill_boost_layers", 0) or 0))
    n_under_repeat = max(0, int(getattr(template, "background_underfill_repeat_layers", 0) or 0))
    n_tail_under = max(0, int(getattr(template, "background_tail_underfill_layers", 0) or 0))
    uf_list = list(underfill_paths) if underfill_paths else []
    needs_under = (
        card_edge == "borderless"
        and fill_mode == "photos"
        and (n_under + n_under_boost + n_under_repeat + n_tail_under) > 0
        and len(uf_list) > 0
    )
    canvas = _init_strip_canvas(template, cw, ch, bg, card_edge, layout_seed)

    if card_edge == "borderless":
        assert canvas.mode == "RGBA"

        def paint_borderless_slot(target: Image.Image, i: int) -> None:
            slot = slots[i]
            fill = fills[i]
            if fill is None:
                return
            sx, sy, sw, sh = slot_dims(slot)
            if fill_mode == "tokens" and not slot.polaroid:
                tok = token_rgb_for_path(Path(fill.path))
                img = Image.new("RGB", (sw, sh), tok).convert("RGBA")
                if abs(slot.rotation_deg) > 1e-6:
                    img = img.rotate(
                        -float(slot.rotation_deg),
                        expand=True,
                        resample=Image.Resampling.BICUBIC,
                        fillcolor=(0, 0, 0, 0),
                    )
                cx = sx + sw / 2.0
                cy = sy + sh / 2.0
                px = int(round(cx - img.width / 2.0))
                py = int(round(cy - img.height / 2.0))
                target.paste(img, (px, py), img)
                return
            if slot.polaroid:
                ss = polaroid_supersample_factor(sw, s)
                m_side, m_top, m_bot = polaroid_margins(sw, sh)
                iw = max(1, sw - 2 * m_side)
                ih = max(1, sh - m_top - m_bot)
                if ss <= 1:
                    tw, th = iw, ih
                else:
                    ms2, mt2, mb2 = polaroid_margins(sw * ss, sh * ss)
                    tw = max(1, sw * ss - 2 * ms2)
                    th = max(1, sh * ss - mt2 - mb2)
                if slot.fit == "contain":
                    inner = cached_contained(
                        Path(fill.path),
                        tw,
                        th,
                        fill.pan_x,
                        fill.pan_y,
                        flip_h=fill.flip_h,
                        fill_rgb=POLAROID_INNER_FILL,
                        source_trim_left_frac=slot.source_trim_left_frac,
                        source_center_band_frac=slot.horizontal_center_band_frac,
                    )
                else:
                    inner = cached_panned(
                        Path(fill.path),
                        tw,
                        th,
                        fill.pan_x,
                        fill.pan_y,
                        flip_h=fill.flip_h,
                        source_trim_left_frac=slot.source_trim_left_frac,
                        source_center_band_frac=slot.horizontal_center_band_frac,
                        cover_height_first=slot.cover_height_first,
                    )
                if inner is None:
                    raise ValueError(f"Could not process image for strip slot {i + 1}: {fill.path}")
                slot_seed = (_layout_seed_int(layout_seed) * 1009 + i * 9176) & 0xFFFFFFFF
                card_ss = build_polaroid_rgba_supersampled(
                    inner.convert("RGB"),
                    sw,
                    sh,
                    supersample=max(1, ss),
                    slot_seed=slot_seed,
                    transparent_corners=True,
                )
                ang = float(slot.rotation_deg)
                if abs(ang) > 1e-6:
                    img = card_ss.rotate(
                        -ang,
                        expand=True,
                        resample=Image.Resampling.BICUBIC,
                        fillcolor=(*bg, 0),
                    )
                else:
                    img = card_ss
                img = scale_down_rotated_polaroid(img, ss)
            else:
                if slot.fit == "contain":
                    img = cached_contained(
                        Path(fill.path),
                        sw,
                        sh,
                        fill.pan_x,
                        fill.pan_y,
                        flip_h=fill.flip_h,
                        fill_rgb=bg,
                        source_trim_left_frac=slot.source_trim_left_frac,
                        source_center_band_frac=slot.horizontal_center_band_frac,
                    )
                else:
                    img = cached_panned(
                        Path(fill.path),
                        sw,
                        sh,
                        fill.pan_x,
                        fill.pan_y,
                        flip_h=fill.flip_h,
                        source_trim_left_frac=slot.source_trim_left_frac,
                        source_center_band_frac=slot.horizontal_center_band_frac,
                        cover_height_first=slot.cover_height_first,
                    )
                if img is None:
                    raise ValueError(f"Could not process image for strip slot {i + 1}: {fill.path}")
                img = img.convert("RGBA")
            if (not slot.polaroid) and abs(slot.rotation_deg) > 1e-6:
                img = img.rotate(
                    -float(slot.rotation_deg),
                    expand=True,
                    resample=Image.Resampling.BICUBIC,
                    fillcolor=(0, 0, 0, 0),
                )
            cx = sx + sw / 2.0
            cy = sy + sh / 2.0
            px = int(round(cx - img.width / 2.0))
            py = int(round(cy - img.height / 2.0))
            if slot.polaroid:
                refl = _reflection_from_card(img, height_frac=0.32, opacity=0.08, fade_stops=0.15)
                if refl is not None:
                    target.paste(refl, (px, py + img.height - 2), refl)
                shad = _contact_shadow_from_alpha(img, offset=(5, 8), blur=12, color=(24, 20, 14, 110))
                target.paste(shad, (px, py), shad)
            target.paste(img, (px, py), img)

        fsi = template.layout_flagship_slot_index
        mx = int(template.gap_fill_max_layers)
        top_set: set[int] = set()
        if fsi is not None and 0 <= fsi < len(slots):
            top_set.add(fsi)
            for r in template.layout_flagship_rim_slot_indices:
                if 0 <= r < len(slots):
                    top_set.add(r)
        split_hero_after_gap = bool(top_set) and mx > 0 and fill_mode == "photos"

        def finish_borderless_with_underfill() -> Image.Image:
            temp_canvas = _init_strip_canvas(template, cw, ch, bg, card_edge, layout_seed)
            for i in order:
                paint_borderless_slot(temp_canvas, i)
            flat_preview = flatten_rgba_over_rgb(temp_canvas, bg)
            tol = max(12, int(template.gap_fill_beige_tolerance))
            boxes = plan_background_underfill_boxes(
                flat_preview,
                bg,
                tol,
                n_under,
                layout_seed,
                boost_n=n_under_boost,
            )
            use_n = min(len(boxes), len(uf_list), n_under + n_under_boost)
            canvas_u = _init_strip_canvas(template, cw, ch, bg, card_edge, layout_seed)
            for j in range(use_n):
                sx0, sy0, bw, bh, rot = boxes[j]
                pth = uf_list[j]
                img = cached_panned(Path(pth), bw, bh, 0.0, 0.0, flip_h=False)
                if img is None:
                    continue
                img = img.convert("RGBA")
                if abs(rot) > 1e-6:
                    img = img.rotate(
                        -float(rot),
                        expand=True,
                        resample=Image.Resampling.BICUBIC,
                        fillcolor=(0, 0, 0, 0),
                    )
                cx = sx0 + bw / 2.0
                cy = sy0 + bh / 2.0
                px = int(round(cx - img.width / 2.0))
                py = int(round(cy - img.height / 2.0))
                canvas_u.paste(img, (px, py), img)
            if n_under_repeat > 0 and uf_list:
                flat_after_two = flatten_rgba_over_rgb(canvas_u, bg)
                rep_boxes = plan_background_underfill_repeat_boxes(
                    flat_after_two,
                    bg,
                    tol,
                    n_under_repeat,
                    layout_seed,
                    beige_tolerance_bonus=38,
                )
                rrng = random.Random((_layout_seed_int(layout_seed) ^ 0x5EED1EAF) & 0xFFFFFFFF)
                for sx0, sy0, bw, bh, rot in rep_boxes:
                    pth = uf_list[rrng.randrange(len(uf_list))]
                    img = cached_panned(Path(pth), bw, bh, 0.0, 0.0, flip_h=False)
                    if img is None:
                        continue
                    img = img.convert("RGBA")
                    if abs(rot) > 1e-6:
                        img = img.rotate(
                            -float(rot),
                            expand=True,
                            resample=Image.Resampling.BICUBIC,
                            fillcolor=(0, 0, 0, 0),
                        )
                    cx = sx0 + bw / 2.0
                    cy = sy0 + bh / 2.0
                    px = int(round(cx - img.width / 2.0))
                    py = int(round(cy - img.height / 2.0))
                    canvas_u.paste(img, (px, py), img)
            if n_tail_under > 0 and uf_list:
                flat_after_repeat = flatten_rgba_over_rgb(canvas_u, bg)
                tail_slices = max(1, int(getattr(template, "background_tail_slice_count", 2) or 2))
                if use_full:
                    tw0, ov0 = int(template.slice_width), int(template.overlap_px)
                else:
                    tw0 = max(1, int(round(template.slice_width * s)))
                    ov0 = max(0, min(tw0 - 1, int(round(template.overlap_px * s))))
                tx0, tx1 = carousel_tail_band_x_range(
                    cw,
                    tw0,
                    template.slice_count,
                    ov0,
                    tail_slices,
                )
                tail_tol_bonus = 52 if tail_slices <= 1 else 42
                tail_boxes = plan_background_tail_underfill_boxes(
                    flat_after_repeat,
                    bg,
                    tol,
                    n_tail_under,
                    layout_seed,
                    tx0,
                    tx1,
                    beige_tolerance_bonus=tail_tol_bonus,
                )
                trng = random.Random((_layout_seed_int(layout_seed) ^ 0x7A11BEEF) & 0xFFFFFFFF)
                for sx0, sy0, bw, bh, rot in tail_boxes:
                    pth = uf_list[trng.randrange(len(uf_list))]
                    img = cached_panned(Path(pth), bw, bh, 0.0, 0.0, flip_h=False)
                    if img is None:
                        continue
                    img = img.convert("RGBA")
                    if abs(rot) > 1e-6:
                        img = img.rotate(
                            -float(rot),
                            expand=True,
                            resample=Image.Resampling.BICUBIC,
                            fillcolor=(0, 0, 0, 0),
                        )
                    cx = sx0 + bw / 2.0
                    cy = sy0 + bh / 2.0
                    px = int(round(cx - img.width / 2.0))
                    py = int(round(cy - img.height / 2.0))
                    canvas_u.paste(img, (px, py), img)
            for i in order:
                paint_borderless_slot(canvas_u, i)
            result = flatten_rgba_over_rgb(canvas_u, bg)
            if mx > 0 and fill_mode == "photos":
                pool = [Path(fill.path) for fill in fills if fill is not None]
                if pool:
                    result = apply_beige_gap_fill(
                        result,
                        bg=bg,
                        paths=pool,
                        layout_seed=layout_seed,
                        max_layers=mx,
                        tolerance=max(12, int(template.gap_fill_beige_tolerance)),
                        stop_ratio=float(template.gap_fill_stop_ratio),
                        panned_loader=gap_panned,
                    )
            return result

        if needs_under:
            return finish_borderless_with_underfill()

        if split_hero_after_gap:
            main_order = [i for i in order if i not in top_set]
            top_order = sorted(top_set, key=lambda i: slots[i].z_index)
            for i in main_order:
                paint_borderless_slot(canvas, i)
            pool = [Path(fill.path) for fill in fills if fill is not None]
            result = flatten_rgba_over_rgb(canvas, bg)
            if pool:
                result = apply_beige_gap_fill(
                    result,
                    bg=bg,
                    paths=pool,
                    layout_seed=layout_seed,
                    max_layers=mx,
                    tolerance=max(12, int(template.gap_fill_beige_tolerance)),
                    stop_ratio=float(template.gap_fill_stop_ratio),
                    panned_loader=gap_panned,
                )
            canvas2 = Image.new("RGBA", (cw, ch), (*bg, 255))
            canvas2.paste(result, (0, 0))
            for i in top_order:
                paint_borderless_slot(canvas2, i)
            return flatten_rgba_over_rgb(canvas2, bg)

        for i in order:
            paint_borderless_slot(canvas, i)
        result = flatten_rgba_over_rgb(canvas, bg)
        if mx > 0 and fill_mode == "photos":
            pool = [Path(fill.path) for fill in fills if fill is not None]
            if pool:
                result = apply_beige_gap_fill(
                    result,
                    bg=bg,
                    paths=pool,
                    layout_seed=layout_seed,
                    max_layers=mx,
                    tolerance=max(12, int(template.gap_fill_beige_tolerance)),
                    stop_ratio=float(template.gap_fill_stop_ratio),
                    panned_loader=gap_panned,
                )
        return result

    assert canvas.mode == "RGB"
    for i in order:
        slot = slots[i]
        fill = fills[i]
        if fill is None:
            continue
        sx, sy, sw, sh = slot_dims(slot)
        wedge_fill: tuple[int, int, int]
        if slot.polaroid:
            ss = polaroid_supersample_factor(sw, s)
            m_side, m_top, m_bot = polaroid_margins(sw, sh)
            iw = max(1, sw - 2 * m_side)
            ih = max(1, sh - m_top - m_bot)
            if ss <= 1:
                tw, th = iw, ih
            else:
                ms2, mt2, mb2 = polaroid_margins(sw * ss, sh * ss)
                tw = max(1, sw * ss - 2 * ms2)
                th = max(1, sh * ss - mt2 - mb2)
            if slot.fit == "contain":
                inner = cached_contained(
                    Path(fill.path),
                    tw,
                    th,
                    fill.pan_x,
                    fill.pan_y,
                    flip_h=fill.flip_h,
                    fill_rgb=POLAROID_INNER_FILL,
                    source_trim_left_frac=slot.source_trim_left_frac,
                    source_center_band_frac=slot.horizontal_center_band_frac,
                )
            else:
                inner = cached_panned(
                    Path(fill.path),
                    tw,
                    th,
                    fill.pan_x,
                    fill.pan_y,
                    flip_h=fill.flip_h,
                    source_trim_left_frac=slot.source_trim_left_frac,
                    source_center_band_frac=slot.horizontal_center_band_frac,
                    cover_height_first=slot.cover_height_first,
                )
            if inner is None:
                raise ValueError(f"Could not process image for strip slot {i + 1}: {fill.path}")
            slot_seed = (_layout_seed_int(layout_seed) * 1009 + i * 9176) & 0xFFFFFFFF
            card_ss = build_polaroid_rgba_supersampled(
                inner.convert("RGB"),
                sw,
                sh,
                supersample=max(1, ss),
                slot_seed=slot_seed,
                transparent_corners=True,
            )
            flat = flatten_rgba_over_rgb(card_ss, (255, 255, 255))
            ang = float(slot.rotation_deg)
            if abs(ang) > 1e-6:
                img = flat.rotate(
                    -ang,
                    expand=True,
                    resample=Image.Resampling.BICUBIC,
                    fillcolor=(255, 255, 255),
                )
            else:
                img = flat
            img = scale_down_rotated_polaroid(img, ss)
            wedge_fill = (255, 255, 255)
        else:
            if slot.fit == "contain":
                img = cached_contained(
                    Path(fill.path),
                    sw,
                    sh,
                    fill.pan_x,
                    fill.pan_y,
                    flip_h=fill.flip_h,
                    fill_rgb=bg,
                    source_trim_left_frac=slot.source_trim_left_frac,
                    source_center_band_frac=slot.horizontal_center_band_frac,
                )
            else:
                img = cached_panned(
                    Path(fill.path),
                    sw,
                    sh,
                    fill.pan_x,
                    fill.pan_y,
                    flip_h=fill.flip_h,
                    source_trim_left_frac=slot.source_trim_left_frac,
                    source_center_band_frac=slot.horizontal_center_band_frac,
                    cover_height_first=slot.cover_height_first,
                )
            if img is None:
                raise ValueError(f"Could not process image for strip slot {i + 1}: {fill.path}")
            if img.mode != "RGB":
                img = img.convert("RGB")
            wedge_fill = bg

        if card_edge == "border":
            bw = max(1, int(round(border_width_px * s))) if not use_full else border_width_px
            img = ImageOps.expand(img, border=bw, fill=br)
            wedge_fill = br

        if (not slot.polaroid) and abs(slot.rotation_deg) > 1e-6:
            img = img.rotate(
                -float(slot.rotation_deg),
                expand=True,
                resample=Image.Resampling.BICUBIC,
                fillcolor=wedge_fill,
            )

        cx = sx + sw / 2.0
        cy = sy + sh / 2.0
        px = int(round(cx - img.width / 2.0))
        py = int(round(cy - img.height / 2.0))
        canvas.paste(img, (px, py))
    return canvas
