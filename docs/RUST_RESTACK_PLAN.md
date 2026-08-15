# Rust restack

Rewrite Carousel Canvas as a Rust desktop tool: one scene of photo cards, rendered on the GPU, exported as ten Instagram slides. Python and Pillow go away when the new app can do the job.

The current prototype spends about 52s and 600MB on a mural v2 export from real album JPEGs. Almost half of that is pasting and rotating on a 10800x1350 CPU bitmap. Another third is decoding and resizing full-resolution photos. Beige-pocket underfill planning is another tenth. Writing the JPEGs is about one percent. The restack is built around those facts, not around a faster JPEG encoder.

Port behavior from the Python code, not from stale specs. Mural v2 repeat underfill is 9 layers and tail is 1 slice (`template_data.py`), not the older SPEC numbers.

## Locked architecture

Layout is data. A template is slots (box, rotation, z, fit, hero/rim flags) plus a placer and a fill policy. Assignment picks photos. Required-slot photos decode to the draw size of that slot (never open a 5000px source to fill a 400px slot). Underfill cards decode to their planner box size (often much larger than a slot; mural v2 boxes can approach ~2480px wide). Colors stay sRGB from decode through shader to JPEG.

The compositor is wgpu via eframe from the first drawing phase: one device, offscreen `Texture` for export, a window surface later for GUI. Do not build a hand-rolled wgpu stack in an early phase and throw it away when the GUI arrives. Dev machine is Windows 10 Direct3D 12. CI is `windows-latest` plus WARP so adapter and formats match home, not a Linux Lavapipe surprise. Headless export uses that same eframe/wgpu stack (invisible viewport / exit-after-N-frames pattern), not a second compositor.

There is no 10800x1350 CPU mural used as the product canvas, and no Pillow-style second compose for export. Allowed CPU work: JPEG/PNG decode, underfill occupancy planning (below), JPEG encode of finished tiles. Polaroid cards are GPU draw passes (photo, paper, grain, shadow, reflection), not CPU-baked full-card RGBA.

Underfill is not an abstract occupancy grid of slot boxes. Port `app/strip/underfill.py`. Contract:

1. After required slots are assigned and decoded, `render` draws those cards only and returns a full-canvas RGB flatten (`render_required_slots_flat_rgb`) — one GPU pass + readback, or an equivalent flat RGB the planner can own. Tokens are not enough here.
2. `core` keeps a full 10800x1350 RGB work buffer copied from that flatten. Each iteration downscales that work image (~620–820px) only to find beige anchors, maps the anchor back to full coords, places a box, then blacks out a padded region on the **full-res** work buffer before the next box. Same as Python.
3. Passes: base 10 + boost 5 (one function), repeat 9, tail 18 constrained to the last carousel slice (`background_tail_slice_count=1`). Tolerance 48; boost/repeat/tail bonuses and edge-vs-center anchors as in code.
4. Photo pool: port `pick_underfill_paths` (prefer files not in main slots, then cycle). Base pass assigns sequentially from that list; repeat picks with `Random(layout_seed ^ 0x5EED1EAF)`; tail with `^ 0x7A11BEEF`. Underfill RNG for path pick is `underfill_rng_from_layout_seed` (`layout_seed ^ 0x1B873F91`) on the **effective** seed after layout retry.
5. Borderless only; wedges/border skip underfill. Top gap-fill stays off (`gap_fill_max_layers=0`). GPU draws the planned extra cards behind the required slots; it does not re-invent hole finding.

Export order: assign → hero pin → token layout retry (if enabled) → pick underfill paths from `eff_seed` → decode required → required-slot flatten for planner → plan underfill → decode underfill boxes → final tile export.

Layout retry is a different system. Mural v2 + borderless tries 5 seeds at 0.25 scale with token-colored slots (no photo decode, no underfill) and keeps the best per-slice color variety score (`layout_retry.py`). Do not score retry with beige holes or with full GPU photo exports.

Tiles: each deliverable slice is one ortho render of the full scene with camera `[i*1080 - bleed, (i+1)*1080 + bleed]` x `[0-bleed, 1350+bleed]`, then center-crop to 1080x1350. `bleed_px` is the max of (rotated slot AABB padding, largest underfill box half-width) so straddling cards and wide tail boxes still draw. Seamless v1 has slots with negative x; those must appear in tile 0.

Wide master: today’s Python writes the full composed 10800x1350 canvas, then crops slices from it. Rust still always writes `*_wide.jpg` (uncapped), but builds it as a horizontal concat of the same ten cropped tile renders used for the slices — not a second full-width GPU pass, and not claiming the Python path already stitched tiles.

Visual parity with today’s templates, not bit-identical pixels. Unique-photo rule applies to required image slots only (mural v2 needs 35, polaroid 20, 10col 10, mural v1 8, seamless v1 9, mosaic 9–12 depending on seed). Underfill may reuse files.

One binary: GUI if launched with no folder/export flags, CLI export otherwise. Keep today’s CLI flag names (`--strip`, `--strip-template`, `--strip-layout-seed` default 0, `--strip-allow-repeats`, `--strip-dumb-shuffle`, `--strip-no-layout-retry`, `--strip-hero-image`, `--strip-card-edge`, `--output`, `--color`). Hero path: absolute, folder-relative, or unique filename substring; ambiguous match errors. Settings and logs under `%LOCALAPPDATA%\CarouselCanvas`. Tracing for decode/GPU from phase 2.

Threading: worker threads may scan folders, assign, plan, and decode into CPU buffers. The eframe loop, wgpu device/queue, texture upload, draw, and export stay on the main thread. Cross-thread channels pass scene IR and decoded RGBA, never wgpu handles.

Workspace:

- `crates/core` — templates, assignment, jitter, polaroid scatter, mosaic builder, underfill planner, token layout retry, scene IR
- `crates/render` — decode, eframe/wgpu, required-slot flatten readback, tile cameras, 8MB JPEG encode, assets
- `crates/app` — CLI + GUI

The compositor API takes `card_edge` from phase 3 even if only borderless is implemented until the edges phase. JPEG encode in `render` ports the 8MB quality search (`save_optimized`). Decode cache key: path, dest size, pan, trim, band, flip. Stream or evict underfill textures so the RAM bar stays honest when many large boxes are decoded.

Python under `app/` remains until the last phase, then it is deleted in that same phase.

## Scope

In: all six templates; smart assignment and allow-repeats; hero pin; layout seed and token retry; card edges; unique-source rule; ten 1080x1350 JPEGs ≤ 8MB plus wide stitch; GUI with live preview and the current slot-edit gestures.

Out: Image Stacker collages, paid GPU clouds, Electron/TypeScript, keeping Pillow as a fallback renderer, bit-exact hashes against old Python JPEGs, inventing a new underfill look.

## Performance bar

Same class of 83-photo album and mural v2: export well under 10s, peak RAM well under 600MB including underfill box decodes (cache/evict as needed), preview without decoding every album file at once. Layout retry stays on tokens. Underfill needs one required-slot flatten readback, then CPU planning on the work buffer — not ten full-res readbacks for hole finding.

## Phase 1 — Workspace and scene IR

Cargo workspace, Windows MSVC, crates as above. Port all six template records from `template_data.py` (ids, 10800x1350, 10x 1080x1350, overlap 0, slot boxes, fit flags, fill_required masks). Mural v2: 35 slots, hero 34, rims 32–33, underfill 10+5+9+18, tail slice count 1. Polaroid: 24 slot defs, 20 required + 4 optional. Seamless v1: include negative-x slots. Mosaic: port `build_seamless_mosaic_v1_slots` (9–12 slots per seed, no optional fills). Folder scan of jpg/png. Smart assign. Scene IR: cards plus background. No GPU.

Done when `cargo test -p core` passes with all six template ids registered at correct slot counts, and a mural v2 scene prints 35 slots with hero set, without decoding pixels.

Verify: uniqueness fails when files < required slots for that template; mosaic widths sum to 10800; polaroid fill_required is 20 true + 4 false; seamless v1 has at least one negative x.

## Phase 2 — Decode to destination size

Open JPEG/PNG, EXIF orientation, cover/contain, pan, left-trim, height-first cover, 2H center band. Decode target is the destination box (slot or underfill box), never a full-res intermediate kept around. Rayon over independent decodes. Cache as above. Log decode sizes.

Done when filling 35 mural slot textures from `TEST IMAGES` is a small fraction of the old ~10s decode+resize phase.

Verify: cover math unit tests on tiny images; one real TEST IMAGES file decoded to 200px.

## Phase 3 — wgpu compositor

eframe + offscreen export texture from day one, including a headless/invisible-viewport export path that CI will reuse. Upload textures, draw back-to-front, rotation around slot center, solid background, tile camera + center crop, `encode_slice_jpeg` with 8MB cap. Start with `strip_10col`. Adapter: DX12, fall back to WARP. `card_edge` on the API, only borderless implemented.

Done when CLI can write ten 1080x1350 JPEGs from a folder with `--strip --strip-template strip_10col` (cover, one photo per slide) without a visible window.

Verify: files on disk; runs on integrated GPU or WARP; same binary path CI will call.

## Phase 4 — Mural v2, underfill, token retry

Jitter placer: flagship first, rims under hero, hero on top in slice 1, moderate tilt. Export order and underfill contract from Locked architecture. `render_required_slots_flat_rgb` exists. Bleed formula so tail boxes and rotations straddle slices. Token retry 5 seeds at 0.25, variety score; skip if `--strip-no-layout-retry` or not mural v2 borderless. No top gap-fill.

Done when a mural v2 borderless export from `TEST IMAGES` reads as a full overlapping collage, hero dominating slide 1, last slice packed, almost no empty beige.

Verify: `cargo test` smoke — slices are 1080x1350; underfill ran (extra cards present when borderless). Human look at slides 1, 5, and 10. Do not use `gap_fill_stop_ratio` 0.004 as the underfill pass/fail (that knob is for disabled top gap-fill only).

## Phase 5 — Polaroid placer and wood

Port `resolve_organic_polaroid_slots` (hero in slice 0, per-column anchors, wild bleed, upside-down cards, slice coverage nudge, separation, tilt pools). Twenty required slots plus four optional. Wood: procedural brick-stitch from `procedural_backgrounds.py`; if a wood PNG is added under crate assets, use it, but git may have none — do not assume `app/strip/assets/polaroid_table_wood.png` exists.

Done when polaroid slot boxes look like a scattered table with a stable first-slice hero, on wood, photos still simple cover quads.

Verify: hero stays in slice 0; cards are not a regular grid.

## Phase 6 — Polaroid card materials

GPU passes matching the visual recipe in `polaroid_card.py` and the polaroid branch of `composer.py`: white chin, inner feather, paper grain, tint, contact shadow (offset + blur), weak table reflection. No CPU supersample of the whole card; photo decode stays at inner-window size.

Done when `strip_polaroid_table_v1` export feels like instant film on a table, not rectangles on brown.

Verify: chin readable, inner photo cover-cropped, shadows sit on wood.

## Phase 7 — Other templates and edges

Seamless mosaic via the seeded builder (lead V with left trim, col 2 landscape H/2H/3H, 2H x 1200 stack, tail H H, jitter placer ignored). Seamless v1 (including negative x). Mural v1. One implementation of wedges and border used for all templates. Underfill still borderless-only.

Done when every CLI template id writes ten valid slices.

Verify: one export per id into `output/strip_latest`.

## Phase 8 — CLI product

Wire the flag set in Locked architecture. Default template mural v2. Unique-source error names the actual slot need for that template and seed. Always write `*_wide.jpg` as the tile stitch (uncapped). Hero resolution as today. Export order as locked (retry before underfill pick).

Done when `carousel-canvas --strip "TEST IMAGES" --strip-layout-seed 7` produces a mural, and a too-small folder fails without `--strip-allow-repeats`.

Verify: `--help` ASCII-safe on Windows cp1252; 8-photo mural v2 fails; 10col with 10 photos succeeds.

## Phase 9 — GUI

Same eframe surface. Folder, template, card edge, allow-repeats, smart shuffle, border color. Layout seed is not a typed field: Shuffle / New layout randomizes it (not saved in settings). Preview from the renderer. Gestures to keep: drag thumb to slot, pan on slot, double-click flip, Ctrl+drag swap, right-click clear, undo/redo (50), hero-only replace, Shift+wheel horizontal scroll, pick output folder, open output folder. Export always runs token retry for mural v2 borderless (no GUI toggle for `--strip-no-layout-retry`). Workers decode and plan; main thread uploads and draws.

Done when the Python Tk app is unnecessary for the full loop: load album, see mural, swap hero, export.

Verify: Windows 10 session; UI stays alive while preview rebuilds.

## Phase 10 — Cut over

Delete the Python product: `app/`, `run_gui.py`, `script.py`, `requirements.txt`, `requirements-dev.txt`, `build_windows.ps1`, `tests/test_*.py`, `scripts/*.py` (including profile and baseline hash generators), `docs/BASELINE_HASHES.txt` as a Python golden. Replace `.github/workflows/test.yml` with `cargo test` plus the Phase 3 headless `strip_10col` export on `windows-latest` (WARP / DX12). README is cargo/rustup MSVC only. Cursor rules point at cargo. Polaroid wood, if any, lives in crate assets.

Done when the repo has no Python product path, `cargo run -p app` launches the GUI, CI is green without Pillow.

Verify: README clone-fresh on Windows 10; `TEST IMAGES` still gitignored.

## Estimated duration

- Phase 1 — Workspace and scene IR: 4–8 agent-hours
- Phase 2 — Decode to destination size: 6–10 agent-hours
- Phase 3 — wgpu compositor: 10–16 agent-hours
- Phase 4 — Mural v2, underfill, token retry: 14–20 agent-hours
- Phase 5 — Polaroid placer and wood: 8–12 agent-hours
- Phase 6 — Polaroid card materials: 10–16 agent-hours
- Phase 7 — Other templates and edges: 8–14 agent-hours
- Phase 8 — CLI product: 6–10 agent-hours
- Phase 9 — GUI: 12–20 agent-hours
- Phase 10 — Cut over: 4–8 agent-hours
