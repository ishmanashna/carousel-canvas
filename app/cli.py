"""Command-line entry for Carousel Canvas (strip carousel export only)."""

from __future__ import annotations

import argparse
import logging
import random
import sys
from pathlib import Path

from app.io import get_valid_paths, parse_color
from app.strip import STRIP_SLICE_COUNT, StripImageFill, export_strip_carousel, pick_strip_fills
from app.strip.assign import pick_underfill_paths
from app.strip.hero_pin import pin_strip_hero_fill, resolve_strip_hero_argument
from app.strip.layout_retry import (
    pick_best_layout_seed_with_token_retry,
    strip_layout_token_retry_enabled,
    underfill_rng_from_layout_seed,
)
from app.strip.pipeline import validate_strip_unique_sources
from app.strip.template_data import get_template_by_id, strip_background_underfill_count

logging.basicConfig(level=logging.INFO, format="%(message)s")
logger = logging.getLogger(__name__)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Carousel Canvas - mural strip exporter")
    parser.add_argument("folder", nargs="?", default=".", help="Folder with images")
    parser.add_argument(
        "--strip",
        action="store_true",
        help=(
            f"Carousel strip: random photos to chosen template, "
            f"export {STRIP_SLICE_COUNT} vertical 1080x1350 JPEGs"
        ),
    )
    parser.add_argument(
        "--strip-template",
        choices=(
            "strip_mural_v2",
            "strip_polaroid_table_v1",
            "strip_seamless_mosaic_v1",
            "strip_seamless_v1",
            "strip_mural_v1",
            "strip_10col",
        ),
        default="strip_mural_v2",
        help="Strip template; slot count and layout vary (collage uses --strip-layout-seed for random layout)",
    )
    parser.add_argument(
        "--strip-dumb-shuffle",
        action="store_true",
        help="Random slot assignment (default: match landscape/wide and portrait/tall slots)",
    )
    parser.add_argument(
        "--strip-allow-repeats",
        action="store_true",
        help="Allow reusing the same files when the folder has fewer photos than image slots",
    )
    parser.add_argument(
        "--strip-layout-seed",
        type=int,
        default=0,
        help="Layout RNG seed (mural jitter, polaroid scatter, seamless mosaic geometry).",
    )
    parser.add_argument(
        "--strip-no-layout-retry",
        action="store_true",
        help="Skip token-based layout retry (mural v2 borderless tries 5 seeds by default).",
    )
    parser.add_argument(
        "--strip-hero-image",
        metavar="PATH_OR_NAME",
        default=None,
        help="Pin this image to the template flagship (hero) slot.",
    )
    parser.add_argument(
        "--strip-card-edge",
        choices=("borderless", "wedges", "border"),
        default="borderless",
        help="Transparent wedges, classic template-color wedges, or border (--color).",
    )
    parser.add_argument("--output", default="output", help="Output folder")
    parser.add_argument("--color", default="white", help="Color name (e.g., beige) or hex (e.g., #FFFFFF)")

    args = parser.parse_args(argv)
    if not args.strip:
        parser.error("Carousel Canvas CLI requires --strip (use script.py --strip …)")

    folder = Path(args.folder).resolve()
    if not folder.is_dir():
        logger.error("Folder does not exist: %s", folder)
        return 1

    try:
        tpl = get_template_by_id(args.strip_template)
        paths = get_valid_paths(folder, "mixed")
        if len(paths) < 1:
            logger.error("Strip needs at least one photo in the folder (H or V).")
            return 1
        try:
            validate_strip_unique_sources(
                tpl,
                source_paths=paths,
                allow_repeats=bool(args.strip_allow_repeats),
                layout_seed=int(args.strip_layout_seed),
            )
        except ValueError as e:
            logger.error("%s", e)
            return 1
        rng = random.Random()
        chosen = pick_strip_fills(
            paths,
            tpl,
            smart=not args.strip_dumb_shuffle,
            rng=rng,
            layout_seed=args.strip_layout_seed,
        )
        fills = [StripImageFill(p) if p is not None else None for p in chosen]
        if args.strip_hero_image:
            hero_p = resolve_strip_hero_argument(folder, args.strip_hero_image)
            pin_strip_hero_fill(
                fills,
                tpl,
                hero_p,
                layout_seed=int(args.strip_layout_seed),
                pool_paths=paths,
                allow_repeats=bool(args.strip_allow_repeats),
                rng=rng,
            )
            logger.info("Pinned hero image: %s", hero_p)
        ce = args.strip_card_edge
        eff_seed = int(args.strip_layout_seed)
        if not args.strip_no_layout_retry and strip_layout_token_retry_enabled(tpl.id, ce):
            eff_seed = pick_best_layout_seed_with_token_retry(
                fills, tpl, eff_seed, card_edge=ce
            )
        n_uf = strip_background_underfill_count(tpl)
        uf_rng = underfill_rng_from_layout_seed(eff_seed)
        uf_paths = pick_underfill_paths(paths, chosen, n_uf, uf_rng) if n_uf else None
        brgb = parse_color(args.color) if ce == "border" else None
        wide_p, out_paths = export_strip_carousel(
            fills,
            args.output,
            template=tpl,
            layout_seed=eff_seed,
            card_edge=ce,
            border_rgb=brgb,
            underfill_paths=uf_paths,
        )
        logger.info("Wrote wide master %s", wide_p)
        for p in out_paths:
            logger.info("Wrote %s", p)
        logger.info("\nAll tasks finished.")
    except Exception as e:
        logger.exception("[FATAL] %s", e)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
