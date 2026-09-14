# Out of frame — design

A new strip canvas. Not mural v2. Not polaroid scatter. Papers (full photos) first, figures (whole people with the background gone) seated only in empty space, coming out of the paper. Slide cuts must not bisect a person.

Canvas stays 10800×1350, ten 1080×1350 Instagram slices.

## What it looks like

Large photographic prints overlap a little, like photos on a desk. Edges of those prints stay sharp. A person from another photo is lifted off their background and tucked into empty sky, wall, floor, or margin of a print, then allowed to break the print’s edge. The chopped bottom of the cutout is hidden in the paper. You do not see a person glued onto another person’s face. You do not see a head split by a slide line.

Slide 1 is the hook: one clear paper, one figure stepping out of it.

## Two roles

**Paper.** Keeps its background. Opaque rectangle, slight tilt, drop shadow. Occupies about one to two slices. Neighbors overlap so beige does not run in valleys between prints. Overlap is paper-on-paper, not a fade or dissolve.

**Figure.** Background removed. Only used if the person (or single clear subject) can be shown whole. Dest size follows the subject, not the original photo frame. Sits on top of papers. Drop shadow from the silhouette.

A photo is never both in the same layout. Leftover files stay unused rather than being forced into a bad role.

## Subject vs empty space (one local pass)

There is no mural slot board underneath this. There is also no separate “find people” network plus a “cut background” network.

One salient-foreground model — rembg’s `isnet-general-use.onnx` (DIS dichotomous segmentation) — runs locally through ONNX Runtime (`ort`, DirectML on Windows, CPU if DirectML is missing). It produces a soft mask per photo, cached on disk by file content hash. That mask is used three ways:

1. **Cutout.** Figure pixels = photo × mask, after a cheap color-decontamination pass on the fringe so hair does not keep a halo.
2. **Occupancy.** On a paper, high mask = someone/something already in the photo. Low mask = empty. Figures may only cover empty. Dilate occupancy slightly so we do not nick faces.
3. **Wholeness.** The largest mask blob decides the role:
   - Touches the **top** of the source file → head is chopped → not a figure.
   - Touches **left and right** → not a figure.
   - Touches **bottom only** → feet/crop cut is allowed; that cut must be hidden in the paper.
   - Two or more large blobs, or a very wide blob → group / busy scene → paper, not figure.
   - Tiny blob in a large scene → paper; the blob is occupancy to avoid covering.
   - One blob, moderate coverage, head intact → figure candidate.

Shuffle after the first analysis does **not** rerun the model. It only reads cached masks and places.

No Ultralytics YOLO (AGPL). No Python rembg as a program. No paid cloud cutout. No extra app to install. First run downloads the ONNX file into `%LOCALAPPDATA%/CarouselCanvas/` (~176 MB), never into the git repo. DIS’s Apache-2.0 text covers their code; the weight file’s commercial terms are not a clean grant — do not treat that as settled. After the file is local, the machine can work offline.

This will be wrong sometimes (glass, a person the same color as a wall, a crowd). Those photos stay paper, or the user swaps them in Manual. Auto must refuse people-on-people and chopped-through-the-middle rather than guess harder.

## Placement

1. Pick papers (enough to cover the strip with overlap) and figures (as many as empty pockets can take).
2. Lay papers left to right, first paper dominating slice 1, each overlapping the next, z rising toward the viewer on later papers so a figure on slice 1 can still sit in front.
3. Map each paper’s occupancy onto the canvas (cover fit).
4. For each figure, search scales and positions. Keep occupancy under the body low. Hide the chopped feet in **empty** paper, not on a face. Keep the face/torso off the ten vertical slide lines. Prefer hair/arm/shoulder breaking the paper edge. Score the poses and take the best; if none work, drop that figure. After it lands, that person counts as occupied so the next figure cannot sit on them.
5. Lock. Manual move/swap/layer does not re-analyse or re-pack papers unless unlock. Shuffle on the same folder does not run the model again.

A person who would only show as a fragment does not count as a figure. Better fewer figures than a half body.

## Papers do not blend

Sharp edges. Small shadow. Overlap instead of dissolve. A fade would turn this into a single montage and you would lose “coming out of the photo.” Tiny leftovers of canvas color at the extreme top/bottom are acceptable. No beige underfill planner.

## Time

| Moment | What happens | Expect |
| --- | --- | --- |
| First time this PC sees the model | Download ONNX (~176 MB) | Once. First build also fetches ONNX Runtime binaries. First GPU session may spend extra seconds compiling. |
| First time a folder is opened in this mode | Mask every photo, write cache | On GPU, about 30–90 s for ~40 photos. CPU-only can be 1–4 min; ~80 photos can take several minutes. Progress line, off the UI thread. |
| Shuffle / new layout on the same folder | Layout only | Same as today’s shuffle. The model does not run again. |
| Later sessions, same files | Read cache | Seconds, not a minute. |
| Export | Same 10-slice GPU path, now with real alpha on figures | In the same ballpark as current export. |

Analysis never runs on the UI thread.

## Manual

Same lock idea as now. After Auto, papers and figures freeze. Replace a figure, drag it into a different empty pocket, change z. Unlock to shuffle again.

## Out of scope

Mural jitter, polaroid scatter, underfill, fading papers into each other, a second detection model, cloud APIs, Python, changing slice count or canvas size, stickers/tape/type.
