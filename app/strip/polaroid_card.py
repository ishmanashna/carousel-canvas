"""Supersampled polaroid frame: paper texture, soft inner edge, premultiplied-safe flatten."""

from __future__ import annotations

import random

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageOps

POLAROID_INNER_FILL = (252, 252, 250)


def polaroid_margins(sw: int, sh: int) -> tuple[int, int, int]:
    """Side (L=R), top, bottom — instant-film chin (~26–28% of card height)."""
    m_side = max(5, int(round(sw * 0.042)))
    m_top = max(5, int(round(sh * 0.038)))
    m_bottom = max(18, int(round(sh * 0.27)))
    inner_h = sh - m_top - m_bottom
    min_inner = max(48, int(round(sw * 0.45)))
    if inner_h < min_inner:
        m_bottom = max(12, sh - m_top - min_inner)
    return m_side, m_top, m_bottom


def polaroid_supersample_factor(slot_w: int, compose_scale: float) -> int:
    if slot_w < 200:
        return 1
    if compose_scale < 0.92:
        return 2
    return 3


def flatten_rgba_over_rgb(im: Image.Image, bg: tuple[int, int, int]) -> Image.Image:
    """Straight-alpha RGBA flattened via premultiplied compositing (fewer tilt/JPEG fringes)."""
    if im.mode != "RGBA":
        return im.convert("RGB")
    r, g, b, a = im.split()
    pr = ImageChops.multiply(r, a)
    pg = ImageChops.multiply(g, a)
    pb = ImageChops.multiply(b, a)
    inv_a = Image.eval(a, lambda x: 255 - x)
    b0 = Image.new("L", im.size, bg[0])
    b1 = Image.new("L", im.size, bg[1])
    b2 = Image.new("L", im.size, bg[2])
    or_r = ImageChops.add(pr, ImageChops.multiply(b0, inv_a))
    or_g = ImageChops.add(pg, ImageChops.multiply(b1, inv_a))
    or_b = ImageChops.add(pb, ImageChops.multiply(b2, inv_a))
    return Image.merge("RGB", (or_r, or_g, or_b))


def _paper_base_rgb(seed: int) -> tuple[int, int, int]:
    """Per-card stock tint — subtle variation (toned-down vs loud spreads)."""
    rng = random.Random((seed * 0x9E3779B1 + 0xA11CE) & 0xFFFFFFFF)
    anchors: list[tuple[int, int, int]] = [
        (248, 245, 236),
        (242, 244, 252),
        (252, 248, 236),
        (246, 242, 232),
        (244, 248, 238),
        (250, 244, 246),
        (252, 250, 238),
        (238, 240, 244),
        (255, 248, 228),
        (240, 244, 236),
        (248, 240, 242),
        (244, 246, 250),
    ]
    br, bg, bb = anchors[rng.randrange(len(anchors))]
    return (
        max(225, min(255, br + rng.randint(-18, 18))),
        max(222, min(255, bg + rng.randint(-20, 20))),
        max(215, min(255, bb + rng.randint(-22, 22))),
    )


def _noise_dims_capped(w: int, h: int, max_pixels: int) -> tuple[int, int]:
    """Scale (w,h) down uniformly so area ≤ max_pixels (faster than full-card effect_noise)."""
    if w * h <= max_pixels:
        return w, h
    scale = (max_pixels / float(w * h)) ** 0.5
    return max(1, int(w * scale)), max(1, int(h * scale))


def _paper_rgb(w: int, h: int, seed: int) -> Image.Image:
    """
    Paper grain: large layers stay smooth; **fine grit** is generated at up to ~4Mpx then
    upscaled with BILINEAR so speckle stays crisp (tiny tiles + LANCZOS looked mushy and low-res).
    """
    rng = random.Random((seed * 0x9E3779B1) & 0xFFFFFFFF)
    base_rgb = _paper_base_rgb(seed)
    base = Image.new("RGB", (w, h), base_rgb)
    out = base

    def _grain_layer(
        tw: int,
        th: int,
        noise_lo: float,
        noise_hi: float,
        blur: float,
        alpha: float,
        *,
        up: Image.Resampling,
    ) -> None:
        nonlocal out
        tw = max(8, min(w, tw))
        th = max(8, min(h, th))
        n = Image.effect_noise((tw, th), float(rng.uniform(noise_lo, noise_hi)))
        if blur > 1e-6:
            n = n.filter(ImageFilter.GaussianBlur(radius=blur))
        if (tw, th) != (w, h):
            n = n.resize((w, h), resample=up)
        g = n.convert("L")
        tint = Image.merge("RGB", (g, g, g))
        out = Image.blend(out, tint, min(0.55, max(0.0, alpha)))

    # Large-scale tone (smooth — LANCZOS upscale OK)
    _grain_layer(max(64, w // 4), max(64, h // 4), 55.0, 92.0, 0.42, 0.20 + rng.uniform(0, 0.05), up=Image.Resampling.LANCZOS)
    _grain_layer(max(96, w // 2), max(96, h // 2), 42.0, 72.0, 0.22, 0.17 + rng.uniform(0, 0.04), up=Image.Resampling.BILINEAR)
    mw = max(180, int(w * 0.55))
    mh = max(220, int(h * 0.55))
    _grain_layer(mw, mh, 28.0, 48.0, 0.11, 0.14 + rng.uniform(0, 0.04), up=Image.Resampling.BILINEAR)
    # Sharp grit: high-res noise capped for speed, BILINEAR keeps grain sharper than LANCZOS from tiny maps
    gw, gh = _noise_dims_capped(w, h, 4_200_000)
    _grain_layer(gw, gh, 10.0, 20.0, 0.05, 0.12 + rng.uniform(0, 0.03), up=Image.Resampling.BILINEAR)
    gw2, gh2 = _noise_dims_capped(w, h, 4_200_000)
    _grain_layer(gw2, gh2, 7.0, 14.0, 0.03, 0.10 + rng.uniform(0, 0.03), up=Image.Resampling.BILINEAR)
    return out


def _border_only_mask(
    w: int,
    h: int,
    m_side: int,
    m_top: int,
    iw2: int,
    ih2: int,
    ss: int,
) -> Image.Image:
    """L mask: 255 on border, 0 in window, soft inner transition (Phase A)."""
    m = Image.new("L", (w, h), 255)
    dr = ImageDraw.Draw(m)
    dr.rectangle((m_side, m_top, m_side + iw2 - 1, m_top + ih2 - 1), fill=0)
    feather = max(2, min(5, int(round(2.5 * max(1, ss)))))
    return m.filter(ImageFilter.GaussianBlur(radius=feather * 0.5))


def _outer_border_rim_mask(border_mask: Image.Image, ss: int) -> Image.Image:
    """Thin band along the physical outer edge of the card (border region only)."""
    k = max(3, 2 * ss + 1)
    if k % 2 == 0:
        k += 1
    eroded = border_mask.filter(ImageFilter.MinFilter(k))
    rim = ImageChops.subtract(border_mask, eroded)
    return rim.filter(ImageFilter.GaussianBlur(radius=max(0.8, ss * 0.56)))


def _apply_border_tone_and_vignette(
    paper: Image.Image,
    border_mask: Image.Image,
    slot_seed: int,
    ss: int,
) -> Image.Image:
    """
    Phase A.1: gentle warm/cool + rim vignette + soft blotches (toned-down color).
    """
    w, h = paper.size
    rng = random.Random((slot_seed ^ 0x85EBCA6B) & 0xFFFFFFFF)
    gv = Image.linear_gradient("L").resize((w, h), resample=Image.Resampling.LANCZOS)
    gh = (
        Image.linear_gradient("L")
        .transpose(Image.Transpose.ROTATE_270)
        .resize((w, h), resample=Image.Resampling.LANCZOS)
    )
    comb = ImageChops.add(gv, gh).point(lambda x: min(255, x // 2))
    wr = min(255, 252 + rng.randint(-8, 4))
    wg = min(255, max(228, 238 + rng.randint(-12, 18)))
    wb = min(255, max(210, 228 + rng.randint(-18, 22)))
    warm = Image.new("RGB", (w, h), (wr, wg, wb))
    wmul = 0.22 + rng.uniform(0.0, 0.10)
    warm_amt = ImageChops.multiply(
        comb.point(lambda p, m=wmul: int(min(255, p * m))),
        border_mask,
    )
    out = Image.composite(warm, paper, warm_amt)
    cr = max(200, min(255, 236 + rng.randint(-18, 14)))
    cg = max(205, min(255, 240 + rng.randint(-14, 16)))
    cb = max(215, min(255, 248 + rng.randint(-12, 10)))
    cool = Image.new("RGB", (w, h), (cr, cg, cb))
    inv = ImageOps.invert(comb)
    cmul = 0.10 + rng.uniform(0.0, 0.08)
    cool_amt = ImageChops.multiply(
        inv.point(lambda p, m=cmul: int(min(255, p * m))),
        border_mask,
    )
    out = Image.composite(cool, out, cool_amt)
    er_k = max(3, 2 * ss + 1)
    if er_k % 2 == 0:
        er_k += 1
    eroded = border_mask.filter(ImageFilter.MinFilter(er_k))
    outer_band = ImageChops.subtract(border_mask, eroded)
    outer_band = outer_band.filter(ImageFilter.GaussianBlur(radius=max(0.85, ss * 0.55)))
    dr = max(175, min(228, 210 + rng.randint(-22, 22)))
    dg = max(168, min(222, 200 + rng.randint(-22, 24)))
    db = max(158, min(215, 188 + rng.randint(-24, 26)))
    dark = Image.new("RGB", (w, h), (dr, dg, db))
    vmul = 0.18 + rng.uniform(0.0, 0.10)
    vig_amt = ImageChops.multiply(
        outer_band.point(lambda p, m=vmul: int(min(255, p * m))),
        border_mask,
    )
    out = Image.composite(dark, out, vig_amt)
    blob_t = max(48, min(200, w // 6))
    blob = Image.effect_noise((blob_t, blob_t), float(rng.uniform(32.0, 52.0)))
    blob = blob.filter(ImageFilter.GaussianBlur(radius=7.0 + rng.uniform(0, 5))).resize(
        (w, h), resample=Image.Resampling.LANCZOS
    )
    blob_l = blob.split()[0]
    blot_warm = Image.new(
        "RGB",
        (w, h),
        (
            228 + rng.randint(0, 18),
            210 + rng.randint(0, 28),
            175 + rng.randint(0, 35),
        ),
    )
    blot_cool = Image.new(
        "RGB",
        (w, h),
        (
            195 + rng.randint(0, 28),
            205 + rng.randint(0, 25),
            218 + rng.randint(0, 22),
        ),
    )
    bmask = blob_l.point(lambda x: int(min(255, max(0, (x - 96) * 2.0))))
    bmask = ImageChops.multiply(bmask, border_mask)
    bmul = 0.14 + rng.uniform(0.0, 0.10)
    bamt = bmask.point(lambda p, m=bmul: int(min(255, p * m)))
    out = Image.composite(blot_warm, out, bamt)
    bmask2 = blob_l.point(lambda x: int(min(255, max(0, (180 - x) * 2.4))))
    bmask2 = ImageChops.multiply(bmask2, border_mask)
    bmul2 = 0.09 + rng.uniform(0.0, 0.07)
    bamt2 = bmask2.point(lambda p, m=bmul2: int(min(255, p * m)))
    return Image.composite(blot_cool, out, bamt2)


def _apply_border_aging_rgb(
    card: Image.Image,
    border_mask: Image.Image,
    outer_rim: Image.Image,
    slot_seed: int,
    ss: int,
) -> None:
    """Phase A.2: subtle warm rim; per-slot tint stays near neutral paper."""
    if card.mode != "RGBA":
        return
    w, h = card.size
    rng = random.Random((slot_seed ^ 0xC0FFEE) & 0xFFFFFFFF)
    r, g, b, al = card.split()
    rgb = Image.merge("RGB", (r, g, b))
    rim = outer_rim.filter(ImageFilter.GaussianBlur(radius=max(1.1, ss * 0.7)))
    er = max(235, min(255, 250 + rng.randint(-8, 6)))
    eg = max(218, min(255, 236 + rng.randint(-14, 20)))
    eb = max(198, min(248, 222 + rng.randint(-18, 28)))
    warm_edge = Image.new("RGB", (w, h), (er, eg, eb))
    emul = 0.28 + rng.uniform(0.0, 0.14)
    edge_amt = ImageChops.multiply(
        rim.point(lambda p, m=emul: int(min(255, p * m))),
        border_mask,
    )
    rgb = Image.composite(warm_edge, rgb, edge_amt)

    card.paste(Image.merge("RGBA", (*rgb.split(), al)), (0, 0))


def _alpha_contour_imperfection(
    alpha: Image.Image,
    outer_shape_mask: Image.Image,
    slot_seed: int,
    ss: int,
) -> Image.Image:
    """Phase A.3: nudge alpha only on the outer rounded-rect edge (not inner photo soft mask — that caused dark seams)."""
    rng = random.Random((slot_seed ^ 0xDEADBEEF) & 0xFFFFFFFF)
    w, h = alpha.size
    dil = outer_shape_mask.filter(ImageFilter.MaxFilter(3))
    ero = outer_shape_mask.filter(ImageFilter.MinFilter(3))
    edge = ImageChops.subtract(dil, ero).filter(ImageFilter.GaussianBlur(radius=0.45))
    tw, th = _noise_dims_capped(w, h, 3_800_000)
    n = Image.effect_noise((tw, th), float(rng.uniform(9.0, 16.0)))
    n = n.resize((w, h), resample=Image.Resampling.BILINEAR).split()[0]
    amp = (12.0 + rng.uniform(0.0, 10.0)) * max(1.0, ss * 0.45)
    n = n.point(lambda x, a=amp: int((x - 128) * (a / 128.0)))
    ew = 0.30 + rng.uniform(0.0, 0.12)
    edge_w = edge.point(lambda p, m=ew: min(255, int(p * m)))
    delta = ImageChops.multiply(n, edge_w)
    out = ImageChops.add(alpha, delta)
    return out.point(lambda x: max(0, min(255, x)))


def _inner_soft_mask(iw: int, ih: int, feather: int) -> Image.Image:
    m = Image.new("L", (iw, ih), 0)
    ImageDraw.Draw(m).rectangle((0, 0, iw - 1, ih - 1), fill=255)
    r = max(0.55, feather * 0.48)
    return m.filter(ImageFilter.GaussianBlur(radius=r))


def _polaroid_photo_finish(im: Image.Image, seed: int) -> Image.Image:
    rng = random.Random((seed ^ 0xA5A5F00D) & 0xFFFFFFFF)
    warm = Image.new("RGB", im.size, (255, 247, 234))
    out = Image.blend(im, warm, 0.038 + rng.uniform(0.0, 0.018))
    ac = ImageOps.autocontrast(out, cutoff=0.25)
    out = Image.blend(out, ac, 0.08 + rng.uniform(0.0, 0.04))
    n = Image.effect_noise(im.size, max(2.4, min(5.5, im.width / 320.0)))
    n = n.filter(ImageFilter.GaussianBlur(radius=0.32))
    ns = n.split()[0]
    g = Image.merge("RGB", (ns, ns, ns))
    return Image.blend(out, g, 0.022 + rng.uniform(0.0, 0.012))


def _outer_bevel(card: Image.Image, ss: int, rad: int) -> None:
    """1px highlight top/left, shadow bottom/right for thickness."""
    if card.mode != "RGBA":
        return
    w, h = card.size
    ov = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    dr = ImageDraw.Draw(ov)
    lw = max(1, int(round(1.6 * ss)))
    hi = (255, 252, 245, min(84, 36 + 8 * ss))
    lo = (12, 10, 8, min(76, 32 + 8 * ss))
    dr.line([(rad, 0), (w - rad, 0)], fill=hi, width=lw)
    dr.line([(0, rad), (0, h - rad)], fill=hi, width=lw)
    dr.line([(rad, h - 1), (w - rad, h - 1)], fill=lo, width=lw)
    dr.line([(w - 1, rad), (w - 1, h - rad)], fill=lo, width=lw)
    ov = ov.filter(ImageFilter.GaussianBlur(radius=max(0.6, ss * 0.3)))
    card.alpha_composite(ov)


def _inset_shadow(card: Image.Image, mx: int, my: int, iw: int, ih: int, ss: int) -> None:
    if card.mode != "RGBA":
        return
    ov = Image.new("RGBA", card.size, (0, 0, 0, 0))
    dr = ImageDraw.Draw(ov)
    lw = max(1, int(round(2.3 * ss)))
    a = min(104, 44 + 18 * ss)
    c = (18, 14, 12, a)
    x0, x1 = mx, mx + iw
    y0, y1 = my, my + ih
    dr.line([(x0, y0), (x1, y0)], fill=c, width=lw)
    dr.line([(x0, y1 - 1), (x1, y1 - 1)], fill=c, width=lw)
    dr.line([(x0, y0), (x0, y1)], fill=c, width=lw)
    dr.line([(x1 - 1, y0), (x1 - 1, y1)], fill=c, width=lw)
    ov = ov.filter(ImageFilter.GaussianBlur(radius=max(0.7, ss * 0.44)))
    card.alpha_composite(ov)


def _outer_rounded_alpha(w: int, h: int, radius: int) -> Image.Image:
    m = Image.new("L", (w, h), 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, w - 1, h - 1), radius=radius, fill=255)
    return m


def build_polaroid_rgba_supersampled(
    inner: Image.Image,
    slot_w: int,
    slot_h: int,
    *,
    supersample: int,
    slot_seed: int,
    transparent_corners: bool,
) -> Image.Image:
    """
    ``inner`` must match the supersampled inner window (iw_ss × ih_ss).

    Returns RGBA at ``slot_w * ss`` × ``slot_h * ss``. Caller rotates (optional), then
    ``scale_down_rotated_polaroid`` to final pixel size before pasting.
    """
    ss = max(1, int(supersample))
    sw2 = max(1, slot_w * ss)
    sh2 = max(1, slot_h * ss)
    m_side, m_top, m_bot = polaroid_margins(sw2, sh2)
    iw2 = max(1, sw2 - 2 * m_side)
    ih2 = max(1, sh2 - m_top - m_bot)

    inner = inner.convert("RGB")
    if inner.size != (iw2, ih2):
        inner = inner.resize((iw2, ih2), resample=Image.Resampling.LANCZOS)

    inner = _polaroid_photo_finish(inner, slot_seed)
    feather = max(1, int(round(1.35 * ss)))
    alpha = _inner_soft_mask(iw2, ih2, feather)
    inner_rgba = Image.merge("RGBA", (*inner.split(), alpha))

    # Phase A: border materiality — tone / vignette on paper, then aging on composed card (photo unchanged).
    border_mask = _border_only_mask(sw2, sh2, m_side, m_top, iw2, ih2, ss)
    outer_rim = _outer_border_rim_mask(border_mask, ss)
    paper_rgb = _paper_rgb(sw2, sh2, slot_seed + 11)
    paper_rgb = _apply_border_tone_and_vignette(paper_rgb, border_mask, slot_seed, ss)
    paper = paper_rgb.convert("RGBA")
    win = paper.crop((m_side, m_top, m_side + iw2, m_top + ih2))
    blended = Image.alpha_composite(win, inner_rgba)
    card = paper.copy()
    card.paste(blended, (m_side, m_top))

    _apply_border_aging_rgb(card, border_mask, outer_rim, slot_seed, ss)

    _inset_shadow(card, m_side, m_top, iw2, ih2, ss)

    rad = max(5, int(round(min(15, min(slot_w, slot_h) * 0.021) * ss)))
    _outer_bevel(card, ss, rad)

    if transparent_corners:
        mask = _outer_rounded_alpha(sw2, sh2, rad)
        ca = card.split()[3]
        ca = ImageChops.multiply(ca, mask)
        # Compositing order: rounded mask, then subtle alpha on outer silhouette only (Phase A.3).
        ca = _alpha_contour_imperfection(ca, mask, slot_seed, ss)
        card.putalpha(ca)

    return card


def scale_down_rotated_polaroid(rotated: Image.Image, supersample: int) -> Image.Image:
    ss = max(1, int(supersample))
    if ss <= 1:
        return rotated
    nw = max(1, int(round(rotated.width / ss)))
    nh = max(1, int(round(rotated.height / ss)))
    return rotated.resize((nw, nh), resample=Image.Resampling.LANCZOS)
