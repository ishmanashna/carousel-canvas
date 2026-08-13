# Carousel Canvas

Template-driven Instagram carousel murals: compose one wide master image and export 10 vertical 1080x1350 JPEG slices.

Standalone product — no dependency on the Image Stacker monolith or `image-stacker` repo.

## Requirements

- Python **3.10+**
- [Pillow](https://pypi.org/project/pillow/) (see `requirements.txt`)
- Tkinter (included with most Python installs; needed for the GUI)

## Quick start

```text
python -m venv .venv

# Windows
.venv\Scripts\pip install -r requirements.txt
.venv\Scripts\python.exe run_gui.py

# macOS / Linux
source .venv/bin/activate
pip install -r requirements.txt
python run_gui.py
```

### Fixture photos (optional)

For local dev you can keep a `TEST IMAGES/` folder at the repo root (gitignored). Tests synthesize small JPGs when that folder is missing. The baseline-hash script (`scripts/generate_baseline_hashes.py`) expects real photos there.

## CLI (strip export only)

```text
python script.py "path/to/photos" --strip --output output/strip_latest --strip-layout-seed 7
```

Equivalent: `python -m app.cli …`

Default template is `strip_mural_v2`. Other templates need `--strip-template <id>` — see `STRIP_TEMPLATE_OPTIONS` in `app/strip/template_data.py`.

**Photo counts:** `strip_mural_v2` needs **35 unique** photos by default. If your folder is smaller, add `--strip-allow-repeats` (GUI: “Allow repeating photos”). `strip_10col` only needs 10.

## Templates

- `strip_mural_v2` (default)
- `strip_polaroid_table_v1`
- `strip_seamless_mosaic_v1`
- `strip_seamless_v1`
- `strip_mural_v1`
- `strip_10col`

## Tests

```text
python -m unittest tests.test_hero_pin tests.test_layout_retry tests.test_strip -v
```

The full suite takes several minutes (export smoke test runs every template).

## Build (optional)

```text
pip install -r requirements-dev.txt
.\build_windows.ps1
```

Produces `dist\CarouselCanvas.exe` via PyInstaller (`run_gui.py` entry point).

## Docs

- `docs/SPEC_STRIP_MURAL_V2.md` — mural v2 layout spec
- `docs/PERFORMANCE.md` — performance tracing
- `docs/archive/` — internal extraction / planning notes from the monolith split

## Logs & settings

- Logs: `%LOCALAPPDATA%\CarouselCanvas\logs\app.log` (Windows) or `~/CarouselCanvas/logs/app.log` (macOS/Linux)
- Settings: `%LOCALAPPDATA%\CarouselCanvas\settings.json` (Windows) or `~/CarouselCanvas/settings.json` (macOS/Linux)
- Performance tracing: set `CAROUSEL_CANVAS_PERF=1` or pass `--perf` to the GUI entry point.

## License

MIT — see [LICENSE](LICENSE).
