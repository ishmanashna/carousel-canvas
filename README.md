# Carousel Canvas

Template-driven Instagram carousel murals: compose one wide master image and export 10 vertical 1080×1350 JPEG slices.

Standalone product — no dependency on the Image Stacker monolith or `image-stacker` repo.

## Requirements

- [Rust](https://rustup.rs/) (stable) with the **MSVC** toolchain on Windows (`rustup default stable-msvc`)
- Windows 10+ with Direct3D 12 (integrated GPU or WARP software adapter)

## Quick start

```text
cargo run -p app
```

Launches the GUI. Pick a photo folder, choose a template, preview the mural, export.

### Fixture photos (optional)

Keep a `TEST IMAGES/` folder at the repo root for local exports (gitignored). CI creates synthetic JPEGs when needed.

## CLI (strip export)

```text
cargo run -p app -- "path/to/photos" --strip --output output/strip_latest --strip-layout-seed 7
```

Default template is `strip_mural_v2`. Other templates need `--strip-template <id>` — see `TEMPLATE_IDS` in `crates/core/src/registry.rs`.

**Photo counts:** `strip_mural_v2` needs **35 unique** photos by default. If your folder is smaller, add `--strip-allow-repeats` (GUI: “Allow repeating photos”). `strip_10col` only needs 10.

Release build (faster export):

```text
cargo run -p app --release -- "path/to/photos" --strip --output output/strip_latest
```

## Templates

- `strip_mural_v2` (default)
- `strip_polaroid_table_v1`
- `strip_seamless_mosaic_v1`
- `strip_seamless_v1`
- `strip_mural_v1`
- `strip_10col`

## Tests

```text
cargo test -p core
cargo test -p render --lib
cargo test -p app
```

## Docs

- `docs/RUST_RESTACK_PLAN.md` — Rust rewrite plan and phase checklist
- `docs/SPEC_STRIP_MURAL_V2.md` — mural v2 layout spec (historical)
- `docs/PERFORMANCE.md` — performance notes from the Python era
- `docs/archive/` — internal extraction / planning notes from the monolith split

## Logs & settings

- Logs: `%LOCALAPPDATA%\CarouselCanvas\logs\app.log`
- Settings: `%LOCALAPPDATA%\CarouselCanvas\settings.json`

## License

MIT — see [LICENSE](LICENSE).
