"""Print full-resolution image dimensions."""

from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image, ImageOps


def run_info(image_path: str) -> int:
    path = Path(image_path)
    if not path.is_file():
        print(f"error: image not found: {path}", file=sys.stderr)
        return 1

    with Image.open(path) as img:
        img = ImageOps.exif_transpose(img)
        print(f"{img.width} {img.height}")
    return 0
