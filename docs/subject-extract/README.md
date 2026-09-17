# Subject extract

Local cutout kit: one photo in, PNG of the people (and what they hold) out.

Entry: `tools\subject-extract\sx.ps1`. Process: [03.1.md](03.1.md).

```powershell
$sx = "tools\subject-extract\sx.ps1"
& $sx doctor
```

First-time weights: `python tools/subject-extract/prefetch.py`.

## One folder overnight

One photo at a time (4 GB GPU). Skips images that already have `cutout_full.png`. A failed photo does not stop the folder.

```powershell
powershell -File tools\subject-extract\run-folder.ps1 -InputDir "TEST IMAGES" -OutputDir output\subject_extract\run
```

Each image gets one Cursor `agent` CLI job (`composer-2.5`). The script does not launch the next until the previous `REPORT.md` exists or the per-image timeout fires.

## GPU

`birefnet-portrait` uses `birefnet-portrait.fp16.onnx` on DirectML. rembg and SAM take a lock (`%LOCALAPPDATA%\CarouselCanvas\subject-extract\.gpu.lock`) so two jobs cannot share the card.

## Wipe

`sx wipe --xyxy "x1,y1,x2,y2" cutout.png cutout.png` — zeros alpha inside the box. Isolated specks only.
