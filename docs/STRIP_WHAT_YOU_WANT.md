# What “strip” is supposed to be (vs what we had)

## What you **don’t** want

A **single ultra-wide rectangle** divided into **10 equal vertical columns**, each column filled with **one independent photo** (cover-cropped to 1080×1350), then exported as 10 files.

That is mathematically a horizontal panorama of **10 separate slides**. When you swipe the carousel, you are just moving along a row of unrelated vertical crops. There is **no single designed composition** — only alignment.

## What you **do** want

1. **One horizontal mural** — a deliberate layout: layers, overlaps, rotations, different sizes, maybe a full-width “base” that reads as **one continuous band** (sky, ground, texture) under or behind smaller pieces.

2. **Continuity across Instagram’s cuts** — the 10 outputs are still **10× 1080×1350** (platform constraint), but the **artwork** is built so that:
   - visual mass **crosses** slice boundaries (one card straddles two slides, a horizon line continues, color blocks align),
   - optional **overlap between exported slices** so adjacent slides share pixels and the swipe feels less like “next unrelated frame”.

3. **Fewer photos than slides is OK** — you might use **8 images** (or 5) in a rich layout, while the **carousel still has 10 frames** slicing that same mural. The slides are a **viewport** moving across one piece, not “one photo per slide.”

## Terms

| Term | Meaning |
|------|--------|
| **Image slots** | How many photos the template expects you to assign (shuffle / drag / etc.). |
| **Carousel slices** | Always **10** vertical exports (for this project), 1080×1350, left → right through the mural. |
| **Template** | Defines canvas size, each layer’s box, rotation, z-order, background, and slice overlap. |

The old `strip_10col` template is kept as **`strip_10col`** (10 slots = 10 columns) for people who want that literal strip. The **default** is **`strip_mural_v2`** (**10** photos: base band + 9 foreground cards, mixed wide + portrait; foreground slots use **`cover`** so photos bleed to the frame edges without big beige letterboxing). **`strip_seamless_v1`** is a **9**-photo, low-tilt “seamless” stack on a dark field. **`strip_mural_v1`** is the legacy **8**-photo layout. Pick templates in the GUI or via CLI `--strip-template`.

**Slice overlap:** If `overlap_px > 0`, the canvas width must satisfy  
`canvas_width ≥ slice_count × slice_width − (slice_count − 1) × overlap_px`  
so every pixel of the composition is covered by at least one exported slide (otherwise the right side can be “orphaned”). With a 10800px-wide canvas and 10×1080px slices, overlap must be **0** unless you widen the canvas.
