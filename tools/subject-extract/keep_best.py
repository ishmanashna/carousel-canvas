"""Promote or restore best cutout snapshots."""

from __future__ import annotations

import shutil
import sys
from pathlib import Path


def _copy_if_exists(src: Path, dst: Path) -> None:
    if src.is_file():
        shutil.copy2(src, dst)


def promote(dir_path: str) -> int:
    root = Path(dir_path)
    if not root.is_dir():
        print(f"error: directory not found: {root}", file=sys.stderr)
        return 1

    cutout = root / "cutout.png"
    if not cutout.is_file():
        print(f"error: missing {cutout}", file=sys.stderr)
        return 1

    _copy_if_exists(cutout, root / "best.png")
    for checker_name in ("cutout_checker.png", "checker.png"):
        _copy_if_exists(root / checker_name, root / "best_checker.png")
    for preview_name in (
        "cutout_checker_preview.png",
        "cutout_preview.png",
        "checker_preview.png",
        "preview.png",
    ):
        _copy_if_exists(root / preview_name, root / "best_preview.png")
    print(f"promoted best snapshot in {root}")
    return 0


def restore(dir_path: str) -> int:
    root = Path(dir_path)
    if not root.is_dir():
        print(f"error: directory not found: {root}", file=sys.stderr)
        return 1

    best = root / "best.png"
    if not best.is_file():
        print(f"error: missing {best}", file=sys.stderr)
        return 1

    _copy_if_exists(best, root / "cutout.png")
    _copy_if_exists(root / "best_checker.png", root / "cutout_checker.png")
    _copy_if_exists(root / "best_preview.png", root / "cutout_checker_preview.png")
    print(f"restored best snapshot in {root}")
    return 0
