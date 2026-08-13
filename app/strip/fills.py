"""Slot fill payload for strip export (no GUI dependency)."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


@dataclass
class StripImageFill:
    path: Path
    pan_x: float = 0.0
    pan_y: float = 0.0
    flip_h: bool = False
    grayscale: bool = False
