from __future__ import annotations

import random
from dataclasses import dataclass
from typing import Literal


@dataclass(frozen=True)
class StripSlotDef:
    """One image layer. (x,y,w,h) is the placement box; rotated pixels are centered in that box."""

    x: int
    y: int
    w: int
    h: int
    rotation_deg: float = 0.0
    z_index: int = 0
    fit: Literal["cover", "contain"] = "cover"
    prefer_portrait: bool = False
    # Smart fill: require landscape sources (slot aspect near 1:1 still counts as "any" without this).
    prefer_landscape: bool = False
    polaroid: bool = False
    # 2H stack: scale to slot height, centered horizontal strip = frac x that height (full width), then cover.
    horizontal_center_band_frac: float | None = None
    # Remove this fraction of source width from the left edge before fit (0..~0.49).
    source_trim_left_frac: float | None = None
    # Cover: fill slot height first, then crop excess width centered (or scale width if too narrow).
    cover_height_first: bool = False


@dataclass(frozen=True)
class StripTemplate:
    id: str
    canvas_width: int
    canvas_height: int
    slice_width: int
    slice_height: int
    slice_count: int
    overlap_px: int
    background: str
    slots: tuple[StripSlotDef, ...]
    slot_fill_required: tuple[bool, ...] | None = None
    layout_jitter_px: int = 0
    layout_cover_slot_index: int | None = None
    layout_cover_max_jitter: int = 14
    # Paint-order flagship: jitter main board first, then hero + small rim overlaps (see layout_jitter).
    layout_flagship_slot_index: int | None = None
    layout_flagship_max_jitter: int = 18
    layout_flagship_rim_slot_indices: tuple[int, ...] = ()
    # After full compose, paste N extra photos *under* all slots where beige still shows (borderless).
    background_underfill_layers: int = 0
    # Optional second underfill pass (same logic, stronger on residual pockets; used by mural v2).
    background_underfill_boost_layers: int = 0
    # Optional third pass with repeated large images over residual pockets after first two underfill passes.
    background_underfill_repeat_layers: int = 0
    # Extra pass: placements constrained to the wide-canvas band under the last N carousel slices (right side).
    background_tail_underfill_layers: int = 0
    background_tail_slice_count: int = 2
    # After the fixed slots, paste extra cover cards on detected beige (borderless murals only).
    gap_fill_max_layers: int = 0
    gap_fill_beige_tolerance: int = 48
    gap_fill_stop_ratio: float = 0.004
    # If set, full canvas from procedural_backgrounds (seeded by layout seed).
    procedural_background: str | None = None
    layout_placer: Literal["jitter", "organic_polaroid"] = "jitter"

    @property
    def num_slots(self) -> int:
        return len(self.slots)


def effective_slot_fill_required(tpl: StripTemplate) -> tuple[bool, ...]:
    if tpl.slot_fill_required is not None:
        if len(tpl.slot_fill_required) != len(tpl.slots):
            raise ValueError("slot_fill_required length must match slots")
        return tpl.slot_fill_required
    return tuple(True for _ in tpl.slots)


# --- Seamless mosaic v1 geometry (widths sum to 10800) ---
_M2H_W = 1200
_M2H_FRAC = 0.5
_MOSAIC_H2_W = 2200
_MOSAIC_H_TAIL_W = 1700
_FIRST_V_TRIM_LEFT = 0.08


def build_seamless_mosaic_v1_slots(layout_seed: int) -> tuple[StripSlotDef, ...]:
    """
    Seeded mosaic: first column always single V (with left trim); optional 2V on one of the next three
    portrait columns; column 2 as H | 2H | 3H (landscape); fixed 2H x 1200 stack; two tail H columns.
    """
    CH = 1350
    r = random.Random(int(layout_seed) & 0xFFFFFFFF)
    col2 = ("H", "2H", "3H")[r.randrange(3)]
    # 2V never on the lead portrait column (hero column keeps trim + single slot).
    twov_idx = r.choice((None, 1, 2, 3))
    trim = _FIRST_V_TRIM_LEFT
    slots: list[StripSlotDef] = []
    x = 0
    base_kw = {"rotation_deg": 0.0, "z_index": 0, "fit": "cover"}

    def v_kw() -> dict:
        return {**base_kw, "prefer_portrait": True}

    def h_kw() -> dict:
        return {**base_kw, "prefer_landscape": True}

    # Portrait column 0 (1000): always one V with left-edge trim
    slots.append(
        StripSlotDef(x, 0, 1000, CH, source_trim_left_frac=trim, **v_kw())
    )
    x += 1000

    # Column 2 (2200): always landscape - full H, or 2H, or 3H stack
    if col2 == "H":
        slots.append(
            StripSlotDef(x, 0, 2200, CH, cover_height_first=True, **h_kw())
        )
    elif col2 == "2H":
        slots.append(
            StripSlotDef(
                x, 0, 2200, 675, horizontal_center_band_frac=0.5, **h_kw()
            )
        )
        slots.append(
            StripSlotDef(
                x, 675, 2200, 675, horizontal_center_band_frac=0.5, **h_kw()
            )
        )
    else:
        h3 = CH // 3
        bf = 1.0 / 3.0
        for i in range(3):
            slots.append(
                StripSlotDef(
                    x, i * h3, 2200, h3, horizontal_center_band_frac=bf, **h_kw()
                )
            )
    x += 2200

    # Three full-height portrait columns (1000 each), optional 2V on one
    for j in range(3):
        col = 1 + j
        if twov_idx == col:
            slots.append(StripSlotDef(x, 0, 500, CH, **v_kw()))
            slots.append(StripSlotDef(x + 500, 0, 500, CH, **v_kw()))
        else:
            slots.append(StripSlotDef(x, 0, 1000, CH, **v_kw()))
        x += 1000

    # Fixed 2H stack (1200 wide)
    slots.append(
        StripSlotDef(
            x, 0, _M2H_W, 675, horizontal_center_band_frac=_M2H_FRAC, **h_kw()
        )
    )
    slots.append(
        StripSlotDef(
            x, 675, _M2H_W, 675, horizontal_center_band_frac=_M2H_FRAC, **h_kw()
        )
    )
    x += _M2H_W

    slots.append(
        StripSlotDef(x, 0, _MOSAIC_H_TAIL_W, CH, cover_height_first=True, **h_kw())
    )
    x += _MOSAIC_H_TAIL_W
    slots.append(
        StripSlotDef(x, 0, _MOSAIC_H_TAIL_W, CH, cover_height_first=True, **h_kw())
    )
    x += _MOSAIC_H_TAIL_W
    assert x == 10800
    return tuple(slots)


def mosaic_v1_max_slot_count() -> int:
    return max(len(build_seamless_mosaic_v1_slots(s)) for s in range(64))


def strip_effective_fill_required(
    tpl: StripTemplate, layout_seed: int | None = None
) -> tuple[bool, ...]:
    """Required image slots for export/preview; mosaic layout depends on ``layout_seed``."""
    if tpl.id == "strip_seamless_mosaic_v1":
        eff = 0 if layout_seed is None else int(layout_seed)
        return (True,) * len(build_seamless_mosaic_v1_slots(eff))
    return effective_slot_fill_required(tpl)


def strip_effective_num_slots(tpl: StripTemplate, layout_seed: int | None = None) -> int:
    return len(strip_effective_fill_required(tpl, layout_seed))


def strip_image_slot_count(tpl: StripTemplate, layout_seed: int | None = None) -> int:
    if tpl.id == "strip_seamless_mosaic_v1" and layout_seed is None:
        return mosaic_v1_max_slot_count()
    return sum(1 for r in strip_effective_fill_required(tpl, layout_seed) if r)


def strip_background_underfill_count(tpl: StripTemplate) -> int:
    base = max(0, int(getattr(tpl, "background_underfill_layers", 0) or 0))
    boost = max(0, int(getattr(tpl, "background_underfill_boost_layers", 0) or 0))
    repeat = max(0, int(getattr(tpl, "background_underfill_repeat_layers", 0) or 0))
    tail = max(0, int(getattr(tpl, "background_tail_underfill_layers", 0) or 0))
    return base + boost + repeat + tail


# --- Literal 10-column strip (one photo per carousel column; not a "mural") ---
_STRIP_10COL_SLOTS: tuple[StripSlotDef, ...] = tuple(
    StripSlotDef(x=i * 1080, y=0, w=1080, h=1350, z_index=i) for i in range(10)
)

TEMPLATE_STRIP_10COL = StripTemplate(
    id="strip_10col",
    canvas_width=10800,
    canvas_height=1350,
    slice_width=1080,
    slice_height=1350,
    slice_count=10,
    overlap_px=0,
    background="white",
    slots=_STRIP_10COL_SLOTS,
)


# --- Mural v1 (legacy): portrait-heavy overlapping cards ---
_STRIP_MURAL_V1_SLOTS: tuple[StripSlotDef, ...] = (
    StripSlotDef(x=0, y=480, w=10800, h=870, rotation_deg=0.0, z_index=0),
    StripSlotDef(x=-60, y=40, w=1280, h=1180, rotation_deg=-2.2, z_index=1),
    StripSlotDef(x=1180, y=90, w=1020, h=1040, rotation_deg=3.0, z_index=2),
    StripSlotDef(x=2480, y=30, w=1180, h=1180, rotation_deg=-3.5, z_index=3),
    StripSlotDef(x=3780, y=140, w=980, h=1020, rotation_deg=2.5, z_index=4),
    StripSlotDef(x=4980, y=20, w=1220, h=1220, rotation_deg=-4.0, z_index=5),
    StripSlotDef(x=6320, y=80, w=1060, h=1080, rotation_deg=3.2, z_index=6),
    StripSlotDef(x=7720, y=50, w=1240, h=1200, rotation_deg=-2.8, z_index=7),
)

def _gen_mural_v2_main_board() -> tuple[StripSlotDef, ...]:
    """32 overlapping medium tiles - see docs/SPEC_STRIP_MURAL_V2.md."""
    rng = random.Random(881_122)
    CW, CH = 10800, 1350
    min_w, max_w = 760, 2000
    min_h, max_h = 560, 880
    slots: list[StripSlotDef] = []
    z = 0
    positions: list[tuple[int, int]] = [(c, r) for c in range(10) for r in range(4)]
    positions = positions[:32]
    for col, row in positions:
        cx = col * 1080 + rng.randint(320, 760)
        cy = row * 290 + rng.randint(120, 340)
        w = rng.randint(min_w, max_w)
        h = rng.randint(min_h, max_h)
        x = int(cx - w / 2 + rng.randint(-55, 55))
        y = int(cy - h / 2 + rng.randint(-45, 45))
        x = max(-120, min(CW - w + 220, x))
        y = max(-70, min(CH - h + 140, y))
        rot = rng.uniform(-2.35, 2.35)
        slots.append(StripSlotDef(x=x, y=y, w=w, h=h, rotation_deg=rot, z_index=z))
        z += 1
    return tuple(slots)


# 32 main + 2 rim + 1 flagship = 35 slots. Hero slice 1; gap-fill OFF per spec.
_STRIP_MURAL_V2_MAIN = _gen_mural_v2_main_board()
# Rims z < hero so they paint first: hero stays on top (slice 1); rims peek at transparent corners only.
_STRIP_MURAL_V2_SLOTS: tuple[StripSlotDef, ...] = _STRIP_MURAL_V2_MAIN + (
    StripSlotDef(x=0, y=0, w=500, h=400, rotation_deg=2.05, z_index=85),
    StripSlotDef(x=0, y=0, w=480, h=380, rotation_deg=-1.95, z_index=86),
    StripSlotDef(
        x=52,
        y=78,
        w=976,
        h=1194,
        rotation_deg=-1.85,
        z_index=90,
        prefer_portrait=True,
    ),
)

# overlap_px > 0 needs canvas_width >= slice_count * slice_width - (slice_count - 1) * overlap_px
# (here 10800 = 10*1080, so overlap must be 0 unless you widen the canvas)
TEMPLATE_STRIP_MURAL_V1 = StripTemplate(
    id="strip_mural_v1",
    canvas_width=10800,
    canvas_height=1350,
    slice_width=1080,
    slice_height=1350,
    slice_count=10,
    overlap_px=0,
    background="#ece8e3",
    slots=_STRIP_MURAL_V1_SLOTS,
)

TEMPLATE_STRIP_MURAL_V2 = StripTemplate(
    id="strip_mural_v2",
    canvas_width=10800,
    canvas_height=1350,
    slice_width=1080,
    slice_height=1350,
    slice_count=10,
    overlap_px=0,
    background="#ece8e3",
    slots=_STRIP_MURAL_V2_SLOTS,
    layout_jitter_px=56,
    layout_cover_slot_index=None,
    layout_flagship_slot_index=34,
    layout_flagship_max_jitter=12,
    layout_flagship_rim_slot_indices=(32, 33),
    background_underfill_layers=10,
    background_underfill_boost_layers=5,
    background_underfill_repeat_layers=9,
    background_tail_underfill_layers=18,
    background_tail_slice_count=1,
    gap_fill_max_layers=0,
    gap_fill_beige_tolerance=48,
    gap_fill_stop_ratio=0.004,
)

# --- Seamless v1: layered photos only, mild overlap, tiny tilt (~Â±0.5Â°); dark field; 9 slots ---
_STRIP_SEAMLESS_V1_SLOTS: tuple[StripSlotDef, ...] = (
    StripSlotDef(x=0, y=410, w=10800, h=940, rotation_deg=0.0, z_index=0),
    StripSlotDef(x=-35, y=72, w=1320, h=1010, rotation_deg=-0.52, z_index=1),
    StripSlotDef(x=920, y=38, w=1720, h=1040, rotation_deg=0.42, z_index=2),
    StripSlotDef(x=2480, y=88, w=1460, h=990, rotation_deg=-0.38, z_index=3),
    StripSlotDef(x=3780, y=22, w=1980, h=1088, rotation_deg=0.48, z_index=4),
    StripSlotDef(x=5420, y=62, w=1620, h=1035, rotation_deg=-0.45, z_index=5),
    StripSlotDef(x=6780, y=32, w=1860, h=1075, rotation_deg=0.4, z_index=6),
    StripSlotDef(x=8280, y=78, w=1520, h=1005, rotation_deg=-0.42, z_index=7),
    StripSlotDef(x=9060, y=28, w=1740, h=1032, rotation_deg=0.36, z_index=8),
)

TEMPLATE_STRIP_SEAMLESS_V1 = StripTemplate(
    id="strip_seamless_v1",
    canvas_width=10800,
    canvas_height=1350,
    slice_width=1080,
    slice_height=1350,
    slice_count=10,
    overlap_px=0,
    background="#101010",
    slots=_STRIP_SEAMLESS_V1_SLOTS,
)

# --- Seamless mosaic v1: seeded mix of V / 2V, H / 2H / 3H (col 2), fixed 2H x 1200, tail H H ---
_STRIP_SEAMLESS_MOSAIC_V1_SLOTS: tuple[StripSlotDef, ...] = build_seamless_mosaic_v1_slots(0)

TEMPLATE_STRIP_SEAMLESS_MOSAIC_V1 = StripTemplate(
    id="strip_seamless_mosaic_v1",
    canvas_width=10800,
    canvas_height=1350,
    slice_width=1080,
    slice_height=1350,
    slice_count=10,
    overlap_px=0,
    background="#121212",
    slots=_STRIP_SEAMLESS_MOSAIC_V1_SLOTS,
    layout_jitter_px=0,
)

# --- Polaroid table: uniform cards; x/y here are placeholders (organic_polaroid placer replaces). ---
_POLAROID_CARD_W = 740
_POLAROID_CARD_H = 920
_POLAROID_Z_ORDER = (48,) + tuple(range(3, 26))
_STRIP_POLAROID_TABLE_SLOTS: tuple[StripSlotDef, ...] = tuple(
    StripSlotDef(
        0,
        0,
        _POLAROID_CARD_W,
        _POLAROID_CARD_H,
        rotation_deg=0.0,
        z_index=z,
        prefer_portrait=(i == 0),
        polaroid=True,
    )
    for i, z in enumerate(_POLAROID_Z_ORDER)
)

_STRIP_POLAROID_TABLE_FILL_REQUIRED: tuple[bool, ...] = (True,) * 20 + (False,) * 4

TEMPLATE_STRIP_POLAROID_TABLE_V1 = StripTemplate(
    id="strip_polaroid_table_v1",
    canvas_width=10800,
    canvas_height=1350,
    slice_width=1080,
    slice_height=1350,
    slice_count=10,
    overlap_px=0,
    background="#5a3d26",
    slots=_STRIP_POLAROID_TABLE_SLOTS,
    slot_fill_required=_STRIP_POLAROID_TABLE_FILL_REQUIRED,
    layout_jitter_px=0,
    layout_cover_slot_index=None,
    procedural_background="wood_polaroid_table",
    layout_placer="organic_polaroid",
)

# Default = mural v2. Literal column strip: TEMPLATE_STRIP_10COL; legacy mural: TEMPLATE_STRIP_MURAL_V1.
DEFAULT_STRIP_TEMPLATE = TEMPLATE_STRIP_MURAL_V2

STRIP_SLOT_COUNT = DEFAULT_STRIP_TEMPLATE.num_slots
STRIP_SLICE_COUNT = DEFAULT_STRIP_TEMPLATE.slice_count

# GUI / CLI picker order (id, short label).
STRIP_TEMPLATE_OPTIONS: tuple[tuple[str, str], ...] = (
    (
        "strip_mural_v2",
        "Mural v2 - 35 slots + 10+5+9+18 bg underfill (base+boost+repeat+last-slice tail), hero 1st slice; no gap-fill on top (SPEC_STRIP_MURAL_V2.md)",
    ),
    ("strip_polaroid_table_v1", "Polaroid table - wood + 20 photos (+4 opt.)"),
    (
        "strip_seamless_mosaic_v1",
        "Seamless mosaic - V (+trim) + optional 2V + H/2H/3H + ... + 2H stack + H H (seeded)",
    ),
    ("strip_seamless_v1", "Seamless v1 - flat overlap (9)"),
    ("strip_mural_v1", "Mural v1 - legacy (8)"),
    ("strip_10col", "10 columns - 1 photo / slide (10)"),
)

STRIP_GUI_TEMPLATE_IDS: tuple[str, ...] = tuple(t[0] for t in STRIP_TEMPLATE_OPTIONS)


def get_default_template() -> StripTemplate:
    return DEFAULT_STRIP_TEMPLATE


def get_template_by_id(template_id: str) -> StripTemplate:
    if template_id == TEMPLATE_STRIP_10COL.id:
        return TEMPLATE_STRIP_10COL
    if template_id == TEMPLATE_STRIP_MURAL_V1.id:
        return TEMPLATE_STRIP_MURAL_V1
    if template_id == TEMPLATE_STRIP_MURAL_V2.id:
        return TEMPLATE_STRIP_MURAL_V2
    if template_id == TEMPLATE_STRIP_SEAMLESS_V1.id:
        return TEMPLATE_STRIP_SEAMLESS_V1
    if template_id == TEMPLATE_STRIP_SEAMLESS_MOSAIC_V1.id:
        return TEMPLATE_STRIP_SEAMLESS_MOSAIC_V1
    if template_id == TEMPLATE_STRIP_POLAROID_TABLE_V1.id:
        return TEMPLATE_STRIP_POLAROID_TABLE_V1
    raise KeyError(f"Unknown strip template: {template_id!r}")


def max_strip_slots() -> int:
    return max(
        mosaic_v1_max_slot_count()
        if tid == "strip_seamless_mosaic_v1"
        else strip_effective_num_slots(get_template_by_id(tid))
        for tid in STRIP_GUI_TEMPLATE_IDS
    )


