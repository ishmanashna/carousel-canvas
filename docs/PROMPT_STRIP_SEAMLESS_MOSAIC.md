# Agent prompt: Instagram strip template — seamless geometric mosaic (implement from scratch)

Use this document as the **full product spec** for a new carousel strip template. The previous attempt (`strip_seamless_geometric_v1` + `seamless_mosaic`) was removed; implement cleanly against the real codebase.

## Context (read first)

- **Repo:** Python app with `app/strip/` — `StripTemplate` / `StripSlotDef` in `app/strip/template_data.py`, composition in `app/strip/composer.py`, export in `app/strip/pipeline.py`, smart photo→slot assignment in `app/strip/assign.py`.
- **Output:** One wide JPEG `10800×1350`, then **10** vertical slices `1080×1350` (carousel). Same pipeline as existing templates.
- **Reference patterns:**
  - **Static geometry:** `strip_seamless_v1`, `strip_10col`.
  - **Seeded dynamic geometry:** `layout_placer="organic_polaroid"` + `resolve_organic_polaroid_slots` in `app/strip/polaroid_scatter.py`, wired in `resolve_strip_slots_for_preview` in `composer.py` **before** the branch that returns raw slots when `layout_jitter_px <= 0`.

## Goal (what the user wants)

A **seamless Instagram-style** wide collage: photos feel continuous when swiping slides, **not** a mural-v2 pile of tilted cards.

### Hard constraints

1. **Exactly two compositing layers (z_index semantics):**
   - **Layer 0 (background):** Non-overlapping rectangles that **partition** the full canvas `(0,0)–(10800,1350)` — no gaps, no overlaps. Every pixel covered by exactly one background slot.
   - **Layer 1 (foreground):** Additional rectangles **on top**, centered on or near **carousel junctures** (boundaries between 1080px-wide columns: `x = 1080, 2160, …, 9720`).

2. **Axis-aligned only:** `rotation_deg = 0` for all slots. `fit="cover"` is fine.

3. **Background must NOT look like:**
   - Four (or few) **full-height vertical stripes** spanning the whole strip (user rejected “4 columns of 2700×1350”).
   - A few **global horizontal bands** that span the **entire 10800px width**, then only subdivided vertically (reads as “three rows of tiles” — user rejected this). This happens easily if you use a **guillotine** on a wide rectangle with a **strong bias toward horizontal cuts** first.

4. **Background SHOULD evoke** a **smart random** mix of cell types, similar in spirit to a sequence like **`V + 2H + H + V + V + H + H + V`** (illustrative, not literal string parsing):
   - **V:** A **tall** cell (portrait-leaning aspect in that region) — can be a full column stack segment, not necessarily full 1350 if the layout has structure.
   - **H:** A **wide** cell (landscape-leaning).
   - **2H:** **Two** landscape cells **side by side**, **same vertical band** (aligned tops/bottoms), behaving as one **row pair** in that locality — not “thin strips”, still **real photo slots**.

   The point is **variety** and **no obvious single global grid** (neither “only columns” nor “only full-width rows”).

5. **Juncture overlays (layer 1):**
   - **Moderate** size (user: not tiny, not huge — think roughly on the order of **~900–1200px wide**, **~600–900px tall**, tune as needed).
   - **Placement:** Seeded randomness; sometimes near **top** or **bottom** of the strip, sometimes **floating** mid-height. Several should **straddle** juncture lines so carousel slides feel connected.

6. **Seeding:** Same **layout seed** must yield the **same** geometry. Changing seed changes layout. Wire **`--strip-layout-seed`** (and GUI `strip_layout_seed`) consistently for **both**:
   - slot resolution in `compose_strip_wide` / `resolve_strip_slots_for_preview`, and  
   - **smart assignment** in `pick_smart_fills` (orientation vs slot aspect), which requires either:
     - passing **resolved** `StripSlotDef` geometry into assignment when placeholders are meaningless, **or**
     - resolving inside `pick_strip_fills` before `pick_smart_fills` for this template only.

### Pitfalls (do not repeat)

- Defining pretty placeholders in `template_data.py` but **forgetting** `layout_placer` + composer branch → export uses **static** slots forever (user hit this).
- `resolve_strip_slots_for_preview`: if you only add a placer but leave **`layout_jitter_px == 0`** handled by an early `return template.slots` **before** your placer, mosaic **never runs**. Placer must run **before** that early return (see polaroid branch order).
- Guillotine + careless split bias → **aligned horizontal rows across full width** or **skinny vertical columns** — both were rejected.
- Column-only stacks without design → can still feel monotonous; aim for **mixed aspect** and **local** 2H pairs where it makes sense.

## Deliverables

1. New template id (suggest `strip_seamless_mosaic_v1` or user-approved name) in `template_data.py`:
   - `STRIP_TEMPLATE_OPTIONS` label,
   - `get_template_by_id`,
   - `StripTemplate` with `layout_placer` literal extended if needed.
2. Resolver module (e.g. `app/strip/seamless_mosaic_v2.py` or similar) implementing `resolve_*_slots(template, seed) -> tuple[StripSlotDef, ...]`.
3. `composer.py` — call resolver in `resolve_strip_slots_for_preview` for this placer.
4. `pipeline.py` / `assign.py` — ensure smart fill uses **resolved** geometry for this template + same `layout_seed` as export.
5. `app/cli.py` — add `--strip-template` choice; pass `layout_seed` into fill picking if your design requires it.
6. `tests/test_strip.py` — tests: slot count, placer id, deterministic seed, different seed ≠ geometry, export smoke still passes for all `STRIP_GUI_TEMPLATE_IDS`.
7. Optional: short note in `readme.txt` if the project documents strip templates there.

## Acceptance checks (manual)

- Export with at least **12+** distinct photos; toggle **layout seed** — layout changes, no crash.
- Visually: **no** obvious “only 4 vertical pillars” or “only 3 horizontal shelves across the entire strip” unless seed lottery is extremely unlucky (then adjust algorithm).
- Juncture overlays clearly overlap **some** slice boundaries.

## Suggested technical directions (agent’s choice)

Any algorithm that satisfies the partition + variety constraints is fine, for example:

- **Shelves with variable row height** and **variable column spans** (spanning cells break global row alignment).
- **Explicit packed layout** with randomized grammar generating rectangles (validate disjoint + cover).
- **Column stacks with deliberate cross-column height variation** only if you can still avoid “one obvious global row grid”.

Prove coverage: sum of background rectangle areas = `10800 * 1350`, pairwise intersections empty.

---

**End of prompt.** Implement against the current tree; do not restore deleted files verbatim without meeting this spec.
