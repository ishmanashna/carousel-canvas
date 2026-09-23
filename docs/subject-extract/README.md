# Subject extract

Local cutout kit: one photo in, PNG of the people (and what they hold) out.

Entry: `tools\subject-extract\sx.ps1`. Per-photo process: [03.1.md](03.1.md).

```powershell
$sx = "tools\subject-extract\sx.ps1"
& $sx doctor
```

First-time weights: `python tools/subject-extract/prefetch.py`.

## Point at a folder (the real run)

One pass (no retry round). No wipe / cleanup phase: rembg seed, optional SAM for missing bits, lift. Every photo gets a folder under the output dir whether the cutout worked or not. `compare_finals\` copies `cutout_full.png` when present, otherwise `cutout.png`.

```powershell
powershell -File tools\subject-extract\run-folder.ps1 `
  -InputDir "path\to\photos" `
  -OutputDir output\subject_extract\batch `
  -Seats 8
```

Optional: `-Include MIII1129,MIII1196` (order-preserving), `-Seats 1..12` (default 2), `-TimeoutSeconds 2400`, `-DoneCutoutsDir`, `-Model composer-2.5`.

To survive closing Cursor, start through `start-detached.ps1` (new console, job-breakaway, `runner.log` + `runner.pid` in the output dir). This Nueva carpeta job: `tools\subject-extract\launch-nueva-carpeta.ps1`. Stop later with `stop-detached.ps1`.

TEST IMAGES launcher (skips DONE keepers): `tools\subject-extract\launch-test-folder-full.ps1`.

After a folder run: look-only triage (no GPU) — [triage.md](triage.md). The pass after a second cutout uses [triage-second.md](triage-second.md) (`good` or `needs_cleanup` only). Each seat gets up to 5 photos; at most 5 seats at once. This folder’s second triage: `tools\subject-extract\launch-triage-round2-nueva.ps1`. Stage order: [pipeline.md](pipeline.md). Cleanup session: [cleanup.md](cleanup.md), launcher `tools\subject-extract\launch-cleanup-nueva.ps1`.

Five-photo rehearsal notes: [five.md](five.md). Ten-seat TEST IMAGES recipe: [ten.md](ten.md).

## GPU

`birefnet-portrait` uses `birefnet-portrait.fp16.onnx` on DirectML. rembg and SAM take `%LOCALAPPDATA%\CarouselCanvas\subject-extract\.gpu.lock` (heartbeat; steal only if the holder PID is dead or 20 min stale).

`sx rembg` retries empty GPU masks in a new process (60s × 8). Do not mark a photo BLOCKED because rembg came back empty once.

## Wipe (not in 03.1)

The kit still has `sx wipe` and `sx wipe-islands`. The folder prompt and [03.1.md](03.1.md) ban them. Agents must not run them.

## Observability

Per run: `FOLDER.jsonl`, `compare_finals\`, `METRICS.md`. Per photo: `ACTS.jsonl`, `TIMING.md`, `REPORT.md`, `cutout_full.png`.
