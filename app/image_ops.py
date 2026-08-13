"""Mural/carousel image processing for Carousel Canvas."""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Tuple

from PIL import Image, ImageOps

from app.io import fix_orientation, flatten_alpha

logger = logging.getLogger(__name__)


def _strip_trim_source_left(
    img: Image.Image,
    *,
    source_trim_left_frac: float | None,
) -> Image.Image:
    """Remove a fraction of width from the left (strip cue); band logic is separate."""
    if source_trim_left_frac is not None and source_trim_left_frac > 0:
        frac = min(0.49, max(0.0, float(source_trim_left_frac)))
        iw, ih = img.size
        cut = int(round(iw * frac))
        if cut > 0 and cut < iw - 1:
            img = img.crop((cut, 0, iw, ih))
    return img


def _cover_resize_and_crop_panned(
    img: Image.Image,
    target_width: int,
    target_height: int,
    pan_x: float,
    pan_y: float,
) -> Image.Image:
    """Aspect-cover into box with pan on excess."""
    tw, th = target_width, target_height
    width_ratio = tw / img.width
    height_ratio = th / img.height
    ratio = max(width_ratio, height_ratio)
    new_width = max(1, int(img.width * ratio))
    new_height = max(1, int(img.height * ratio))
    img = img.resize((new_width, new_height), Image.Resampling.BILINEAR)
    excess_w = new_width - tw
    excess_h = new_height - th
    left = int(round((1.0 + pan_x) * excess_w / 2.0)) if excess_w > 0 else 0
    top = int(round((1.0 + pan_y) * excess_h / 2.0)) if excess_h > 0 else 0
    left = max(0, min(excess_w, left))
    top = max(0, min(excess_h, top))
    return img.crop((left, top, left + tw, top + th))


def _strip_two_h_height_then_center_band(
    img: Image.Image,
    slot_height: int,
    band_frac: float,
) -> Image.Image:
    """Scale to full slot height, then take centered horizontal strip (frac × that height)."""
    iw, ih = img.size
    th = max(1, int(slot_height))
    s = th / float(max(1, ih))
    nw = max(1, int(round(iw * s)))
    img = img.resize((nw, th), Image.Resampling.BILINEAR)
    frac = max(0.05, min(1.0, float(band_frac)))
    band_h = max(1, int(round(th * frac)))
    top = max(0, (th - band_h) // 2)
    return img.crop((0, top, nw, min(th, top + band_h)))


def _cover_height_first_panned(
    img: Image.Image,
    target_width: int,
    target_height: int,
    pan_x: float,
    pan_y: float,
) -> Image.Image:
    """Fill slot height, crop excess width centered; if too narrow, fill width and crop height."""
    tw, th = target_width, target_height
    iw, ih = img.size
    s = th / float(max(1, ih))
    nw = max(1, int(round(iw * s)))
    img = img.resize((nw, th), Image.Resampling.BILINEAR)
    if nw >= tw:
        excess_w = nw - tw
        left = int(round((1.0 + pan_x) * excess_w / 2.0)) if excess_w > 0 else 0
        left = max(0, min(excess_w, left))
        return img.crop((left, 0, left + tw, th))
    nh_new = max(th, int(round(th * (tw / float(nw)))))
    img = img.resize((tw, nh_new), Image.Resampling.BILINEAR)
    excess_h = nh_new - th
    top = int(round((1.0 + pan_y) * excess_h / 2.0)) if excess_h > 0 else 0
    top = max(0, min(excess_h, top))
    return img.crop((0, top, tw, top + th))


def _process_image_impl(
    image_path: Path,
    target_width: int,
    target_height: int,
    required_orientation: str,
    pan_x: float = 0.0,
    pan_y: float = 0.0,
    *,
    flip_h: bool = False,
    grayscale: bool = False,
    source_trim_left_frac: float | None = None,
    source_center_band_frac: float | None = None,
    cover_height_first: bool = False,
):
    pan_x = max(-1.0, min(1.0, float(pan_x)))
    pan_y = max(-1.0, min(1.0, float(pan_y)))
    try:
        with Image.open(image_path) as img:
            if img.format == "JPEG":
                img.draft("RGB", (target_width, target_height))

            img = fix_orientation(img)

            is_horizontal = img.width > img.height
            if required_orientation == "horizontal" and not is_horizontal:
                return None
            if required_orientation == "vertical" and is_horizontal:
                return None

            img = flatten_alpha(img)
            img = _strip_trim_source_left(img, source_trim_left_frac=source_trim_left_frac)

            tw, th = target_width, target_height
            if source_center_band_frac is not None:
                img = _strip_two_h_height_then_center_band(
                    img, th, source_center_band_frac
                )
                img = _cover_resize_and_crop_panned(img, tw, th, pan_x, pan_y)
            elif cover_height_first:
                img = _cover_height_first_panned(img, tw, th, pan_x, pan_y)
            else:
                img = _cover_resize_and_crop_panned(img, tw, th, pan_x, pan_y)

            if flip_h:
                img = ImageOps.mirror(img)
            if grayscale:
                img = ImageOps.grayscale(img).convert("RGB")
            return img

    except Exception as e:
        logger.debug("process_image failed %s: %s", image_path, e)
        return None


def process_image_panned(
    image_path: Path,
    target_width: int,
    target_height: int,
    required_orientation: str,
    pan_x: float = 0.0,
    pan_y: float = 0.0,
    *,
    flip_h: bool = False,
    grayscale: bool = False,
    source_trim_left_frac: float | None = None,
    source_center_band_frac: float | None = None,
    cover_height_first: bool = False,
):
    """Uncached crop with pan and mural-specific source transforms."""
    return _process_image_impl(
        image_path,
        target_width,
        target_height,
        required_orientation,
        pan_x,
        pan_y,
        flip_h=flip_h,
        grayscale=grayscale,
        source_trim_left_frac=source_trim_left_frac,
        source_center_band_frac=source_center_band_frac,
        cover_height_first=cover_height_first,
    )


def process_image_contained_panned(
    image_path: Path,
    target_width: int,
    target_height: int,
    required_orientation: str,
    pan_x: float = 0.0,
    pan_y: float = 0.0,
    *,
    flip_h: bool = False,
    grayscale: bool = False,
    fill_rgb: Tuple[int, int, int] = (255, 255, 255),
    source_trim_left_frac: float | None = None,
    source_center_band_frac: float | None = None,
):
    """Scale image to fit inside the box (letterbox / pillarbox on ``fill_rgb``). Pan nudges within margins."""
    pan_x = max(-1.0, min(1.0, float(pan_x)))
    pan_y = max(-1.0, min(1.0, float(pan_y)))
    try:
        with Image.open(image_path) as img:
            if img.format == "JPEG":
                img.draft("RGB", (target_width, target_height))

            img = fix_orientation(img)

            is_horizontal = img.width > img.height
            if required_orientation == "horizontal" and not is_horizontal:
                return None
            if required_orientation == "vertical" and is_horizontal:
                return None

            img = flatten_alpha(img)
            img = _strip_trim_source_left(img, source_trim_left_frac=source_trim_left_frac)
            if source_center_band_frac is not None:
                img = _strip_two_h_height_then_center_band(
                    img, target_height, source_center_band_frac
                )
            if flip_h:
                img = ImageOps.mirror(img)

            iw, ih = img.size
            tw, th = target_width, target_height
            scale = min(tw / iw, th / ih)
            nw = max(1, int(round(iw * scale)))
            nh = max(1, int(round(ih * scale)))
            img = img.resize((nw, nh), Image.Resampling.BILINEAR)

            plate = Image.new("RGB", (tw, th), fill_rgb)
            excess_x = tw - nw
            excess_y = th - nh
            px = int(round((1.0 + pan_x) * excess_x / 2.0)) if excess_x > 0 else 0
            py = int(round((1.0 + pan_y) * excess_y / 2.0)) if excess_y > 0 else 0
            px = max(0, min(excess_x, px))
            py = max(0, min(excess_y, py))
            plate.paste(img, (px, py))
            if grayscale:
                plate = ImageOps.grayscale(plate).convert("RGB")
            return plate

    except Exception as e:
        logger.debug("process_image_contained failed %s: %s", image_path, e)
        return None
