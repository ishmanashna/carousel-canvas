#!/usr/bin/env python3
"""subject-extract CLI dispatch."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

from cache import (  # noqa: E402
    EXIT_DOCTOR_FAILED,
    EXIT_INVALID_PROMPTS,
    EXIT_MISSING_WEIGHTS,
    EXIT_OK,
    EXIT_OTHER,
    EXIT_TIMEOUT,
    set_cache_env,
)
from checker_cmd import compose_checker  # noqa: E402
from compose_cmd import (  # noqa: E402
    run_alpha_close,
    run_clip_alpha,
    run_crop,
    run_crop_rembg,
    run_erase_alpha,
    run_grow_color,
    run_lasso,
    run_lasso_preview,
    run_lift_alpha,
    run_paste,
    run_seed,
    run_subtract,
    run_union,
    run_wipe_islands,
    run_work_prep,
)
from doctor import run_doctor  # noqa: E402
from gaps_cmd import run_fill_gaps, run_propose  # noqa: E402
from grid_cmd import run_grid  # noqa: E402
from info_cmd import run_info  # noqa: E402
from keep_best import promote, restore  # noqa: E402
from rembg_cmd import run_rembg  # noqa: E402
from sam2_cmd import run_sam2_apply, run_sam2_generate, run_sam2_refine  # noqa: E402
from sam_cmd import run_sam_box, run_sam_points  # noqa: E402
from timing import Span, log_command  # noqa: E402

EPILOG = """
Exit codes:
  0   ok
  1   other error
  2   doctor failed / venv missing
  10  missing weights
  11  timeout
  12  invalid prompts.json

PowerShell: use ; not && to chain commands.
Example: python tools/subject-extract/prefetch.py ; powershell -File tools/subject-extract/sx.ps1 doctor
"""


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="subject-extract",
        description="Shared subject-extract toolkit for carousel-canvas agents.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=EPILOG,
    )
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("doctor", help="Check venv, imports, weights, disk space")

    info_p = sub.add_parser("info", help="Print full-res WIDTH HEIGHT")
    info_p.add_argument("image")

    grid_p = sub.add_parser(
        "grid",
        help="Write a 12-cell overlay (4x3 landscape / 3x4 portrait) + meta JSON",
    )
    grid_p.add_argument("image")
    grid_p.add_argument("out_png")
    grid_p.add_argument("--grid-long-side", type=int, default=2048)

    checker_p = sub.add_parser("checker", help="Compose cutout on checkerboard preview")
    checker_p.add_argument("cutout_png")
    checker_p.add_argument("out_png")
    checker_p.add_argument(
        "--full-res-checker",
        action="store_true",
        help="Write full-res checker to OUT; also writes OUT_preview.png at long-side <=1600",
    )

    rembg_p = sub.add_parser("rembg", help="Run rembg foreground model (no download)")
    rembg_p.add_argument("--model", required=True)
    rembg_p.add_argument("image")
    rembg_p.add_argument("out_png")

    sam_points_p = sub.add_parser("sam-points", help="rembg SAM with point prompts JSON")
    sam_points_p.add_argument("--prompts", required=True)
    sam_points_p.add_argument("image")
    sam_points_p.add_argument("out_png")

    sam_box_p = sub.add_parser("sam-box", help="rembg SAM with box + required point prompts")
    sam_box_p.add_argument("--xyxy", required=True, help="x1,y1,x2,y2 in full-res pixels")
    sam_box_p.add_argument("--prompts", required=True)
    sam_box_p.add_argument("image")
    sam_box_p.add_argument("out_png")

    gen_p = sub.add_parser("sam2-generate", help="SAM2 automatic masks + contact sheet")
    gen_p.add_argument("image")
    gen_p.add_argument("--out-dir", required=True)
    gen_p.add_argument("--max-side", type=int, default=1024)
    gen_p.add_argument("--points-per-side", type=int, default=16)
    gen_p.add_argument("--crop-n-layers", type=int, default=0)
    gen_p.add_argument(
        "--timeout-seconds",
        type=int,
        default=0,
        help="Kill generate after N seconds. 0 = wait until finished (default).",
    )
    gen_p.add_argument("--max-masks", type=int, default=32)

    apply_p = sub.add_parser("sam2-apply", help="OR selected SAM2 masks to full-res cutout")
    apply_p.add_argument("--ids", required=True, help="Comma-separated mask ids, e.g. 0,13,14")
    apply_p.add_argument("image")
    apply_p.add_argument("--out-dir", required=True)

    refine_p = sub.add_parser("sam2-refine", help="SAM2 refine with full-res include/exclude points")
    refine_p.add_argument("--include", default="[]", help='JSON array of [x,y] points')
    refine_p.add_argument("--exclude", default="[]", help='JSON array of [x,y] points')
    refine_p.add_argument("image")
    refine_p.add_argument("--out-dir", required=True)

    keep_p = sub.add_parser("keep-best", help="Promote or restore best cutout snapshot")
    keep_sub = keep_p.add_subparsers(dest="keep_action", required=True)
    promote_p = keep_sub.add_parser("promote", help="Copy cutout/checker to best.*")
    promote_p.add_argument("dir")
    restore_p = keep_sub.add_parser("restore", help="Copy best.* back to cutout/checker")
    restore_p.add_argument("dir")

    seed_p = sub.add_parser("seed", help="Copy an existing cutout.png into OUT_DIR")
    seed_p.add_argument("--from", dest="from_path", required=True)
    seed_p.add_argument("out_dir")

    crop_p = sub.add_parser("crop", help="Crop a rectangle for looking (quote --xyxy)")
    crop_p.add_argument("--xyxy", required=True, help='"x1,y1,x2,y2" full-res')
    crop_p.add_argument("image")
    crop_p.add_argument("out_png")

    union_p = sub.add_parser(
        "union",
        help="OR two alphas; RGB taken from the original photo",
    )
    union_p.add_argument("--image", required=True, help="Original full-res photo")
    union_p.add_argument("a_png")
    union_p.add_argument("b_png")
    union_p.add_argument("out_png")

    sub_p = sub.add_parser(
        "subtract",
        help="Zero cutout alpha wherever the donor is opaque; RGB from the original photo",
    )
    sub_p.add_argument("--image", required=True, help="Original full-res photo")
    sub_p.add_argument("cutout_png")
    sub_p.add_argument("donor_png")
    sub_p.add_argument("out_png")

    clip_p = sub.add_parser("clip-alpha", help="Zero alpha outside a box")
    clip_p.add_argument("--xyxy", required=True, help='"x1,y1,x2,y2" full-res')
    clip_p.add_argument("cutout_png")
    clip_p.add_argument("out_png")

    erase_p = sub.add_parser(
        "wipe",
        aliases=["erase-alpha"],
        help="Zero alpha inside a box (Paint-style delete). Quote --xyxy.",
    )
    erase_p.add_argument("--xyxy", required=True, help='"x1,y1,x2,y2" full-res')
    erase_p.add_argument("cutout_png")
    erase_p.add_argument("out_png")

    lasso_p = sub.add_parser(
        "lasso",
        help="Zero alpha inside a polygon. Quote --poly. RGB stays.",
    )
    lasso_p.add_argument("--poly", required=True, help='"x,y;x,y;x,y" at least 3 points')
    lasso_p.add_argument("--origin", default=None, help='"x,y" added to every point (crop top-left)')
    lasso_p.add_argument("cutout_png")
    lasso_p.add_argument("out_png")

    preview_p = sub.add_parser(
        "lasso-preview",
        help="Draw the polygon on a copy. Does not change the cutout.",
    )
    preview_p.add_argument("--poly", required=True, help='"x,y;x,y;x,y"')
    preview_p.add_argument("--origin", default=None, help='"x,y" crop top-left')
    preview_p.add_argument("image")
    preview_p.add_argument("out_png")

    islands_p = sub.add_parser(
        "wipe-islands",
        help="Drop floating alpha blobs; keep people and bits within --link-px of them",
    )
    islands_p.add_argument("--min-person-frac", type=float, default=0.08)
    islands_p.add_argument("--link-px", type=int, default=16)
    islands_p.add_argument("cutout_png")
    islands_p.add_argument("out_png")

    crop_rembg_p = sub.add_parser(
        "crop-rembg",
        help="Run rembg on a crop, paste into a full-res transparent canvas",
    )
    crop_rembg_p.add_argument("--model", required=True)
    crop_rembg_p.add_argument("--xyxy", required=True, help='"x1,y1,x2,y2" full-res')
    crop_rembg_p.add_argument("image")
    crop_rembg_p.add_argument("out_png")

    close_p = sub.add_parser("alpha-close", help="Morphological close on alpha")
    close_p.add_argument("--radius", type=int, default=8)
    close_p.add_argument("cutout_png")
    close_p.add_argument("out_png")

    grow_p = sub.add_parser(
        "grow-color",
        help="Flood-fill similar Lab color from a point, clipped to a box, OR into seed alpha",
    )
    grow_p.add_argument("--point", required=True, help='"x,y" full-res, quote it')
    grow_p.add_argument("--xyxy", required=True, help='"x1,y1,x2,y2" full-res, quote it')
    grow_p.add_argument("--tolerance", type=float, default=22.0)
    grow_p.add_argument("--image", required=True, help="Original full-res photo")
    grow_p.add_argument("seed_png")
    grow_p.add_argument("out_png")

    work_p = sub.add_parser(
        "work-prep",
        help="Downscale original (and optional seed alpha) to --max-side; write work_orig.png, work_meta.json",
    )
    work_p.add_argument("--max-side", type=int, default=2048)
    work_p.add_argument("--seed", default=None, help="Optional full-res seed cutout.png")
    work_p.add_argument("image", help="Full-res original photo")
    work_p.add_argument("out_dir")

    lift_p = sub.add_parser(
        "lift-alpha",
        help="Upscale a working-res cutout alpha onto the full-res original RGB",
    )
    lift_p.add_argument("--image", required=True, help="Full-res original photo")
    lift_p.add_argument("--meta", required=True, help="work_meta.json from work-prep")
    lift_p.add_argument("work_cutout_png")
    lift_p.add_argument("out_png")

    paste_p = sub.add_parser(
        "paste",
        help="Paste a crop-sized RGBA into a full-res transparent canvas at --xyxy",
    )
    paste_p.add_argument("--xyxy", required=True, help='"x1,y1,x2,y2" full-res crop origin')
    paste_p.add_argument("--image", required=True, help="Full-res original photo")
    paste_p.add_argument("piece_png")
    paste_p.add_argument("out_png")

    propose_p = sub.add_parser(
        "propose",
        help="Find hull-gap regions the person-cutter dropped; write candidates.json + overlay",
    )
    propose_p.add_argument("cutout_png")
    propose_p.add_argument("out_json")
    propose_p.add_argument("overlay_png")

    fill_p = sub.add_parser(
        "fill-gaps",
        help="03.1 add: SAM-box each proposed hull gap that touches the person, then union",
    )
    fill_p.add_argument("--image", required=True, help="Working-res original (work_orig.png)")
    fill_p.add_argument("--max", dest="max_candidates", type=int, default=3)
    fill_p.add_argument("cutout_png")
    fill_p.add_argument("out_png")

    return parser


def _dispatch(args: argparse.Namespace) -> int:
    if args.command == "doctor":
        return run_doctor()
    if args.command == "info":
        return run_info(args.image)
    if args.command == "grid":
        return run_grid(args.image, args.out_png, grid_long_side=args.grid_long_side)
    if args.command == "checker":
        return compose_checker(
            args.cutout_png,
            args.out_png,
            full_res=args.full_res_checker,
        )
    if args.command == "rembg":
        return run_rembg(args.model, args.image, args.out_png)
    if args.command == "sam-points":
        return run_sam_points(args.prompts, args.image, args.out_png)
    if args.command == "sam-box":
        return run_sam_box(args.xyxy, args.prompts, args.image, args.out_png)
    if args.command == "sam2-generate":
        return run_sam2_generate(
            args.image,
            args.out_dir,
            max_side=args.max_side,
            points_per_side=args.points_per_side,
            crop_n_layers=args.crop_n_layers,
            timeout_seconds=args.timeout_seconds,
            max_masks=args.max_masks,
        )
    if args.command == "sam2-apply":
        ids = [int(x.strip()) for x in args.ids.split(",") if x.strip()]
        return run_sam2_apply(args.image, args.out_dir, ids)
    if args.command == "sam2-refine":
        return run_sam2_refine(args.image, args.out_dir, args.include, args.exclude)
    if args.command == "keep-best":
        if args.keep_action == "promote":
            return promote(args.dir)
        return restore(args.dir)
    if args.command == "seed":
        return run_seed(args.from_path, args.out_dir)
    if args.command == "crop":
        return run_crop(args.image, args.xyxy, args.out_png)
    if args.command == "union":
        return run_union(args.image, args.a_png, args.b_png, args.out_png)
    if args.command == "subtract":
        return run_subtract(args.image, args.cutout_png, args.donor_png, args.out_png)
    if args.command == "clip-alpha":
        return run_clip_alpha(args.cutout_png, args.xyxy, args.out_png)
    if args.command in ("wipe", "erase-alpha"):
        return run_erase_alpha(args.cutout_png, args.xyxy, args.out_png)
    if args.command == "lasso":
        return run_lasso(args.cutout_png, args.poly, args.out_png, args.origin)
    if args.command == "lasso-preview":
        return run_lasso_preview(args.image, args.poly, args.out_png, args.origin)
    if args.command == "wipe-islands":
        return run_wipe_islands(
            args.cutout_png,
            args.out_png,
            min_person_frac=args.min_person_frac,
            link_px=args.link_px,
        )
    if args.command == "crop-rembg":
        return run_crop_rembg(args.model, args.image, args.xyxy, args.out_png)
    if args.command == "alpha-close":
        return run_alpha_close(args.cutout_png, args.out_png, args.radius)
    if args.command == "grow-color":
        return run_grow_color(
            args.image,
            args.seed_png,
            args.out_png,
            args.point,
            args.xyxy,
            args.tolerance,
        )
    if args.command == "work-prep":
        return run_work_prep(args.image, args.seed, args.out_dir, args.max_side)
    if args.command == "lift-alpha":
        return run_lift_alpha(args.image, args.work_cutout_png, args.out_png, args.meta)
    if args.command == "paste":
        return run_paste(args.image, args.piece_png, args.xyxy, args.out_png)
    if args.command == "propose":
        return run_propose(args.cutout_png, args.out_json, args.overlay_png)
    if args.command == "fill-gaps":
        return run_fill_gaps(
            args.image,
            args.cutout_png,
            args.out_png,
            args.max_candidates,
        )
    print(f"unknown command: {args.command}", file=sys.stderr)
    return EXIT_OTHER


def main(argv: list[str] | None = None) -> int:
    set_cache_env()
    parser = build_parser()
    args = parser.parse_args(argv)
    logged_argv = list(argv) if argv is not None else sys.argv[1:]
    span = Span()
    code = EXIT_OTHER
    try:
        code = _dispatch(args)
    except KeyboardInterrupt:
        print("interrupted", file=sys.stderr)
        code = EXIT_OTHER
    except Exception as exc:  # noqa: BLE001 - CLI boundary
        print(f"error: {exc!s}".encode("ascii", "backslashreplace").decode("ascii"), file=sys.stderr)
        code = EXIT_OTHER
    finally:
        log_command(args.command, span.seconds(), code, logged_argv)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
