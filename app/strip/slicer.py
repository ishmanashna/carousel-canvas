from __future__ import annotations

from PIL import Image


def slice_wide_to_carousel(
    wide: Image.Image,
    slice_width: int,
    slice_height: int,
    count: int,
    overlap_px: int = 0,
) -> list[Image.Image]:
    """
    Cut a horizontal master image into ``count`` vertical frames (left → right).

    When ``overlap_px`` is 0, each slice is ``slice_width`` wide and slices tile
    the top of the image: ``left = i * slice_width``.

    When ``overlap_px`` > 0, slices advance by ``slice_width - overlap_px`` so
    adjacent frames share a band (template-driven).
    """
    if wide.width < slice_width or wide.height < slice_height:
        raise ValueError(
            f"Image {wide.size} smaller than slice {slice_width}×{slice_height}"
        )
    if count < 1:
        raise ValueError("count must be >= 1")
    if overlap_px < 0 or overlap_px >= slice_width:
        raise ValueError("overlap_px must be in [0, slice_width)")

    out: list[Image.Image] = []
    if overlap_px == 0:
        need_w = slice_width * count
        if wide.width < need_w:
            raise ValueError(
                f"Image width {wide.width} < required {need_w} for {count} non-overlapping slices"
            )
        for i in range(count):
            left = i * slice_width
            out.append(wide.crop((left, 0, left + slice_width, slice_height)).copy())
        return out

    stride = slice_width - overlap_px
    for i in range(count):
        left = i * stride
        right = left + slice_width
        if right > wide.width:
            raise ValueError(
                f"Slice {i + 1}/{count} extends past image width {wide.width} "
                f"(overlap_px={overlap_px}, stride={stride})"
            )
        out.append(wide.crop((left, 0, right, slice_height)).copy())
    return out
