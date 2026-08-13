#!/usr/bin/env python3
"""
Regenerate one strip export for visual A/B after template or composer changes.

Run from the project root:

  python scripts/strip_iteration_smoke.py "C:\\path\\to\\photos"
  python scripts/strip_iteration_smoke.py "TEST IMAGES" --template strip_seamless_mosaic_v1 --seed 42 --smart

Uses a fixed RNG seed by default so the same folder assigns the same photos to slots
each run (unless you change slot count or photo set). Open the *_wide.jpg in output.

This does not auto-detect layout bugs; the loop is: run script, look, edit, run again.
"""

from __future__ import annotations

import argparse
import random
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from app.io import get_valid_paths, parse_color
from app.strip import StripImageFill, export_strip_carousel, pick_strip_fills
from app.strip.assign import pick_underfill_paths
from app.strip.layout_retry import (
    pick_best_layout_seed_with_token_retry,
    strip_layout_token_retry_enabled,
    underfill_rng_from_layout_seed,
)
from app.strip.pipeline import validate_strip_unique_sources
from app.strip.template_data import get_template_by_id, strip_background_underfill_count


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("folder", type=Path, help="Folder with JPG/PNG (mixed orientation)")
    p.add_argument(
        "--template",
        default="strip_seamless_mosaic_v1",
        help="strip_seamless_mosaic_v1 (default) | strip_mural_v2 | strip_polaroid_table_v1 | strip_seamless_v1 | strip_mural_v1 | strip_10col",
    )
    p.add_argument(
        "--output",
        type=Path,
        default=ROOT / "output" / "strip_smoke",
        help="Output directory (created if missing)",
    )
    p.add_argument("--seed", type=int, default=42, help="RNG seed for shuffle / smart assign")
    p.add_argument(
        "--layout-seed",
        type=int,
        default=0,
        help="Layout seed (mural jitter, polaroid scatter, seamless mosaic)",
    )
    p.add_argument(
        "--card-edge",
        choices=("borderless", "wedges", "border"),
        default="borderless",
        help="Transparent wedges, classic wedges, or border",
    )
    p.add_argument(
        "--border-color",
        default=None,
        help="With --card-edge border (default: template background string)",
    )
    p.add_argument("--smart", action="store_true", help="Use smart orientation matching (default: dumb shuffle)")
    p.add_argument("--dumb", action="store_true", help="Force random assignment (default with --seed)")
    p.add_argument(
        "--allow-repeats",
        action="store_true",
        help="Allow fewer distinct files than image slots (same file reused)",
    )
    p.add_argument(
        "--no-layout-retry",
        action="store_true",
        help="Skip mural v2 borderless token layout retry (5 seeds)",
    )
    args = p.parse_args()
    smart = bool(args.smart) and not bool(args.dumb)
    if not args.dumb and not args.smart:
        smart = False

    folder = args.folder.resolve()
    if not folder.is_dir():
        print(f"Not a directory: {folder}", file=sys.stderr)
        return 1

    tpl = get_template_by_id(args.template)
    paths = get_valid_paths(folder, "mixed")
    if len(paths) < 1:
        print(f"Need at least one image in {folder}", file=sys.stderr)
        return 1
    try:
        validate_strip_unique_sources(
            tpl, source_paths=paths, allow_repeats=bool(args.allow_repeats)
        )
    except ValueError as e:
        print(e, file=sys.stderr)
        return 1

    rng = random.Random(args.seed)
    chosen = pick_strip_fills(paths, tpl, smart=smart, rng=rng, layout_seed=args.layout_seed)
    fills = [StripImageFill(p) if p is not None else None for p in chosen]
    ce = args.card_edge
    eff_layout = int(args.layout_seed)
    if not args.no_layout_retry and strip_layout_token_retry_enabled(tpl.id, ce):
        eff_layout = pick_best_layout_seed_with_token_retry(
            fills, tpl, eff_layout, card_edge=ce
        )
    n_uf = strip_background_underfill_count(tpl)
    uf_rng = underfill_rng_from_layout_seed(eff_layout)
    uf_paths = pick_underfill_paths(paths, chosen, n_uf, uf_rng) if n_uf else None
    brgb = parse_color(args.border_color or tpl.background) if ce == "border" else None

    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    wide, slices = export_strip_carousel(
        fills,
        out,
        template=tpl,
        layout_seed=eff_layout,
        card_edge=ce,
        border_rgb=brgb,
        underfill_paths=uf_paths,
    )
    print(
        f"Template: {tpl.id}  seed={args.seed}  layout_seed={args.layout_seed} -> export {eff_layout}  "
        f"smart={smart}  card_edge={ce}"
    )
    print(f"Wide:     {wide}")
    for i, sp in enumerate(slices, start=1):
        print(f"Slice {i:02d}: {sp}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
