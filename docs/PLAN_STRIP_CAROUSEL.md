# Plan: Strip / Carousel / Moodboard Pipeline

## Goal

A separate pipeline that:
- **Input:** N images (any orientation) from a folder
- **Composition:** A wide horizontal collage built from those images (cut, rotated; **mural interior is always color** â€” see [PLAN_STRIP_MURAL.md](PLAN_STRIP_MURAL.md))
- **Output:** 10 vertical slices (1080Ã—1350 or similar) ready for Instagram carousel â€” swipe left/right gives a continuous visual

No full manual placement. User picks a template (or random) and can **swap** photos in slots (e.g. "use this photo instead of that one in slot 3").

---

## Architectural Principles

1. **Modular:** Lives in its own package (e.g. `app/strip/` or `app/carousel/`), not mixed into `layout_engine.py`.
2. **Shared utilities only:** Reuse color parsing, JPEG save, path handling â€” not layout config, grid geometry, or orientation filters.
3. **Template-driven:** Composer logic is separate from slice logic. One can swap composers (grid-like, moodboard-like, random) without touching the slicer.

---

## Pipeline Stages

```
[Input folder] â†’ [Template selector / random] â†’ [Composer] â†’ [Wide canvas] â†’ [Slicer] â†’ [10 vertical files]
                        â†“
                 [Slot assignment: which photo goes where]
                        â†“
                 [User can swap: "photo A â†” photo B in slot X"]
```

### 1. Composer

- **Input:** Ordered list of image paths, template spec (or template ID + seed for random)
- **Output:** Single `PIL.Image` (wide canvas, e.g. 10800Ã—1350 for 10 slices of 1080Ã—1350)
- **Template defines:**
  - Number of slots
  - Per-slot: position (x, y), size (w, h), rotation, crop style (cover/contain), flip_h, grayscale
  - Background color
- **Pre-made templates:** JSON or Python dataclass describing layout
- **Pseudo-random templates:** Generator that produces valid template specs from a seed (e.g. random positions, rotations, overlaps)

### 2. Slicer

- **Input:** Wide canvas image, slice width (1080), slice height (1350), count (10), optional overlap
- **Output:** List of 10 vertical images
- **Behavior:** Fixed-width horizontal slices. Optional overlap so seams land in gutters or safe areas.
- **Reusable:** Same slicer for any wide image â€” moodboard output, grid output, or externally created art.

### 3. Slot Assignment & Swap UX

- **Assignment:** Template says "slot 1 needs a photo". Folder scan yields M images. Auto-assign first N, or random.
- **Swap:** User selects photo A from library, clicks slot showing photo B â†’ swap. No drag-to-position.
- **Preview:** Show the wide canvas (scrolled or scaled) and/or the 10 verticals in a strip.

---

## Package Structure (Proposed)

```
app/
  strip/                    # New package
    __init__.py
    composer.py             # Abstract composer, template loader
    templates/              # Pre-made templates
      editorial_01.json
      moodboard_rand.py     # Random generator
    slicer.py               # wide_image â†’ [vertical_slice, ...]
    pipeline.py             # Orchestrator: folder â†’ composer â†’ slicer â†’ save
  engine/                   # Existing grid layouts
    layout_engine.py
```

---

## Refactoring Preconditions

Before implementing, consider:

1. **Extract shared I/O:** Move `save_optimized`, `parse_color`, `IMAGE_EXTENSIONS` to `app/common.py` or `app/utils.py` so strip pipeline doesn't depend on layout_engine.

2. **GUI entry point:** Strip mode could be a separate tab, window, or mode. Avoid overloading the current mode radio (single/batch/random/combo/manual).

3. **State persistence:** If we add strip projects (template + slot assignments), they need save/load. JSON schema similar to current settings, but for strip-specific data.

---

## Implementation Phases

### Phase 1: Slicer only (low risk)
- Implement `slicer.slice_wide_image(canvas, slice_w, slice_h, count)` â†’ list of images
- Unit test with a generated wide gradient
- No GUI yet

### Phase 2: Simple composer + slicer
- One fixed template: e.g. 5 horizontal strips, each with one photo (cover crop), no rotation
- Pipeline: folder â†’ pick first 5 images â†’ compose wide â†’ slice â†’ save 10
- CLI entry: `python script.py --strip --template simple ./photos`
- Validates the full chain

### Phase 3: Template format + swap UX
- JSON template format
- GUI: strip mode, thumbnail strip for slot assignment, swap on click
- Preview of wide canvas or first few slices

### Phase 4: Random / moodboard templates
- Generator for overlapping, rotated placements
- More "moodboard" feel while keeping slot-based assignment

---

## Open Questions

1. **Slice dimensions:** Instagram portrait 1080Ã—1350 (4:5)? Or 1080Ã—1920 (9:16) for Stories?

4:5

2. **Overlap:** Should adjacent slices overlap slightly so transitions feel continuous, or hard cuts?

Depends on template.

3. **Template count:** Start with 2â€“3 hand-made templates, or invest in a random generator first?

Random generator doesn't generate templates, it generates variations of the template. Start with 1 handmade, no randomization (well, only in image selection for each hole)

4. **Integration:** Same app, new tab? Or separate script (`run_strip.py`)?

same app, new mode.

---

## Implementation status (v1)

- **Intent vs literal strip:** See **`docs/STRIP_WHAT_YOU_WANT.md`** â€” default is a **mural** (fewer photo slots than carousel slides), not â€œ10 vertical photos in a row.â€
- **Package:** `app/strip/` â€” `template_data.py`, `slicer.py`, `composer.py`, `pipeline.py`, `fills.py`.
- **Templates:** **`strip_mural_v2`** (default): **10** slots â€” tilted mural with **cover** on foreground frames (revised geometry to reduce mid-strip gap and right-side top clustering). **`strip_seamless_v1`**: **9** slots â€” flat overlapping stack, tiny tilt, `#101010`. **`strip_mural_v1`**: 8 slots legacy. **`strip_10col`**: 10 equal columns. All export **10** carousel JPEGs at 1080Ã—1350 unless slice settings change.
- **Constants:** `STRIP_SLOT_COUNT` = `num_slots` on the default template (30 for `strip_mural_v2`). Fewer unique files than slots â†’ **cyclic repeat** when assigning. `STRIP_SLICE_COUNT` = JPEG outputs (10). **Overlap:** `overlap_px` needs a matching canvas width; default mural uses 0 overlap on 10800px.
- **GUI:** Mode **Strip (carousel mural)** â€” same thumb/stage interactions as Manual. **Shuffle** fills slots; **Smart shuffle** (default) maps landscape/wide and portrait/tall via `assign.pick_smart_fills`. Layout cards disabled. Borderless/Bleed disabled in this mode.
- **CLI:** `python -m app.cli --strip FOLDER` (â‰¥ template slots). `--strip-dumb-shuffle` = random assignment.
- **Tests:** `python -m unittest tests.test_strip -v` from project root.

