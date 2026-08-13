"""Pin a specific file to the strip template flagship (hero) slot — CLI parity with GUI hero-only swap."""

from __future__ import annotations

import random
from pathlib import Path

from app.io import IMAGE_EXTENSIONS
from app.strip.fills import StripImageFill
from app.strip.template_data import StripTemplate, strip_effective_fill_required


def resolve_strip_hero_argument(folder: Path, spec: str) -> Path:
    """
    Resolve ``spec`` to an image path: existing file path, file under ``folder``, or unique
    substring match among top-level images in ``folder`` (same scope as ``get_valid_paths``).
    """
    s = spec.strip()
    if not s:
        raise ValueError("Hero image argument is empty.")

    p = Path(s).expanduser()
    if p.is_file() and p.suffix.lower() in IMAGE_EXTENSIONS:
        return p.resolve()

    rel = folder / s
    if rel.is_file() and rel.suffix.lower() in IMAGE_EXTENSIONS:
        return rel.resolve()

    frag = s.lower()
    matches: list[Path] = []
    try:
        for x in folder.iterdir():
            if (
                x.is_file()
                and x.suffix.lower() in IMAGE_EXTENSIONS
                and frag in x.name.lower()
            ):
                matches.append(x)
    except OSError as e:
        raise ValueError(f"Cannot read folder {folder}: {e}") from e

    matches.sort(key=lambda q: q.name.lower())
    if not matches:
        raise ValueError(
            f"No image in {folder} matches {spec!r} (tried path, {folder / s}, and filename substring)."
        )
    if len(matches) > 1:
        preview = ", ".join(m.name for m in matches[:8])
        more = f" (+{len(matches) - 8} more)" if len(matches) > 8 else ""
        raise ValueError(
            f"Hero spec {spec!r} is ambiguous ({len(matches)} files): {preview}{more}. "
            "Use a longer substring or a full file path."
        )
    return matches[0].resolve()


def pin_strip_hero_fill(
    fills: list[StripImageFill | None],
    template: StripTemplate,
    hero_path: Path,
    *,
    layout_seed: int | None,
    pool_paths: list[Path],
    allow_repeats: bool,
    rng: random.Random,
) -> None:
    """Set flagship slot to ``hero_path``; optionally dedupe other slots (when repeats disallowed)."""
    hi = getattr(template, "layout_flagship_slot_index", None)
    if hi is None:
        raise ValueError(
            f"Template {template.id!r} has no flagship (hero) slot — omit --strip-hero-image."
        )

    req = strip_effective_fill_required(template, layout_seed)
    if hi < 0 or hi >= len(fills) or hi >= len(req):
        raise ValueError(f"Invalid flagship slot index {hi} for current fill list.")
    if not req[hi]:
        raise ValueError(
            f"Flagship slot {hi} is not an image-required slot for this template/seed."
        )

    hero_r = hero_path.resolve()
    fills[hi] = StripImageFill(hero_path)

    if allow_repeats:
        return

    while True:
        dup_j: int | None = None
        for j, f in enumerate(fills):
            if j == hi or j >= len(req) or not req[j] or f is None:
                continue
            if Path(f.path).resolve() != hero_r:
                continue
            dup_j = j
            break
        if dup_j is None:
            break
        others: set[Path] = set()
        for k, fk in enumerate(fills):
            if k == dup_j or k >= len(req) or not req[k] or fk is None:
                continue
            others.add(Path(fk.path).resolve())
        spare = [p for p in pool_paths if p.resolve() not in others]
        if not spare:
            break
        fills[dup_j] = StripImageFill(spare[rng.randrange(len(spare))])
