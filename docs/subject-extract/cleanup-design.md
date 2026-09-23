# Cleanup design

Lasso is in the kit (`sx lasso`, `sx lasso-preview`). The session prompt is [cleanup.md](cleanup.md). Launcher: `tools\subject-extract\launch-cleanup-nueva.ps1` (do not start until asked).

Hand process in Paint: zoom into the mess, **free selection**, draw a path that wraps the leftover and **hugs the keep-edge** (instrument, hair, hand), then delete. Transparency stays. Typical case: junk fused to a keeper — a rectangle wipe would eat the keep-edge; dropping “islands” would not see a separate blob.

Agents will not get a live lasso. Same intent, coarser tools.

Complete extra people are keepers, not leftovers. Cleanup is for scraps and background, not for removing a second person who is a full silhouette.

## What failed before

- **`wipe` box** — deletes a rectangle. Fine for a speck in empty space. Wrong when junk shares an edge with hair, hands, or an instrument.
- **`wipe-islands`** — automatic connected components. That is how we cut off held kit and usable people. Banned for a reason.
- **SAM to subtract** — useful only when the junk is a separate object the model can grab. A fused opaque slab will often take the keep-edge with it.

## What to build later

Paint’s zoom + free select + delete, in three commands we already almost have:

1. **`crop`** the affected patch (the zoom). Already exists.
2. **Look** at that crop (and a local numbered grid on it, if we add `grid` on the crop).
3. **`lasso`** — zero alpha **inside a polygon**. Vertices in crop pixels, then offset by the crop origin onto `cutout.png`. Same RGB, only alpha. `keep-best` promote/restore.

Optional **`lasso-preview`**: draw the polygon on the crop and write `lasso_preview.png` so the agent looks before deleting. That preview is the substitute for Paint’s marching ants.

Suggested CLI (not in the kit yet):

```powershell
& $sx crop --xyxy "x1,y1,x2,y2" cutout.png region_cut.png
& $sx lasso-preview --poly "x1,y1;x2,y2;x3,y3;..." --origin "x1,y1" region_cut.png lasso_preview.png
& $sx lasso --poly "x1,y1;x2,y2;x3,y3;..." --origin "x1,y1" cutout.png cutout.png
```

`--origin` is the crop’s top-left in work-res. Polygon is closed automatically. Need ≥3 points. Fill with `cv2.fillPoly`. No feather unless we later add a 1px inset.

Then `lift-alpha` again if the work cutout changed.

Triage `problem` and `leftover` on the photo’s latest `TRIAGE.jsonl` line are the cleanup brief. Do not invent extra targets.

## How an agent would run it

1. Read this photo’s latest `TRIAGE.jsonl` line (`problem`, `leftover`, `keep`). Checker on the whole cutout. Name each leftover.
2. Tight crop around leftover **plus** the keep-edge.
3. 6–20 polygon points: around the junk, along the keep-edge, not through the subject.
4. Preview → look. If the line crosses a keeper, rewrite points. Do not apply.
5. Apply once. Checker. Restore if it bit the subject.
6. Cap: **3 lassos**. If a keeper is half-missing, stop — that photo belongs in **redo**, not cleanup.

Detached floaters that do not touch a keeper can still use a **tight box wipe** as a shortcut. Fused junk is polygon-only.

## Honesty

Agents pick vertices from a picture; they will not trace a keep-edge like a mouse. The crop + preview loop is how we keep that from eating the subject. If preview is skipped, cleanup will regress to the old “delete a box and hope.”
