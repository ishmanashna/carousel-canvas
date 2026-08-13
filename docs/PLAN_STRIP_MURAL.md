# Implementation plan: strip mural (`strip_mural_v1` and siblings)

## Target look (reminder)

One continuous horizontal collage: full-width **background band** + **overlapping tilted foreground** frames, read as **one piece** when sliced into 10× 1080×1350 carousel exports. See conversation + `docs/STRIP_WHAT_YOU_WANT.md`.

---

## Done / locked for now

| Item | Notes |
|------|--------|
| **No B&W in mural** | `compose_strip_wide` always passes `grayscale=False`. Slot B&W toggles in the GUI do not affect strip export/preview (flip/pan still apply). Revisit if you add a “mural style” preset later. |
| **Mixed-aspect mural (Phase 1)** | Default **`strip_mural_v2`**: **10** large photo slots on solid `#ece8e3` (calmer than the legacy 40-slot mural); paths **repeat** if the folder has fewer than 10 images. Hero last + 2 rim overlaps; gap-fill on beige. **`strip_mural_v1`** (8 slots) via CLI `--strip-template`. |
| **Per-slot fit (Phase 2)** | `StripSlotDef.fit`: **`cover`** vs **`contain`**. Composer branches on `fit`; pan uses \([-1,1]\) in both modes. **`strip_mural_v2` default (Phase 6)** uses **all `cover`** on foreground slots so wide frames do not show big `#ece8e3` letterbox “holes”. |

---

## Phase 6 — Layout + preview polish (post–real-export review) — shipped

**Validated on a real `carousel_*_wide.jpg` export:** letterboxed wide slots, a tall empty band between the tilted row and the base image, a visually “lonely” left card, and right-side slots sharing nearly the same small **y** (top-heavy).

| Symptom | Cause (confirmed) | Change |
|--------|---------------------|--------|
| Flat empty boxes on beige | `contain` on the widest slots + `#ece8e3` fill | **Mural v2:** foreground slots **all `cover`** (crop / bleed). |
| “Dead band” between stack and base | Base at **y ≈ 488** while most card tops sit **y ≈ 18–140** | **Raise base** (smaller **y**, taller **h**) so the textured band meets the collage sooner (`y=392`, `h=958`). |
| First card feels isolated | Slot 1 far left + `contain` shrinking the visible photo | **Slot 1** moved inward (`x=40`), **slot 2** shifted left (`x=1710`) for **more overlap**; cover removes extra matte. |
| Right side hugging the top | Template math: slots 7–9 had **y ≈ 28–58** | **Stagger** mid/right **y** (e.g. 108 / 92 / 128) while staying above the base band. |
| Slow / heavy strip preview | Full **10800×1350** compose then downscale | **GUI:** `compose_strip_wide(..., compose_scale=…)` capped by **`STRIP_PREVIEW_MAX_COMPOSE_WIDTH`** (default 4000px wide), **cache** keyed by template + file mtimes + pan/flip, **strip pan** uses the same **debounced** repaint as manual (no `immediate=True` per mousemove). **Export** unchanged (full resolution). |

**Suggested order (original plan) — status:** (1) template + contain policy **done** in this phase; (2) preview half-res + cache + pan coalesce **done** for GUI; (3) composer seam tweaks **only if** wedges remain after export review.

---

## Phase 1 — Mixed shapes (template only, still cover-crop) — shipped as v2

**Goal:** Landscape-friendly slots (wider rects) and portrait slots in the same template; no new engine features except possibly documenting slot aspect ratios.

**Tasks:**

1. ~~Revise slots~~ Done as `_STRIP_MURAL_V2_SLOTS` + `TEMPLATE_STRIP_MURAL_V2` (default). v1 slots unchanged for `--strip-template strip_mural_v1`.
2. Keep z-order and rotations coherent (background `z=0`, foreground increasing).
3. Re-check **slice boundaries**: prefer overlaps that **straddle** 1080px cuts for continuity.
4. Update `STRIP_SLOT_COUNT` if slot count changes (or keep 8 and only resize positions).

**Acceptance:** Wide export clearly shows **different aspect holes**; horizontal sources look less “crushed” in wide slots.

---

## Phase 2 — Per-slot fit mode (`cover` vs `contain`) — shipped

**Goal:** Some slots **letterbox/pillarbox** (full image visible) on template background; others stay **cover** (full bleed).

**Done:** `StripSlotDef.fit`, `layout_engine.process_image_contained_panned`, `composer.compose_strip_wide` branch; pan uses same \([-1,1]\) convention within margins. Other templates may still use `contain` where specified; **mural v2** foreground uses **`cover` only** after Phase 6.

---

## Phase 3 — Smart assign — shipped

**Goal:** Shuffle / auto-fill prefers **landscape** sources for **wide** slots and **portrait** for **tall** slots (EXIF-aware dimensions via `get_image_aspect_hint`).

**Done:** `app/strip/assign.py` (`pick_smart_fills`), `pipeline.pick_strip_fills(smart=…)`. GUI: checkbox **Smart shuffle** (default on), persisted in settings. CLI: `--strip-dumb-shuffle` for pure random. Manual drag assignment unchanged.

---

## Phase 4 — Third template: `strip_seamless_v1` (photos only) — shipped

**Goal:** Reference `maxresdefault`-style: **no** polaroid chrome — layered photos only, mild overlap, **~±0.5°** tilt, dark field `#101010`, **9** slots + **10** carousel slices.

**Done:** `TEMPLATE_STRIP_SEAMLESS_V1` in `template_data.py`; registered in CLI/GUI picker (`STRIP_TEMPLATE_OPTIONS`).

---

## Phase 5 — Strip preview UX — shipped

**Goal:** Stage no longer squashes the ~8:1 mural into a postage stamp.

**Done:** Preview **scales to ~92% of stage height** (full vertical use); canvas **horizontal scroll** when the scaled mural is wider than the stage; **Shift + mouse wheel** scrolls horizontally. Hit-testing uses `canvasx`/`canvasy` + `_strip_view` (scale and paste offset). Manual / auto modes reset `scrollregion` to the viewport.

---

## Files likely touched (by phase)

| Phase | Files |
|-------|--------|
| 1 | `app/strip/template_data.py`, tests if slot count changes |
| 2 | `app/strip/template_data.py`, `app/strip/composer.py`, maybe `layout_engine` small helper or local `_contain_fit` |
| 3 | `app/strip/assign.py` (new), `main_window.py` shuffle |
| 4 | `template_data.py`, `get_template_by_id`, CLI/GUI template picker if not already |
| 5 | `main_window.py` stage metrics / strip preview build |
| 6 | `template_data.py` (mural v2 geometry + fit), `composer.py` (`compose_scale`), `main_window.py` (preview cache + pan debounce) |

---

## References

- `docs/STRIP_WHAT_YOU_WANT.md` — intent vs literal strip  
- `docs/PLAN_STRIP_CAROUSEL.md` — pipeline + templates overview  
- Reference assets: YouTube seamless collage + `carousel_*_wide.jpg` exports  
