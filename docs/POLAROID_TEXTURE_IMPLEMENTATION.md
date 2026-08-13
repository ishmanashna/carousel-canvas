# Polaroid materiality: implementation guide (handoff)

This document is for an agent implementing **richer polaroid “object” look**—border materiality, optional gloss over the **whole card** (border + photo), and coherent lighting—**without** changing the core **photo processing** pipeline (`process_image_contained_panned` / `process_image_panned` parameters or `_polaroid_photo_finish` behavior), unless the product owner explicitly expands scope.

---

## 1. Goals and non-goals

### Goals

- Polaroids read as **physical sheets** (paper / laminate) on wood, not flat sprites.
- Effects that apply to **border only** or to **entire card alpha** (border + window together) are in scope.
- **Deterministic** per slot: derive sub-seeds from existing `layout_seed` + slot index (same pattern as today: `(layout_seed * 1009 + i * 9176) & 0xFFFFFFFF`).
- Keep export time acceptable; prefer work at **supersampled** resolution inside `polaroid_card` where quality matters, or **once per final rotated card** in `composer` for full-card overlays.

### Non-goals (unless PM overrides)

- No new user-facing GUI controls required for v1 (constants or a single internal enum is fine).
- Do **not** re-tune smart assign, slot counts, or `polaroid_scatter` layout in this task.
- Avoid changing **inner photo** “develop” look: `_polaroid_photo_finish` is explicitly **out of scope** for this handoff unless implementing an optional **overlay on top of the window** (see §5.4)—that is a layer, not a change to source photo processing.

---

## 2. Repository context

### 2.1 Template

- **ID:** `strip_polaroid_table_v1` (`app/strip/template_data.py`)
- **Procedural background:** `wood_polaroid_table` → `render_procedural_background` in `app/strip/procedural_backgrounds.py`
- **Placer:** `layout_placer="organic_polaroid"` → `resolve_organic_polaroid_slots` in `app/strip/polaroid_scatter.py`
- **Borderless export:** CLI `--strip-card-edge borderless` is the main visual mode for polaroids on wood.

### 2.2 Polaroid card build (supersampled)

**File:** `app/strip/polaroid_card.py`

| Piece | Role |
|--------|------|
| `polaroid_margins` | Side / top / bottom chin geometry |
| `polaroid_supersample_factor` | 1 / 2 / 3× by slot width and `compose_scale` |
| `build_polaroid_rgba_supersampled` | Full card RGBA at `slot_w*ss × slot_h*ss`: paper, inner composite, inset shadow, outer bevel, rounded alpha |
| `_paper_rgb` | Warm base + subtle noise (current “paper”) |
| `_inner_soft_mask` | Soft photo→paper transition in window |
| `_outer_bevel` | Thin highlight/shadow on outer frame |
| `scale_down_rotated_polaroid` | After rotate, LANCZOS downscale by `ss` |

**Caller:** `compose_strip_wide` in `app/strip/composer.py` loads inner image at supersampled inner size, builds card, rotates with `fillcolor=(*bg, 0)` (`bg = parse_color(template.background)`), scales down, then composites onto RGBA canvas.

### 2.3 Per-card effects on canvas (post scale-down)

**File:** `app/strip/composer.py` (inside `paint_borderless_slot`, polaroid branch)

Current order (verify in file when editing):

1. `_reflection_from_card` — table reflection below card (uses flipped card + gradient mask)
2. `_contact_shadow_from_alpha` — shadow from card alpha, offset + blur
3. `target.paste(img, …)` — card on top

**Important:** Any **full-card gloss** (§4.2) should be applied to **`img` after rotate+downscale** and **before** paste, **or** as a separate RGBA layer pasted with the same `(px, py)` and the card’s alpha as mask—so highlights stay registered with the card.

### 2.4 Flatten and JPEG

- Full canvas: `flatten_rgba_over_rgb` in `polaroid_card.py` (used from `composer` for borderless strip output).
- Saves: `save_optimized` in `app/engine/layout_engine.py` (JPEG quality / subsampling already tuned).

### 2.5 Tests and smoke

- `python -m unittest tests.test_strip -v`
- Visual:  
  `python -m app.cli "TEST IMAGES" --strip --strip-template strip_polaroid_table_v1 --output output/strip_latest --strip-card-edge borderless --strip-layout-seed 42`  
  Mention written `carousel_*_wide.jpg` in the summary (workspace rule).

---

## 3. Design principles (coherence)

1. **Single light direction** — e.g. implied key light from **top-left**: bevel highlights, gloss streaks, and optional border warm/cool bias should agree.
2. **One material story** — instant-film: **matte paper border** + optional **thin glossy laminate** over the **entire** visible card (masked by final alpha).
3. **Subtlety** — carousel swipes stay clean; prefer many weak layers over one strong filter.
4. **Supersample-first** — border imperfections and laminate edges benefit from building at `ss` then downscaling with the existing rotate/downscale path.

---

## 4. Implementation phases (recommended order)

### Phase A — Border-only materiality (`polaroid_card.py`)

**A.1 Non-uniform border tone**

- Today `_paper_rgb` is fairly uniform. Extend with **low-frequency variation**:
  - Large-scale gradient (e.g. slightly warmer toward chin / bottom-right), **masked to border region only** (full card minus inner window rectangle).
  - Optional second pass: very soft vignette on **border only** (invert inner window mask, feather 2–4 px at supersample).

**A.2 Outer perimeter aging**

- Thin **warm/yellow** band along **outer** edge only (draw on border mask, blur 1–2 px at ss).
- Optional **corner crush**: seeded choice of one corner; localized darken + 1 px “compression” (alpha or color).

**A.3 Imperfect outer silhouette**

- After rounded rect mask, apply **subtle edge displacement** or **low-amplitude noise** on the **alpha contour** only (erode/dilate blend or filtered noise on alpha band)—avoid destroying anti-aliased tilt quality.
- Keep compatible with existing **rotate + `fillcolor=(*bg,0)`** wedge mitigation.

**Acceptance:** Border alone looks less “vector white”; inner photo pipeline unchanged.

---

### Phase B — Whole-card gloss / laminate (`polaroid_card.py` preferred)

**B.1 Specular sheet over entire card**

- Build an **RGBA overlay** same size as the card **at supersample**, contents:
  - One or two **soft diagonal highlights** (gradient or blurred white ellipse), **very low** max alpha (e.g. 4–12% at ss).
  - **Clip to card alpha** (`ImageChops.multiply` overlay alpha with card alpha) so gloss stops at the physical edge.
- `alpha_composite` onto the card **after** paper + photo + inset/bevel (order: decide whether gloss sits “on top of” bevel—usually **last** before final rounded mask, or **after** rounded mask so corners stay clean).

**B.2 Environment-tinted reflection (optional)**

- Sample or approximate **table color** (e.g. mean of procedural wood or template `background` hex blurred)—not full raytracing.
- Add **very blurred** vertical gradient in gloss layer (cool/warm) so the card picks up a hint of the scene.
- Strength: lower than white specular.

**Acceptance:** At zoom, border and **photo window** show the **same** highlight family; no obvious “frame-only” shine.

---

### Phase C — Composer-level whole-object cues (`composer.py`)

**C.1 Table reflection upgrade (optional)**

- Current `_reflection_from_card` only uses the card. Consider:
  - Slightly **stronger** or **taller** reflection for hero slot only (slot index 0), or
  - **Blur** reflection more to read as glossy wood, not a second copy of the image.

**C.2 Shared ambient occlusion (advanced)**

- Very soft darkening on **canvas** under card bbox (wood only), masked so it doesn’t darken the card—tighter than contact shadow. Risk: busy with 24 cards; keep **very** low contrast.

**Acceptance:** Whole object feels grounded; no double-shadow clutter.

---

### Phase D — Optional “matte window” overlay (product decision)

**D.1 Emulsion haze on window only**

- After inner photo is composited, add a **semi-transparent neutral** layer **only** in the inner window (same soft mask as inner edge), e.g. 2–5% white or warm gray—**not** a change to `_polaroid_photo_finish` if PM wants “no photo pipeline change”: implement as explicit **overlay layer** in `build_polaroid_rgba_supersampled`.

**Acceptance:** Slight “print under matte” read; faces not blown out.

---

## 5. Technical notes and pitfalls

1. **Alpha order** — Rounded outer mask, bevel, and gloss must agree on **compositing order**. Document the final order in code comments once settled.
2. **Rotation fringe** — Do not reintroduce `(0,0,0,0)` rotate fill for polaroids; keep `fillcolor=(*bg, 0)` from `composer`.
3. **Performance** — Avoid per-pixel Python loops on 10800×1350; use `ImageChops`, `ImageFilter`, `Image.eval`, tiled noise + resize.
4. **Preview scale** — `compose_scale < 0.92` uses lower `ss`; test both full export and `compose_scale=0.5` path in `compose_strip_wide`.
5. **RGB wedge path** — Polaroid with `card_edge` not `borderless` uses flatten-to-white then rotate; any new full-card effect must have a path or be skipped consistently (grep `build_polaroid_rgba_supersampled` in `composer.py`).

---

## 6. Suggested API shape (for implementer)

Keep surface small:

- `build_polaroid_rgba_supersampled(..., slot_seed: int, ...)` — add optional `material_profile: str = "default"` or `enable_gloss: bool` if needed, **or** keep everything internal with module-level constants until a second template needs variants.
- Private helpers in `polaroid_card.py`: e.g. `_border_only_mask`, `_apply_border_aging`, `_apply_card_gloss`.

---

## 7. Definition of done

- [ ] Border shows **non-uniform** tone and/or outer aging without banding at JPEG export.
- [ ] Optional **full-card gloss** reads as **one laminate** over border + window (masked by card alpha).
- [ ] `tests.test_strip` passes; polaroid CLI export produces a reviewed `*_wide.jpg`.
- [ ] No regression in tilt fringe (compare PNG export if needed).
- [ ] All randomness **seeded** from `slot_seed` / `layout_seed` + slot index.

---

## 8. Reference: key files

| File | Responsibility |
|------|----------------|
| `app/strip/polaroid_card.py` | Paper, inner mask, bevel, build pipeline, flatten helper |
| `app/strip/composer.py` | Slot loop, rotate fillcolor, reflection, contact shadow, paste order |
| `app/strip/template_data.py` | `strip_polaroid_table_v1` definition |
| `app/strip/procedural_backgrounds.py` | Wood background for table |
| `app/strip/polaroid_scatter.py` | Layout only (out of scope unless bugfix) |
| `tests/test_strip.py` | Regression / export smoke |

---

*End of handoff document.*
