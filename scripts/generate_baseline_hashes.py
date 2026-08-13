#!/usr/bin/env python3
"""Export one strip per template and write SHA-256 hashes for deliverable slices (Phase 0)."""

from __future__ import annotations

import hashlib
import random
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from app.io import get_valid_paths
from app.strip import StripImageFill, export_strip_carousel, pick_strip_fills
from app.strip.template_data import STRIP_GUI_TEMPLATE_IDS, get_template_by_id


def _fixture_dir() -> Path:
    test_images = ROOT / "TEST IMAGES"
    if not test_images.is_dir():
        raise SystemExit(f"Missing fixture folder: {test_images}")
    return test_images


def _sha256(path: Path) -> str:
    h = hashlib.sha256()
    h.update(path.read_bytes())
    return h.hexdigest()


def main() -> int:
    folder = _fixture_dir()
    paths = get_valid_paths(folder, "mixed")
    if not paths:
        raise SystemExit("TEST IMAGES has no usable photos")

    out_root = ROOT / "tests" / "fixtures" / "golden"
    out_root.mkdir(parents=True, exist_ok=True)
    lines: list[str] = []

    for template_id in STRIP_GUI_TEMPLATE_IDS:
        tpl = get_template_by_id(template_id)
        out_dir = out_root / template_id
        out_dir.mkdir(parents=True, exist_ok=True)
        chosen = pick_strip_fills(paths, tpl, smart=True, rng=random.Random(1), layout_seed=7)
        fills = [StripImageFill(p) if p else None for p in chosen]
        wide_p, slices = export_strip_carousel(
            fills,
            out_dir,
            template=tpl,
            layout_seed=7,
            card_edge="borderless",
        )
        lines.append(f"# {template_id}")
        lines.append(f"{_sha256(wide_p)}  {template_id}/{wide_p.name}  (wide, uncapped)")
        for p in slices:
            lines.append(f"{_sha256(p)}  {template_id}/{p.name}")
        print(f"Exported {template_id}: wide + {len(slices)} slices")

    report = ROOT / "docs" / "BASELINE_HASHES.txt"
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"Report: {report}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
