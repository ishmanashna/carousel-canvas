"""Stable RGB per image path for fast token composes (layout scoring)."""

from __future__ import annotations

import hashlib
from pathlib import Path


def token_rgb_for_path(path: Path) -> tuple[int, int, int]:
    h = hashlib.blake2b(str(path.resolve()).encode("utf-8"), digest_size=8).digest()
    # Avoid near-white / near-beige (template bg); keep saturation visible
    r = 32 + (h[0] % 200)
    g = 32 + (h[1] % 200)
    b = 32 + (h[2] % 200)
    return (r, g, b)
