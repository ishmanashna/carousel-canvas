# Specification: `strip_mural_v2` (locked)

This document is the **approved product spec** for the default strip template **`strip_mural_v2`**.  
Implementation should match this; do **not** Ã¢â‚¬Å“improveÃ¢â‚¬Â it in code without updating this file.

---

## 1. First carousel slide (leftmost 1080Ãƒâ€”1350)

1. **Dominated by one photo** Ã¢â‚¬â€ a single clear focal (readable subject), vertically centered in the slide.
2. **Collage only behind** Ã¢â‚¬â€ other layers may show at the **edges** or **under** that focal, but must **not** compete as equal Ã¢â‚¬Å“mainÃ¢â‚¬Â content in column 1.

## 2. Hero slot (user-facing)

1. **Same slot index on every export** Ã¢â‚¬â€ the **flagship** slot (`layout_flagship_slot_index`) is the **only** strip slot intended for Ã¢â‚¬Å“swap this one photoÃ¢â‚¬Â in the app (hero-only replace mode targets this).
2. Geometry stays in **slice 1** (x range within the first **1080px** of the 10800px wide canvas), portrait-friendly, **moderate** tilt.

## 3. Photo count

1. Target **~35 distinct photos** used for the **image-required** slots (template has **35** such slots).
2. If the folder contains **at least** as many **unique** files as required slots, assignment must **not** repeat files until the pool is exhausted (no Ã¢â‚¬Å“10 copies while 80 files sit unusedÃ¢â‚¬Â).

## 4. Scale of tiles

1. **No single giant layer** that reads as Ã¢â‚¬Å“this one crop spans half the muralÃ¢â‚¬Â (no full-width **thin** ribbons; no **mega** rectangles).
2. **No tiny tiles** Ã¢â‚¬â€ no postage-stamp layers next to giants; keep width/height in a **medium band** (overlapping collage).
3. **Resize/cover** is OK inside each slot box; boxes themselves stay in range.

## 5. Tilt & energy

1. **Moderate** tilt (roughly in the **Ã‚Â±2Ã‚Â°** neighborhood), not dead straight, not wild scrapbook.

## 6. Background / empty area

1. Goal: **almost no empty** beige showing Ã¢â‚¬â€ overlap and **cover** so the mural feels **full**.

## 7. Beige handling (top gap-fill vs background underfill)

1. **Top gap-fill off** Ã¢â‚¬â€ **no** extra crops pasted **on top** of the finished mural (`gap_fill_max_layers = 0`).  
   User-facing: no Ã¢â‚¬Å“random patches over the collageÃ¢â‚¬Â for v2.

2. **Background underfill (10 + 5 + 5 photos)** Ã¢â‚¬â€ After the **35-slot** collage is composed, detect where **template beige** still dominates; place up to **10** base underfill images plus **5** boost underfill images plus **5** repeated large underfill images on the **rearmost** layer (under **all** slot photos), preferring files **not** already used in the 35 slots. Same beige-detection spirit as legacy gap-fill, opposite paint order.

## 8. Reference

1. **`carousel_20260322_185217_406_03`** Ã¢â‚¬â€ structural reference (user noted **low tilt** in that frame; overall layout/readability matters more than matching tilt exactly).

## 9. NonÃ¢â‚¬â€˜negotiables (fail if violated)

1. No **long random horizontal strip** of one image spanning **many** slices (e.g. ~6 columns of Ã¢â‚¬Å“same bandÃ¢â‚¬Â).
2. No **large dead voids** of background while photos could cover.
3. No **massive duplication** of the same file across slots when the folder has **many** distinct images available.

## 10. Technical anchors (implementation)

| Item | Rule |
|------|------|
| Template id | `strip_mural_v2` |
| Canvas | 10800Ãƒâ€”1350, 10 slices Ãƒâ€” 1080Ãƒâ€”1350 |
| Image slots | **35** required (`slot_fill_required` default all `True`) |
| Hero | Last flagship slot; **paints after** rims (higher z) so the portrait stays on top; **prefer_portrait** |
| Rims | Two small slots **under** the hero; anchored by jitter; visible mainly at rotated **transparent** corners |
| Jitter | `layout_jitter_px` moderate; hero/rim rules unchanged |
| Gap-fill (on top) | **`gap_fill_max_layers = 0`** |
| Background underfill | **`background_underfill_layers = 10`** + **`background_underfill_boost_layers = 5`** + **`background_underfill_repeat_layers = 5`** |
| Assignment | Unique sample when `unique_files Ã¢â€°Â¥ required_slots` |

---

*Approved from user Q&A (numbered answers 1Ã¢â‚¬â€œ10).*


