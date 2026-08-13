# GUI performance from the IDE

No manual Task Manager steps: tracing is **off by default** and writes **Markdown + JSON** under `perf_reports/` when you ask for it.

## 1. Automated harness (recommended)

Drives the real Tk window through scripted mode changes, folder load, manual thumbnails, resize, then a final **single-mode** step so the background preview thread is measured (manual mode only paints on the UI thread). Default run length scales with how many images are in the folder (capped), plus ~**0.85s** drain before the report.

```bash
python -m app.perf
```

By default the harness uses the project’s **`TEST IMAGES`** folder (or the first immediate subfolder inside it that contains JPG/PNG files). If that tree is empty, it falls back to small synthetic PNGs. Override:

```bash
python -m app.perf --photo-dir "D:\your\photos"
```

`--settle-ms` defaults from image count (more files → longer run so thumbnail pumping can finish). Override explicitly when needed.

Outputs:

- `perf_reports/perf_harness_<UTC>.md` — sortable table (total / mean / p50 / p95).
- `perf_reports/perf_harness_<UTC>.json` — same data for tooling.

## 2. Interactive session with tracing

Use the normal launcher plus `--perf`. Close the window when you are done; a report is written on exit (`perf_*_atexit_*`).

```bash
python run_gui.py --perf
```

Custom output directory:

```bash
python run_gui.py --perf --perf-out D:\reports\perf_runs
```

Or enable without CLI flags:

```text
set IMAGELAYOUT_PERF=1
python run_gui.py
```

## 3. What the spans mean

| Span | Meaning |
|------|--------|
| `gui.main_window.init` | One-shot window build + settings + first scheduled work |
| `engine.get_valid_paths` | Directory scan + per-file orientation check (can dominate big folders) |
| `gui.run_estimate` | UI text that calls into path scanning |
| `gui.preview.render_worker` | Background thread: collage preview render + thumbnail |
| `gui.thumb.pump_batch` | One batch of manual-mode thumbnails (main thread, chunked `after`) |
| `gui.stage.paint` / `gui.stage.build_manual_image` | Live stage redraw (manual mode is heavier) |
| `harness.session_wall` | Harness only: wall time from window open to report |
| `gui.interaction.mode_change` | Whole mode switch handler (thumbs reload, preview schedule, etc.) |
| `gui.interaction.layout_change` | Layout change handler (slot reset, thumb reload in manual, preview) |
| `gui.interaction.layout_card_pick` | Clicking a layout card (includes `layout_change`) |
| `gui.interaction.thumb_release` | Thumb mouse-up (drop on stage vs fill-next-slot) |
| `gui.interaction.assign_to_slot` | Applying one photo to a slot (often nested inside `thumb_release`) |

Mode/layout tweaks and drag-to-preview are **not** separate magic probes: they run real handlers above, which in turn charge `gui.stage.*`, `gui.thumb.*`, `engine.get_valid_paths`, and `gui.preview.render_worker` where applicable.

**Run** (export collages) is timed separately in the status line when you press Run; these spans focus on **normal UI** cost.
