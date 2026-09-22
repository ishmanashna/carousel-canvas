# Subject extract

Local cutout kit: one photo in, PNG of the people (and what they hold) out.

Entry: `tools\subject-extract\sx.ps1`. Per-photo process: [03.1.md](03.1.md).

```powershell
$sx = "tools\subject-extract\sx.ps1"
& $sx doctor
```

First-time weights: `python tools/subject-extract/prefetch.py`.

## Point at a folder (the real run)

One command. Composer seats look in parallel; rembg + SAM take **one GPU lock**. Skips stems that already have `cutout_full.png` in the output dir or in `output\subject_extract\DONE_CUTOUTS\<stem>\`.

```powershell
powershell -File tools\subject-extract\run-folder.ps1 `
  -InputDir "path\to\photos" `
  -OutputDir output\subject_extract\batch `
  -Seats 8
```

Optional: `-Include MIII1129,MIII1196` (order-preserving), `-Seats 1..12` (default 2), `-TimeoutSeconds 2400`, `-DoneCutoutsDir`, `-Model composer-2.5`.

TEST IMAGES launcher (skips DONE keepers): `tools\subject-extract\launch-test-folder-full.ps1`.

Five-photo rehearsal notes: [five.md](five.md). Ten-seat TEST IMAGES recipe: [ten.md](ten.md).

## GPU

`birefnet-portrait` uses `birefnet-portrait.fp16.onnx` on DirectML. rembg and SAM take `%LOCALAPPDATA%\CarouselCanvas\subject-extract\.gpu.lock` (heartbeat; steal only if the holder PID is dead or 20 min stale).

`sx rembg` retries empty GPU masks in a new process (60s × 8). Do not mark a photo BLOCKED because rembg came back empty once.

## Wipe

`sx wipe --xyxy "x1,y1,x2,y2" cutout.png cutout.png` — zeros alpha inside the box. Isolated specks only.

`sx wipe-islands cutout.png cutout.png` — drops floating blobs (detached hand, specks). Keeps people and bits within 16px of them (horn gap). Always run before lift.

## Observability

Per run: `FOLDER.jsonl`, `compare_finals\`, `METRICS.md`. Per photo: `ACTS.jsonl`, `TIMING.md`, `REPORT.md`, `cutout_full.png`.
