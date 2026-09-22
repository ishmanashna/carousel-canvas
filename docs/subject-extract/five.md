# Five-photo 03.1 run

Parallel seats (`run-folder.ps1 -Seats 2`): up to two Composer agents look/decide at once. rembg + SAM still take **one GPU seat** via `gpu_lock.py` (heartbeat + dead-PID steal). Do not open five Cursor chats by hand.

| Order | Photo | Why |
| --- | --- | --- |
| 1 | **MIII1129** | Known missing horn |
| 2 | **MIII1196** | Tight trumpet, bell is most of the frame |
| 3 | **MIII1136** | Guitarist + extra person; wipe-test for floating junk |
| 4 | **MIII1320** | Trumpet + unheld drums |
| 5 | **MIII1143** | Several people; keep them if they come out clean |

**Go (from repo root):**

```powershell
powershell -File tools\subject-extract\run-folder.ps1 `
  -InputDir "TEST IMAGES" `
  -OutputDir output\subject_extract\five `
  -Include MIII1129,MIII1196,MIII1136,MIII1320,MIII1143 `
  -Seats 2
```

Short tonight smoke (recommended first): same command with `-Include MIII1129,MIII1136` only.

Lock-only smoke (no agents / no GPU models):

```powershell
& "$env:LOCALAPPDATA\CarouselCanvas\subject-extract\venv\Scripts\python.exe" tools\subject-extract\smoke_seats.py
```

## Observability

| File | What |
| --- | --- |
| `output/subject_extract/five/FOLDER.jsonl` | start/done/skip/timeout per photo, wall seconds, seats |
| `…/MIII*/ACTS.jsonl` | every `sx` call: elapsed, exit, providers, `gpu_lock_wait_seconds` |
| `…/MIII*/TIMING.md` | rembg / sam / wipe / look overhead from ACTS |
| `…/MIII*/REPORT.md` | verdict + what was missing / wiped |
| `…/MIII*/agent.stdout.txt` | agent CLI log |
| `output/subject_extract/five/compare_finals/` | `cutout_full.png` copies |
| `output/subject_extract/five/METRICS.md` | rolled-up table when summarize-run runs |

Parent ranks `compare_finals/` 1–10. Bottlenecks = rembg vs sam vs wipe vs look vs GPU wait.

Do not launch the full five until the two-photo smoke looks sane. No overnight folder yet.
