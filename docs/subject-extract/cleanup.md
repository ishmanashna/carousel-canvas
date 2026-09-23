# Cleanup — lasso the leftover

One photo. Delete extra pixels. Do not rebuild missing subject.

The runner copies `cutout.png` to `pre_cleanup.png` in the same folder before you start. That file is the undo. Do not edit it, move it, or delete it.

## Brief

The prompt includes this photo’s `problem`, `keep`, and `leftover`. Delete only what `leftover` names. Do not invent extra targets. A complete extra person is a keeper, not leftover. If `leftover` says there is nothing to delete, write `REPORT.md` and stop.

## Commands

From repo root (PowerShell `;` not `&&`). Quote every `--xyxy`, `--poly`, and `--origin`.

```powershell
$sx = "tools\subject-extract\sx.ps1"
& $sx keep-best promote "{{OUT_DIR}}"
& $sx crop --xyxy "x1,y1,x2,y2" "{{OUT_DIR}}\cutout.png" "{{OUT_DIR}}\region_cut.png"
& $sx lasso-preview --poly "x1,y1;x2,y2;x3,y3" --origin "x1,y1" "{{OUT_DIR}}\region_cut.png" "{{OUT_DIR}}\lasso_preview.png"
& $sx lasso --poly "x1,y1;x2,y2;x3,y3" --origin "x1,y1" "{{OUT_DIR}}\cutout.png" "{{OUT_DIR}}\cutout.png"
& $sx checker "{{OUT_DIR}}\cutout.png" "{{OUT_DIR}}\cutout_checker.png"
& $sx keep-best restore "{{OUT_DIR}}"
& $sx lift-alpha --image "{{INPUT}}" --meta "{{OUT_DIR}}\work_meta.json" "{{OUT_DIR}}\cutout.png" "{{OUT_DIR}}\cutout_full.png"
& $sx checker "{{OUT_DIR}}\cutout_full.png" "{{OUT_DIR}}\cutout_full_checker.png"
```

`--origin` is the crop’s top-left on `cutout.png`. Polygon points are crop pixels. At least 3 points. The polygon closes itself.

`lasso` zeros alpha inside the polygon. RGB stays. `lasso-preview` only draws; it does not change the cutout.

If checkerboard fully separates an island from the keeper, one tight `wipe` box around that island is enough. The box must not contain any keeper pixel. Stop the box short of the person rather than shaving them.

Junk that shares an edge with hair, a hand, or an instrument is polygon-only. `lasso-preview` first. If the red fill covers the keeper, rewrite the points. Do not apply.

## Loop

1. Read the brief. Open `cutout_full_checker.png` if present, else `cutout_checker.png`.
2. `keep-best promote` before the first delete.
3. Name each leftover. Tight `crop` around that leftover plus the keep-edge.
4. 6–20 points around the junk, along the keep-edge, not through the subject. `lasso-preview`. Look at `lasso_preview.png`. If the line crosses a keeper, rewrite the points. Do not apply.
5. `lasso` once. Checker. If it bit the subject, `keep-best restore` and stop that shape.
6. At most **3** lassos. Then `lift-alpha` and checker `cutout_full.png` when `cutout.png` changed.

## Bans

No rembg, no SAM, no `wipe-islands`, no third cutout. Do not paint missing heads, necks, or headstocks back in. No GUI, no strip, no commit.

## Done

Write `REPORT.md` even when you deleted nothing.

```markdown
# Cleanup

- deleted: what you removed
- left: what you refused to cut
- snapshot: pre_cleanup.png untouched
```
