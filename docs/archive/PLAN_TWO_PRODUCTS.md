# Plan: Split Into Two Independent Products (Two Repos)

**Status:** Planning (verified against codebase 2026-07-14; Appendix D resolved)  
**Date:** 2026-07-14 (updated)  
**Goal:** Turn the current monolith into **two completely separate products** — **two git repos**, **two folders**, **two virtual environments** — with **zero code dependency** between them. Both must preserve today’s behavior and output parity.

---

## 1. Executive summary

Today this repo is one Instagram layout app with **two pipelines** that already work:

| Product | What it does | Current code home | Output |
|---------|--------------|-------------------|--------|
| **Image Stacker** | Vertical collage layouts (stack-2/3, grids, combo, manual) | `app/engine/`, `app/preview.py` | One 3840×4800 JPEG per collage (**≤ 8 MB**) |
| **Carousel Canvas** | Horizontal combination mural → Instagram carousel | `app/strip/` | 10× 1080×1350 slice JPEGs (**≤ 8 MB** each); `*_wide.jpg` is uncapped internal master |

The split is **mostly extraction and decoupling**, not re-inventing algorithms.

**End state (locked in):**

- **Repo A** — Image Stacker — its own folder, `.git`, `.venv`, `requirements.txt`, GUI, CLI, tests, docs
- **Repo B** — Carousel Canvas — same independence
- **No monorepo.** No shared Python package. No imports across repos. No symlinked code between folders.
- Small I/O helpers (`parse_color`, `save_optimized`, path scanning) are **copied into each repo** and maintained independently.

After cutover, this monolith repo is **archived or deleted** — it is not kept as a parent workspace.

---

## 2. Product definitions

### 2.1 Image Stacker

**User-facing name (working):** Image Stacker / Layout Generator

**Scope:**
- Layouts from `LAYOUT_CONFIG`: `stack-2`, `stack-3`, `grid-1x2-v`, `grid-1x3-m`, `grid-2x4`, `grid-3x3`, `grid-2x2-v`
- Modes: single, batch, random, combo, manual
- Options: borderless, bleed, background color
- Preview via scaled collage render
- CLI without any `--strip*` flags

**Out of scope:** carousel murals, wide canvas, slicing, strip templates, hero pin, layout seed retry.

**Note:** GUI label **“Strip 1×3”** for layout key `grid-1x3-m` in `LAYOUT_CARD_LABELS` (`main_window.py`) is a **three-photo row layout**, not the carousel product. Rename in stacker UI copy (e.g. “Row 1×3”).

### 2.2 Carousel Canvas

**User-facing name (working):** Carousel Canvas / Mural Composer

**Scope:**
- Template-driven wide canvas (default `strip_mural_v2`)
- All **six** registered templates in `STRIP_TEMPLATE_OPTIONS` (`app/strip/template_data.py`):
  - `strip_mural_v2` (default CLI/GUI when `--strip-template` omitted)
  - `strip_polaroid_table_v1`
  - `strip_seamless_mosaic_v1`
  - `strip_seamless_v1`
  - `strip_mural_v1`
  - `strip_10col`
- Smart/dumb shuffle, repeats, layout seed, token retry, hero pin, card edges
- Manual slot swap UX (library → slot), horizontal scroll preview
- Export: wide master + 10 carousel slices
- CLI with `--strip*` flags only (no `--layout`, `--combo`, etc.)

**Out of scope:** vertical grid/stack layouts, combo playlists, `LAYOUT_CONFIG`, collage preview module.

---

## 3. Current architecture (baseline)

```
run_gui.py ──┐
script.py  ──┼──► app/cli.py ──┬──► app/engine/layout_engine.py  (stacker)
             │                 └──► app/strip/pipeline.py         (canvas)
             └──► app/gui/main_window.py (both modes in one window)
                        │
            app/strip/* ──imports──► app/engine/layout_engine.py   ◄── coupling
```

### 3.1 What is already separated

- `app/strip/` is self-contained for compose → slice → export (**16** Python modules under `app/strip/`, including `__init__.py`).
- `app/engine/` does **not** import strip.
- `app/preview.py` is collage-only (imports `create_collage`, `get_layout_geometry`, `process_image*` from engine — no strip).
- One-way dependency only: **strip → engine** (plus a few non-`strip/` callers listed below).

**Image Stacker API ownership (all in `app/engine/layout_engine.py` today):**

| Symbol | Used by stacker GUI/CLI/preview? | Used by strip/canvas? |
|--------|----------------------------------|------------------------|
| `create_collage` | yes (`preview.py`) | no |
| `build_collage_image` | yes (internal to layout jobs) | no |
| `run_collage_from_paths` | yes (**manual** GUI export) | no |
| `run_layout_job` | yes (single / batch / random) | no |
| `run_combo_job` | yes (combo mode) | no |

These five entry points stay in the stacker repo only. Canvas must not retain them.

### 3.2 Coupling points to eliminate

| Area | Coupling today | Target |
|------|----------------|--------|
| **Strip imports** | 6 `app/strip/*` modules import `layout_engine` (see §3.3) | Canvas repo owns all its I/O + image ops |
| **Engine bloat** | Strip-only symbols in `layout_engine.py`: `STRIP_CAROUSEL_SLICE_MAX_BYTES` (alias of `MAX_OUTPUT_JPEG_BYTES`), `_strip_trim_source_left`, `_strip_two_h_height_then_center_band`, `_cover_height_first_panned`, and mural kwargs on `process_image_panned` / `process_image_contained_panned` (`source_trim_left_frac`, `source_center_band_frac`) | Canvas `io.py` holds slice cap; mural image ops in `image_ops.py`; stacker `process_image*` drops mural kwargs |
| **CLI** | Single `app/cli.py` branches on `--strip` vs `--layout`/`--combo` | One CLI per repo |
| **GUI** | One `main_window.py` (**~2,528 lines**, both modes; **~301** lowercase `strip` references) | One GUI per repo |
| **GUI entry** | `app/gui/main.py` logs to `ImageLayoutApp/logs`, optional `--perf` → `app.perf` | Per-product `%LOCALAPPDATA%` paths; see §4.5 perf split |
| **Perf** | `app/perf/tracker.py` + `app/perf/harness.py` (harness is stacker-only) | Stacker: tracker + harness; canvas: tracker only (see §4.5) |
| **Scripts** | `scripts/strip_iteration_smoke.py` imports `get_valid_paths`, `parse_color` from engine | Canvas repo; redirect to local `io.py` after Phase 1 |
| **Settings** | `%LOCALAPPDATA%\ImageLayoutApp\settings.json` (mixed stacker + strip keys in one JSON) | Separate app data dirs per product |
| **Tests** | `tests/test_strip.py` imports `process_image_contained_panned`, `get_valid_paths` from engine; `test_hero_pin` / `test_layout_retry` are strip-only | Each repo’s tests import only local package |
| **Barrel export** | `app/engine/__init__.py` re-exports stacker public API | Stacker repo only |
| **Environment** | One `.venv`; `requirements.txt` (runtime) + PyInstaller ad hoc via `build_windows.ps1` | Two of each; each repo gets `requirements.txt` + `requirements-dev.txt` (see §4.6) |
| **Git** | One repo | Two repos |
| **Packaging** | `ImageLayoutApp.spec`, `build_windows.ps1` | One spec + build script per repo |

### 3.3 Strip → engine import inventory (must go to zero in canvas repo)

**Production `app/strip/*` (6 modules — complete as of baseline):**

| Strip module | Imports from `layout_engine` |
|--------------|------------------------------|
| `pipeline.py` | `STRIP_CAROUSEL_SLICE_MAX_BYTES`, `save_optimized` |
| `composer.py` | `parse_color`, `process_image_panned`, `process_image_contained_panned` |
| `gap_fill.py` | `process_image_panned` |
| `assign.py` | `get_image_aspect_hint` |
| `hero_pin.py` | `IMAGE_EXTENSIONS` |
| `layout_retry.py` | `parse_color` |

No other file under `app/strip/` imports `layout_engine`. The remaining 10 strip modules only import within `app.strip` or Pillow.

**Other canvas-side callers (not under `app/strip/`, still blocked after split):**

| Path | Imports from `layout_engine` |
|------|------------------------------|
| `app/cli.py` (strip branch) | `get_valid_paths`, `parse_color` |
| `scripts/strip_iteration_smoke.py` | `get_valid_paths`, `parse_color` |
| `tests/test_strip.py` | `process_image_contained_panned`, `get_valid_paths` (unit tests for image ops / fixtures) |

Phase 1 grep should be `rg "from app\.engine" app/strip app/cli.py scripts/strip_iteration_smoke.py tests/test_strip.py` → **no matches** before Phase 2 copy.

---

## 4. Target architecture — two repos, two folders

### 4.1 On disk (example layout)

Two sibling folders anywhere on the machine — **not** nested inside this repo:

```
C:\Users\jordi\Desktop\coding stuff\
├── image-stacker\              # Git repo A
│   ├── .git\
│   ├── .venv\                  # python -m venv .venv
│   ├── requirements.txt
│   ├── README.md
│   ├── run_gui.py
│   ├── script.py
│   ├── app\
│   │   ├── cli.py
│   │   ├── preview.py
│   │   ├── engine\
│   │   │   └── layout_engine.py
│   │   ├── gui\
│   │   │   ├── main.py
│   │   │   └── main_window.py
│   │   └── io.py               # paths, colors, JPEG save (stacker copy)
│   ├── tests\
│   ├── TEST IMAGES\            # own copy of fixtures
│   ├── docs\
│   ├── .cursor\rules\
│   └── ImageStacker.spec
│
└── carousel-canvas\            # Git repo B
    ├── .git\
    ├── .venv\
    ├── requirements.txt
    ├── README.md
    ├── run_gui.py
    ├── script.py
    ├── app\
    │   ├── cli.py
    │   ├── io.py               # same ideas, independent copy
    │   ├── image_ops.py        # panned cover/contain + mural trim/band
    │   ├── strip\              # full pipeline (from current app/strip/)
    │   └── gui\
    │       ├── main.py
    │       └── main_window.py
    ├── tests\
    ├── TEST IMAGES\
    ├── docs\
    ├── scripts\
    ├── .cursor\rules\
    └── CarouselCanvas.spec
```

Folder and repo names are placeholders — pick final names once, use consistently for `%LOCALAPPDATA%` paths and exe names.

### 4.2 Independence rules

| Resource | Image Stacker | Carousel Canvas |
|----------|---------------|-----------------|
| Git remote | Own origin | Own origin |
| Virtual env | Own `.venv` | Own `.venv` |
| Dependencies | Own `requirements.txt` | Own `requirements.txt` |
| Python package | `app` (stacker-only) | `app` (canvas-only) |
| Settings | `%LOCALAPPDATA%\ImageStacker\` | `%LOCALAPPDATA%\CarouselCanvas\` |
| Logs | `%LOCALAPPDATA%\ImageStacker\logs\` | `%LOCALAPPDATA%\CarouselCanvas\logs\` |
| Test fixtures | Copy of `TEST IMAGES` | Copy of `TEST IMAGES` |
| Golden outputs | `tests/fixtures/golden/` | `tests/fixtures/golden/` |

**Allowed:** Both use Pillow + stdlib only.  
**Forbidden:** Any import, git submodule, pip dependency, or shared folder between repos.  
**Forbidden:** A third “common” library repo (unless you explicitly add one later — out of scope for now).

**Duplicated code:** `app/io.py` (~150–250 lines) exists in **both** repos as separate files. Each copy defines the same **`MAX_OUTPUT_JPEG_BYTES = 8 * 1024 * 1024`** and passes it to `save_optimized` for all user-facing exports. They may diverge otherwise; no sync tooling required.

### 4.3 Output size policy (resolved)

**Requirement:** Every **final deliverable** JPEG must be **≤ 8 MB** (Instagram upload limit).

| Output | Product | Capped? | Mechanism today |
|--------|---------|---------|-----------------|
| Stacker collage export | Image Stacker | yes | `save_optimized(..., max_file_size=MAX_OUTPUT_JPEG_BYTES)` via `MAX_FILE_SIZE` default |
| Carousel slices `*_01.jpg` … `*_10.jpg` | Carousel Canvas | yes | `save_optimized(..., max_file_size=MAX_OUTPUT_JPEG_BYTES)` + HQ quality ladder |
| Wide master `*_wide.jpg` | Carousel Canvas | **no** | `save_optimized(..., max_file_size=None)` — internal source for slicing, not an upload target |

After split, **both repos** define `MAX_OUTPUT_JPEG_BYTES = 8 * 1024 * 1024` in their own `app/io.py`. The monolith previously used a **7 MB** slice cap; that is **retired** — slices now use the same **8 MB** ceiling as stacker collages (binary search in `save_optimized` guarantees `file_size ≤ cap`).

**Phase 4 tests (both repos):** assert `path.stat().st_size <= MAX_OUTPUT_JPEG_BYTES` on every deliverable export in smoke tests.

### 4.4 Canvas-specific image operations

In the **canvas repo only**, `app/image_ops.py` holds:
- `_strip_trim_source_left`, `_strip_two_h_height_then_center_band`, `_cover_height_first_panned`
- `process_image_panned` / `process_image_contained_panned` with mural kwargs (`source_trim_left_frac`, `source_center_band_frac`)
- `get_image_aspect_hint` (smart shuffle)

In **both** repos, `app/io.py` (copied independently) should hold at minimum: `parse_color`, `save_optimized`, `get_valid_paths`, `IMAGE_EXTENSIONS`, `fix_orientation`, `flatten_alpha` — extracted from today’s `layout_engine.py`.

The **stacker repo** keeps a slimmer `layout_engine.py` with collage geometry, `create_collage`, job runners, and stacker `process_image*` **without** mural trim/band kwargs.

### 4.5 Performance tracing (resolved)

`app/perf/tracker.py` is a **no-op unless enabled** (`span()` / `record_since()` return immediately when tracing is off). Overhead when disabled is negligible.

| Piece | Image Stacker repo | Carousel Canvas repo |
|-------|------------------|----------------------|
| `app/perf/tracker.py` | **yes** — copy as-is | **yes** — copy as-is |
| `app/perf/harness.py` | **yes** — automated GUI perf runs (stacker modes only) | **no** — not applicable |
| `app/gui/main.py` `--perf` / `--perf-out` | **yes** | **yes** (same CLI flags for consistency) |
| Env var to enable | `IMAGE_STACKER_PERF=1` (rename from `IMAGELAYOUT_PERF` at split) | `CAROUSEL_CANVAS_PERF=1` |
| `main_window.py` span calls | keep (`gui.preview.*`, layout cards, manual stage, etc.) | keep (`gui.stage.build_strip_image`, thumb pump, strip export, etc.) |
| `perf_reports/` | yes | yes (when `--perf` used) |

**Do not** strip `span()` imports from canvas GUI during Phase 3 — keep the tracker module instead. **Do not** copy the harness to canvas.

### 4.6 Dependencies & packaging (resolved)

**Runtime** — each repo’s `requirements.txt`:

```text
pillow>=10.0.0
```

**Build / dev** — each repo’s `requirements-dev.txt`:

```text
pyinstaller>=6.0
```

Install: `pip install -r requirements.txt -r requirements-dev.txt` (dev machines only).

**Per repo:**
- `build_windows.ps1` — calls `.venv\Scripts\python.exe -m PyInstaller` (same pattern as today’s monolith script)
- `ImageStacker.spec` or `CarouselCanvas.spec` — one-file windowed exe; entry `app.gui.main`
- README **Build** section: `pip install -r requirements-dev.txt` then `.\build_windows.ps1`

PyInstaller stays **out of** `requirements.txt` so end users / CI smoke tests only need Pillow.

### 4.7 Git history strategy

Choose one when creating the two repos:

| Strategy | Pros | Cons |
|----------|------|------|
| **A. Fresh repos** (`git init` + copy files) | Cleanest; no strip noise in stacker history | Lose unified history |
| **B. Filtered history** (`git filter-repo` per product) | Preserved commits per product | More setup; filter mistakes are painful |
| **C. Archive monolith + fresh repos** | Monolith stays read-only reference | Same as A for day-to-day work |

**Recommendation:** **C** — tag this repo `v1-monolith-archive`, create two fresh repos, copy code. Simplest for two unrelated products.

---

## 5. Migration phases

Work from **this monolith repo** as the source of truth until cutover. Each phase ends with green tests and golden JPEG parity.

### Phase 0 — Baseline in monolith (1 day)

**Objectives:** Lock behavior before any repo split.

- [ ] Export golden outputs from both pipelines (`TEST IMAGES/`):
  - Stacker: one JPEG per layout + one combo sample
  - Canvas: one export per template (`--strip-template` each)
- [ ] Save SHA-256 hashes in `docs/SPLIT_BASELINE_HASHES.txt` (committed to monolith)
- [ ] Run: `python -m unittest discover -s tests -v`
- [ ] Tag monolith: `git tag pre-split-baseline`

**Exit criteria:** Reproducible reference outputs you can compare against in each new repo.

---

### Phase 1 — Decouple canvas code in monolith (2–4 days)

**Objectives:** Prove `app/strip/` runs without `app/engine/` — still in this repo, but ready to lift out.

1. Add `app/io.py` and `app/image_ops.py` under a canvas-oriented structure (or temp `app/strip/core/`).
2. Redirect all `app/strip/*` imports from `layout_engine` → local `io` / `image_ops`.
3. Remove strip-only symbols from `app/engine/layout_engine.py`.
4. Verify strip tests and CLI exports match Phase 0 hashes.

**Verification:**
- `rg "from app\.engine" app/strip app/cli.py scripts/strip_iteration_smoke.py tests/test_strip.py` → **no matches**
- `python -m unittest tests.test_strip tests.test_hero_pin tests.test_layout_retry -v`
- `python -m unittest tests.test_strip.TestStripExportSmoke -v` (all **six** `STRIP_GUI_TEMPLATE_IDS`)

This phase can happen entirely in the monolith before creating new folders.

---

### Phase 2 — Create two repos (2–3 days)

**Objectives:** Two new folders on disk, two `git init`, no link to each other.

#### Repo A — Image Stacker

1. `mkdir` new folder → `git init`
2. Copy in:
   - `app/engine/`, `app/preview.py`
   - Stacker half of `app/cli.py` → `app/cli.py`
   - Stacker half of `app/gui/main_window.py` → `app/gui/main_window.py`
   - `app/gui/main.py` (adapt logging paths)
   - `app/io.py` (extract from `layout_engine`: colors, paths, JPEG save)
   - `app/engine/__init__.py` (barrel re-exports)
   - `app/perf/` (GUI perf harness — stacker modes only)
   - `run_gui.py`, `script.py` shims
   - `requirements.txt`, `readme.txt` → `README.md`
   - `tests/` (new stacker smoke tests; **no** `test_strip*` / `test_hero_pin` / `test_layout_retry`)
   - Copy `TEST IMAGES/`
   - Stacker-relevant `docs/`, `PLAN.md` as historical
   - `.cursor/rules/` (stacker run commands only)
3. `python -m venv .venv` → `pip install -r requirements.txt`
4. Remove **all** strip imports and files — repo must not contain `app/strip/`
5. Initial commit + optional remote

#### Repo B — Carousel Canvas

1. Same isolation steps in a **different** folder
2. Copy in:
   - `app/strip/` → `app/strip/` (or rename to `app/canvas/` if desired)
   - `app/io.py`, `app/image_ops.py`
   - Canvas half of `app/cli.py`, `app/gui/`
   - `tests/test_strip.py`, `test_hero_pin.py`, `test_layout_retry.py` (only canvas tests in monolith today)
   - `docs/PLAN_STRIP_*`, `SPEC_STRIP_*`, `STRIP_WHAT_YOU_WANT.md`, `POLAROID_TEXTURE_IMPLEMENTATION.md`, `PROMPT_STRIP_SEAMLESS_MOSAIC.md`, etc.
   - `scripts/strip_iteration_smoke.py`
   - Copy `TEST IMAGES/`
   - Canvas `.cursor/rules/`
3. Own `.venv`, own `requirements.txt`
4. Remove **all** `layout_engine`, `LAYOUT_CONFIG`, combo, collage preview code
5. Initial commit + optional remote

**Verification (per repo, in its own folder with its own venv):**
- Stacker: `python run_gui.py`, `python script.py "TEST IMAGES" --layout stack-3`
- Canvas: `python run_gui.py`, `python script.py "TEST IMAGES" --strip`
- Golden hashes match Phase 0
- `rg "app\.strip|compose_strip|export_strip|STRIP_TEMPLATE" app/` in stacker repo → no canvas leakage
- `rg "layout_engine|LAYOUT_CONFIG|run_combo|run_layout_job|render_collage_preview|app\.preview" app/` in canvas repo → no stacker leakage

---

### Phase 3 — GUI hardening per repo (3–5 days)

**Objectives:** Each `main_window.py` is lean and product-specific (trim dead branches from the copy-paste split).

**Stacker GUI** — keep:
- Modes: single, batch, random, combo, manual
- Layout cards, collage preview, combo navigation
- Manual slot UX (collage slots only)

**Canvas GUI** — keep:
- Template picker, shuffle, layout seed, card edge, hero-only
- Slot library + undo/redo + horizontal scroll stage
- `compose_strip_wide` preview cache

**Per repo:** fix settings load/save for new `%LOCALAPPDATA%` paths; optional one-time import of old `ImageLayoutApp\settings.json` keys (each app reads only its own keys).

---

### Phase 4 — Tests, docs, cursor rules (2 days)

**Stacker repo**
- Add `tests/test_layout_smoke.py` — every `LAYOUT_CONFIG` key exports once
- `tests/test_preview.py` — preview dimensions vs canvas size
- `docs/` — layout catalog, manual slot order
- `.cursor/rules/run-with-python.mdc` — stacker commands only

**Canvas repo**
- Keep existing strip test suite; fix imports to local `app`
- `docs/` — mural specs, template IDs
- `.cursor/rules/` — canvas export smoke (`--strip-template` examples)

No cross-repo test runner. CI (if added later) is **per repo**.

---

### Phase 5 — Packaging per repo (1–2 days)

**Stacker:** `ImageStacker.spec` → `app.gui.main`, exe name `ImageStacker.exe`  
**Canvas:** `CarouselCanvas.spec` → `app.gui.main`, exe name `CarouselCanvas.exe`

Each repo has its own `build_windows.ps1` if needed. Building one exe does not require the other repo on disk.

---

### Phase 6 — Monolith retirement (0.5 day)

- [ ] Tag monolith `v1-monolith-final`
- [ ] Add `ARCHIVED.md` at monolith root pointing to the two new repo paths / remotes
- [ ] Stop developing in monolith; optionally delete folder after both products are verified

---

## 6. GUI decomposition guide

Source: `app/gui/main_window.py` (**~2,528 lines**; **~301** `strip` substring hits) in monolith. Split by **copy → delete branches → harden** into each repo.

| Concern | Stacker repo | Canvas repo |
|---------|--------------|-------------|
| Mode radio | single, batch, random, combo, manual | strip / mural (fixed; internal `mode_var` value `"strip"`) |
| Layout template cards | yes (`LAYOUT_GRID_ORDER` / `LAYOUT_CARD_LABELS`) | no |
| Strip template combo | no | yes (`STRIP_TEMPLATE_OPTIONS`) |
| `render_collage_preview` | yes (collage modes) | no |
| `compose_strip_wide` preview | no | yes (cached wide master + horizontal scroll stage) |
| Slot assignments + undo/redo | **manual** only | **always** (strip is slot-driven) |
| Shared UX to untangle | Both modes today share `_slot_assignments`, `ManualSlotFill`, thumb drag-drop, undo/redo stacks when `mode_var in ("manual", "strip")` — copy the pattern per repo, do not share code |
| Stage layout | vertical collage slots | horizontal scroll mural |

**Stacker export APIs (by mode — do not conflate):**

| GUI mode | Export function |
|----------|-----------------|
| single / batch / random | `run_layout_job` |
| combo | `run_combo_job` |
| manual | `run_collage_from_paths` (+ `generate_output_filename` for output path) |

**Canvas export:** `export_strip_carousel` (strip mode only).

Thumbnail grid, folder pickers, worker threads: **duplicate** the pattern in each repo (~100–200 lines each). Do not extract to a shared library.

`app/gui/main.py`: both repos get a trimmed copy — update `%LOCALAPPDATA%` dir name; both keep `--perf` / `app.perf.tracker` (see §4.5). Stacker also ships `app/perf/harness.py`; canvas does not.

---

## 7. CLI per repo

### Image Stacker (`python script.py` in stacker folder)

```text
python script.py <folder> [--layout stack-3] [--output ...] [--batch] [--random]
                 [--combo] [--count N] [--borderless] [--bleed] [--color ...]
```

### Carousel Canvas (`python script.py` in canvas folder)

```text
python script.py <folder> --strip [--strip-template strip_mural_v2] [--strip-dumb-shuffle]
                 [--strip-allow-repeats] [--strip-layout-seed N] [--strip-no-layout-retry]
                 [--strip-hero-image PATH] [--strip-card-edge borderless|wedges|border]
                 [--output ...] [--color ...]
```

Keep canvas flag names stable for existing scripts. Stacker repo does not define `--strip` at all.

---

## 8. Risk register

| Risk | Impact | Mitigation |
|------|--------|------------|
| JPEG drift after code move | High | Phase 0 golden hashes; compare in each repo |
| Incomplete GUI trim (dead imports) | Medium | Smoke GUI + export in each repo’s venv |
| Duplicated `io.py` diverges | Low | Acceptable; products are independent |
| Wrong files copied to wrong repo | Medium | Import grep checklist in Phase 2 |
| Two venvs to maintain | Low | Identical `requirements.txt` (pillow) unless needs diverge |
| User has old `ImageLayoutApp` settings | Low | Optional one-time key migration per app |

---

## 9. Success criteria

Split is **done** when:

1. **Two git repos** on disk, each with its own `.git` and remote (if used).
2. **Two `.venv` folders** — installing/running one project never activates the other’s code.
3. **Zero cross-repo imports** — neither repo references the other’s path or package name.
4. **Parity:** exports in each repo match Phase 0 golden hashes.
5. **Stacker repo** contains no `app/strip/`, no `--strip` CLI, no mural GUI.
6. **Canvas repo** contains no `layout_engine` / `LAYOUT_CONFIG` / combo / `app/preview.py` / `render_collage_preview`.
7. **Independent smoke:**
   - Stacker only: `python run_gui.py` from stacker folder
   - Canvas only: `python run_gui.py` from canvas folder
8. Each repo has README, tests, and run instructions that make sense **without mentioning the other product**.
9. **Output size:** every deliverable JPEG (stacker collages; canvas slices `*_01`…`*_10`) has `stat().st_size <= 8 * 1024 * 1024`. Wide `*_wide.jpg` is exempt (internal).

---

## 10. Effort estimate

| Phase | Duration |
|-------|----------|
| 0 Baseline (monolith) | 1 day |
| 1 Decouple strip in monolith | 2–4 days |
| 2 Create two repos | 2–3 days |
| 3 GUI hardening | 3–5 days |
| 4 Tests & docs | 2 days |
| 5 Packaging | 1–2 days |
| 6 Archive monolith | 0.5 day |
| **Total** | **~12–18 days** |

Phase 1 can start in this repo immediately. Phase 2 is the fork into two folders.

---

## 11. What we are explicitly not doing

- Monorepo or shared parent folder for both products
- Git submodules or a shared pip package between repos
- Unified launcher app that opens both products
- Rewriting compose/slice algorithms or template geometry
- Adding features during the split (only parity + separation)

---

## 12. Immediate next steps

1. **Pick final folder names and git remote names** (e.g. `image-stacker`, `carousel-canvas`).
2. **Phase 0** — golden exports + tag `pre-split-baseline` in this repo.
3. **Phase 1** — decouple `app/strip` from `app/engine` here (proves canvas can stand alone).
4. **Phase 2** — `mkdir` two folders, `git init` twice, copy split code, create two `.venv`s.
5. Smoke both repos on the same machine — confirm they do not share `PYTHONPATH` or working tree.

---

## Appendix A — Monolith → repo file map

| Monolith path | Image Stacker repo | Carousel Canvas repo |
|---------------|--------------------|-----------------------|
| `app/engine/layout_engine.py` | `app/engine/layout_engine.py` (trimmed; no strip symbols) | — |
| `app/engine/__init__.py` | yes | — |
| `app/preview.py` | `app/preview.py` | — |
| `app/strip/**` | — | `app/strip/**` (16 modules) |
| `app/cli.py` | stacker branch only (`--layout`, `--combo`, …) | canvas branch only (`--strip*`, `--color`) |
| `app/gui/main_window.py` | stacker GUI (trimmed) | canvas GUI (trimmed) |
| `app/gui/main.py` | yes (`ImageStacker` log path; keep `--perf`) | yes (`CarouselCanvas` log path) |
| `run_gui.py`, `script.py` | yes | yes |
| `tests/test_strip.py`, `test_hero_pin.py`, `test_layout_retry.py` | — | yes |
| `tests/` (new layout smoke) | yes | — |
| `docs/PLAN_STRIP_*`, `SPEC_*`, `STRIP_WHAT_YOU_WANT.md`, `POLAROID_*`, `PROMPT_STRIP_*` | — | yes |
| `docs/DESIGN_STUDIO.md`, `DESIGN_MANUAL_THREE_VARIANTS.md` | yes (stacker GUI design) | — |
| `docs/PERFORMANCE.md`, `perf_reports/` (optional) | yes | — |
| `PLAN.md` | optional historical | — |
| `TEST IMAGES/` | copy | copy |
| `app/perf/` | yes (`tracker.py` + `harness.py`) | yes (`tracker.py` only) |
| `scripts/strip_iteration_smoke.py` | — | yes |
| `build_windows.ps1` | adapt exe name | adapt exe name |
| `ImageLayoutApp.spec` | → `ImageStacker.spec` | → `CarouselCanvas.spec` |
| `.cursor/rules/` | stacker rules only | canvas rules (`after-changes-strip-export.mdc`) |

## Appendix B — New repo bootstrap checklist

**Per repo, after copy:**

```text
cd <repo-folder>
python -m venv .venv
.venv\Scripts\pip install -r requirements.txt
# Dev/build machines only:
.venv\Scripts\pip install -r requirements-dev.txt
.venv\Scripts\python -m unittest discover -s tests -v
.venv\Scripts\python run_gui.py
```

**Sanity greps:**

```text
# In stacker repo — should return nothing under app/:
rg "app\.strip|compose_strip|export_strip|STRIP_TEMPLATE|strip_mural" app/

# In canvas repo — should return nothing under app/:
rg "layout_engine|LAYOUT_CONFIG|run_combo|run_layout_job|run_collage_from_paths|create_collage|render_collage_preview|app\.preview" app/
```

## Appendix C — Terminology (post-split)

| Term | Repo |
|------|------|
| stack-2, stack-3, grid-* | Image Stacker |
| Row 1×3 (`grid-1x3-m`) | Image Stacker (rename from “Strip 1×3”) |
| mural, carousel, wide canvas, strip template | Carousel Canvas |
| `compose_strip_wide` | Carousel Canvas |
| `create_collage` / `build_collage_image` | Image Stacker |
| `run_layout_job` (single/batch/random) | Image Stacker |
| `run_combo_job` | Image Stacker |
| `run_collage_from_paths` (manual) | Image Stacker |
| `generate_output_filename` | Image Stacker |

---

## Appendix D — Resolved decisions (was open questions)

### D.1 Performance tracing

**Decision:** Both repos ship `app/perf/tracker.py`. Canvas does **not** ship `harness.py`.

- `span()` / `record_since()` are no-ops when tracing is off — safe to keep all existing calls in both GUIs.
- Stacker: full perf story (`--perf`, harness, `IMAGE_STACKER_PERF=1`).
- Canvas: `--perf` + `CAROUSEL_CANVAS_PERF=1` for ad-hoc reports; no automated harness.

### D.2 JPEG output size (≤ 8 MB deliverables)

**Decision:** All **user-facing** exports use **`MAX_OUTPUT_JPEG_BYTES = 8 * 1024 * 1024`** in each repo’s `app/io.py`.

| Deliverable | Cap |
|-------------|-----|
| Stacker collage JPEGs | ≤ 8 MB |
| Carousel slice JPEGs (`*_01` … `*_10`) | ≤ 8 MB |
| Wide master `*_wide.jpg` | **Uncapped** (internal; not uploaded) |

`save_optimized` binary-searches JPEG quality until `file_size <= max_file_size`. Slices additionally use the HQ ladder (`jpeg_quality_first=98`, `min=86`, `max=98`) before the size cap applies.

The old **7 MB** slice cap is **removed** (monolith updated 2026-07-14) so slices can use the full 8 MB budget for quality while still meeting the upload limit.

Each repo’s Phase 4 smoke tests must assert deliverable file sizes.

### D.3 PyInstaller / build dependencies

**Decision:**

- `requirements.txt` — runtime only (`pillow>=10.0.0`)
- `requirements-dev.txt` — `pyinstaller>=6.0`
- Each repo: `build_windows.ps1` + product-specific `.spec` + README build instructions

End users running from source never need PyInstaller installed.
