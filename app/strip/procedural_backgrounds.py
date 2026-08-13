"""Strip full-canvas backgrounds: brick-stitched wood tile, cover-scale fallback, or procedural."""

from __future__ import annotations

import array
import random
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

# Registered in ``StripTemplate.procedural_background`` (see template_data).
WOOD_POLAROID_TABLE = "wood_polaroid_table"

_ASSET_DIR = Path(__file__).resolve().parent / "assets"
POLAROID_TABLE_WOOD_ASSET = _ASSET_DIR / "polaroid_table_wood.png"

# Cached gradient masks for feathered seams (overlap × band size).
_mask_h_cache: dict[tuple[int, int], Image.Image] = {}
_mask_v_cache: dict[tuple[int, int], Image.Image] = {}


def render_procedural_background(kind: str, width: int, height: int, seed: int) -> Image.Image:
    if kind == WOOD_POLAROID_TABLE:
        return _wood_polaroid_table(width, height, seed)
    raise ValueError(f"Unknown procedural background: {kind!r}")


def _sample_tile_mean_rgb(tile: Image.Image) -> tuple[int, int, int]:
    t = tile.convert("RGB").resize((32, 32), resample=Image.Resampling.BILINEAR)
    px = t.load()
    r = g = b = 0
    for y in range(32):
        for x in range(32):
            p = px[x, y]
            r += p[0]
            g += p[1]
            b += p[2]
    n = 32 * 32
    return (r // n, g // n, b // n)


def _horizontal_blend_mask(overlap: int, band_h: int) -> Image.Image:
    key = (overlap, band_h)
    hit = _mask_h_cache.get(key)
    if hit is not None:
        return hit
    row = array.array("B", (int(255 * x / max(1, overlap - 1)) for x in range(overlap)))
    buf = array.array("B", (row[x] for _ in range(band_h) for x in range(overlap)))
    g = Image.frombytes("L", (overlap, band_h), buf.tobytes())
    _mask_h_cache[key] = g
    return g


def _vertical_blend_mask(band_w: int, overlap: int) -> Image.Image:
    key = (band_w, overlap)
    hit = _mask_v_cache.get(key)
    if hit is not None:
        return hit
    col = array.array("B", (int(255 * y / max(1, overlap - 1)) for y in range(overlap)))
    buf = array.array("B", (col[y] for y in range(overlap) for _ in range(band_w)))
    g = Image.frombytes("L", (band_w, overlap), buf.tobytes())
    _mask_v_cache[key] = g
    return g


def _paste_blend_horizontal(dest: Image.Image, patch: Image.Image, px: int, py: int, overlap: int) -> None:
    if px <= 0 or overlap <= 2:
        dest.paste(patch, (px, py))
        return
    ol = min(overlap, px, patch.width)
    if ol <= 2:
        dest.paste(patch, (px, py))
        return
    h0 = patch.height
    left = dest.crop((px - ol, py, px, py + h0))
    pr = patch.crop((0, 0, ol, h0))
    mask = _horizontal_blend_mask(ol, h0)
    fused = Image.composite(pr, left, mask)
    dest.paste(fused, (px - ol, py))
    if patch.width > ol:
        dest.paste(patch.crop((ol, 0, patch.width, h0)), (px, py))


def _paste_blend_vertical(dest: Image.Image, patch: Image.Image, px: int, py: int, overlap: int) -> None:
    if py <= 0 or overlap <= 2:
        dest.paste(patch, (px, py))
        return
    ov = min(overlap, py, patch.height)
    if ov <= 2:
        dest.paste(patch, (px, py))
        return
    w0 = patch.width
    top = dest.crop((px, py - ov, px + w0, py))
    pb = patch.crop((0, 0, w0, ov))
    mask = _vertical_blend_mask(w0, ov)
    fused = Image.composite(pb, top, mask)
    dest.paste(fused, (px, py - ov))
    if patch.height > ov:
        dest.paste(patch.crop((0, ov, w0, patch.height)), (px, py))


def _build_brick_row(
    tile: Image.Image,
    tw: int,
    th: int,
    row_h: int,
    out_w: int,
    rng: random.Random,
    oh: int,
    brick_x: int,
    tile_sy: int,
) -> Image.Image:
    """One horizontal band: staggered tile repeats. Same ``tile_sy`` for all rows = one continuous table grain."""
    mean = _sample_tile_mean_rgb(tile)
    row = Image.new("RGB", (out_w, row_h), mean)
    sy = min(max(0, tile_sy), max(0, th - row_h))
    x = -int(brick_x)
    while x < out_w:
        px = max(0, int(x))
        rem = out_w - px
        if rem <= 0:
            break
        pw = min(tw, rem)
        if pw < 1:
            break
        sx = rng.randint(0, max(0, tw - pw))
        piece = tile.crop((sx, sy, sx + pw, sy + row_h))
        pw2 = min(piece.width, out_w - px)
        piece = piece.crop((0, 0, pw2, row_h))
        # Plain paste only: horizontal feather routinely under-filled the right edge (mean-color gutter).
        row.paste(piece, (px, 0))
        x += tw - oh
    return row


def _stitch_brick_wood(tile: Image.Image, w: int, h: int, seed: int, mean_rgb: tuple[int, int, int]) -> Image.Image:
    """
    Brick layout: each row is offset horizontally (~half tile) so vertical seams do not align.
    Rows overlap vertically with a short feather blend. Canvas prefilled with ``mean_rgb`` (no black gaps).
    """
    tile = tile.convert("RGB")
    tw0, th0 = tile.size
    target_tw = max(480, min(960, tw0))
    th_r = max(1, int(th0 * target_tw / tw0))
    tile = tile.resize((target_tw, th_r), resample=Image.Resampling.LANCZOS)
    tw, th = tile.size
    rng = random.Random((seed * 0xC001D00D) & 0xFFFFFFFF)
    oh = max(32, min(92, tw // 10))
    ov = max(20, min(76, th // 11))

    out = Image.new("RGB", (w, h), mean_rgb)
    y_cursor = 0
    row_i = 0
    period = max(1, tw - oh)
    max_row_h = min(th, h)
    tile_sy = rng.randint(0, max(0, th - max(1, max_row_h)))

    while y_cursor < h:
        remain = h - y_cursor
        row_h = min(th, remain)
        if remain > 0 and row_h < 10:
            row_h = remain
        if row_h < 6 and remain > 0:
            row_h = remain

        base_brick = (row_i * (tw // 2)) % period
        brick_j = rng.randint(0, min(120, period // 4))
        brick_x = (base_brick + brick_j) % period

        row_img = _build_brick_row(tile, tw, th, row_h, w, rng, oh, brick_x, tile_sy)

        # Vertical feather shortens the blended band by ``ov_use`` at the bottom; the last row would
        # stop short of the canvas, leaving a horizontal strip of ``mean_rgb``.
        touches_bottom = y_cursor + row_h >= h
        if row_i == 0 or touches_bottom:
            out.paste(row_img, (0, y_cursor))
        else:
            ov_use = min(ov, row_h - 2, y_cursor, row_h // 2)
            ov_use = max(0, ov_use)
            if ov_use > 2:
                _paste_blend_vertical(out, row_img, 0, y_cursor, ov_use)
            else:
                out.paste(row_img, (0, y_cursor))

        if touches_bottom:
            y_cursor = h
            break

        ov_step = min(ov, max(1, row_h - 2))
        y_cursor += max(1, row_h - ov_step)
        row_i += 1

    return out


def _scale_crop_cover(im: Image.Image, w: int, h: int) -> Image.Image:
    im = im.convert("RGB")
    iw, ih = im.size
    if iw <= 0 or ih <= 0:
        raise ValueError("Invalid asset dimensions")
    scale = max(w / iw, h / ih)
    nw = max(1, int(round(iw * scale)))
    nh = max(1, int(round(ih * scale)))
    im = im.resize((nw, nh), resample=Image.Resampling.LANCZOS)
    left = (nw - w) // 2
    top = (nh - h) // 2
    return im.crop((left, top, left + w, top + h))


def _wood_from_asset(w: int, h: int, seed: int) -> Image.Image | None:
    """Brick-stitch the wood tile for fewer obvious upscales; prefilled mean RGB avoids gaps."""
    if not POLAROID_TABLE_WOOD_ASSET.is_file():
        return None
    try:
        with Image.open(POLAROID_TABLE_WOOD_ASSET) as im:
            im = im.convert("RGB")
            mean_rgb = _sample_tile_mean_rgb(im)
            out = _stitch_brick_wood(im, w, h, seed, mean_rgb)
            return out.filter(ImageFilter.GaussianBlur(radius=0.22))
    except OSError:
        return None


def _blend_rgb(a: tuple[int, int, int], b: tuple[int, int, int], t: float) -> tuple[int, int, int]:
    t = max(0.0, min(1.0, t))
    return (
        int(a[0] + (b[0] - a[0]) * t),
        int(a[1] + (b[1] - a[1]) * t),
        int(a[2] + (b[2] - a[2]) * t),
    )


def _wood_polaroid_table(w: int, h: int, seed: int) -> Image.Image:
    baked = _wood_from_asset(w, h, seed)
    if baked is not None:
        return baked
    return _wood_polaroid_table_procedural(w, h, seed)


def _wood_polaroid_table_procedural(w: int, h: int, seed: int) -> Image.Image:
    """Fallback when asset missing: long horizontal planks, low noise, no vignette."""
    rng = random.Random((seed * 1103515245 + 12345) & 0x7FFFFFFF)
    base_lo = (98, 68, 46)
    base_hi = (132, 94, 64)
    img = Image.new("RGB", (w, h), base_lo)
    draw = ImageDraw.Draw(img)
    x = 0
    while x < w:
        pw = rng.randint(620, 1100)
        t0 = rng.random()
        c0 = _blend_rgb(base_lo, base_hi, t0)
        x1 = min(x + pw, w)
        if x1 <= x:
            break
        draw.rectangle([x, 0, x1, h], fill=c0)
        if x1 < w:
            seam_x = min(w - 1, int(x1) + rng.randint(-1, 1))
            draw.line([(seam_x, 0), (seam_x, h)], fill=(52, 36, 24), width=rng.randint(1, 2))
        if x1 >= w:
            break
        x = x1 + rng.randint(-3, 5)
        if x >= w:
            break
    # Advancing x past w could exit with x1 < w uncovered — fill the tail.
    if x < w:
        t0 = rng.random()
        c0 = _blend_rgb(base_lo, base_hi, t0)
        draw.rectangle([x, 0, w, h], fill=c0)
    grain = Image.new("RGB", (480, max(72, h // 18)))
    gd = ImageDraw.Draw(grain)
    for _ in range(2500):
        gx, gy = rng.randint(0, grain.width - 1), rng.randint(0, grain.height - 1)
        gc = _blend_rgb(base_lo, base_hi, rng.uniform(0.15, 0.85))
        gd.point((gx, gy), fill=gc)
    grain = grain.filter(ImageFilter.GaussianBlur(radius=4))
    grain = grain.resize((w, h), resample=Image.Resampling.BILINEAR)
    return Image.blend(img, grain, 0.07)
