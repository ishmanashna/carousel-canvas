# Out of frame implementation plan

New strip template `strip_out_of_frame_v1`. Own placer. Own vision pass. Not a skin on mural v2 or polaroid.

Canvas 10800×1350, 10 slices of 1080×1350, `overlap_px` 0, all underfill and gap-fill counts 0, `layout_placer` `OutOfFrame`. Always `CardEdge::Borderless` for this template (GUI and CLI). Wedges/border would paint beige into the overlaps and kill paper-on-paper.

Do not register the template in `TEMPLATE_IDS` until Phase 4. Registering early makes `pick_smart_fills` and preview call seed-only `resolved_slots` and produce an empty mural.

## Locked product rules

- Two roles only: paper (opaque full photo) and figure (subject cutout). One photo, one role, or unused.
- Papers keep sharp edges and a drop shadow. They overlap neighbors. They do not feather, dissolve, or Poisson-blend.
- Figures use empty space. Occupied mask on a paper is forbidden. After a figure is accepted, its silhouette is burned into the same occupancy buffer so the next figure cannot sit on that person. People on people is a failed layout: drop the later figure.
- A figure must be a whole subject. Head chopped in the source file → not a figure. Slide line through face or mid-torso → illegal, move or drop. Bottom-only crop in the source is allowed if that cut is hidden in empty paper, not on a face.
- Slice 1: one dominant paper and one figure breaking its edge.
- After the first successful Auto, `LayoutSnapshot` locks the **already resolved** slots. It must not call `template.resolved_slots` for this template.

## Locked technical choices

- Model file: rembg’s `isnet-general-use.onnx` (DIS dichotomous segmentation, not a second detector). URL: `https://github.com/danielgatis/rembg/releases/download/v0.0.0/isnet-general-use.onnx`. Store only at `%LOCALAPPDATA%/CarouselCanvas/models/isnet-general-use.onnx` (same folder as existing GUI settings). Never a path inside the git repo.
- DIS README Apache-2.0 covers code and metrics; weight terms for commercial shipping are not a clean grant (DIS issue 150). Do not claim “Apache weights, done.” No Ultralytics YOLO (AGPL). No Python rembg runtime. No paid HTTP cutout.
- Inference: crate `ort` with `download-binaries` and `directml`. Try DirectML with sequential execution and memory pattern off, then CPU. One session. Log which EP actually attached. First session may spend several seconds compiling the graph; that is warmup, not per-photo time.
- New crate `crates/vision`. Types live in `crates/core/src/analysis.rs`. Vision depends on core. Core does not depend on vision or ONNX. Do not duplicate the structs.
- Mask cache: `%LOCALAPPDATA%/CarouselCanvas/masks/<hex>.png`, blake2s of source bytes. Shuffle with a warm `Vec<PhotoAnalysis>` in memory does not call `Session::run`.
- Preprocess like rembg ISNet: apply EXIF orientation, resize 1024×1024, divide by max pixel, mean 0.5 std 1.0, NCHW f32. Saliency upsampled to oriented source size, `u8` alpha.
- Color decontamination on figure fringe when decoding a cutout, not as a leftover pass.
- `StripSlotDef` and `SceneCard` gain `cutout: bool` and `mask_path: Option<PathBuf>`.
- Decode today flattens alpha in `load_rgb_at_most`. Borderless GPU already uses `ALPHA_BLENDING`, but that is useless until decode keeps alpha and multiplies the mask. `DecodeOptions` gains `preserve_alpha` and `mask_path`. `CacheKey` includes those. Cutout: skip flatten, cover-fit the photo into dest, multiply by resized mask, decontaminate, upload. Dest is the card rect; mask (from full source) is what punches the background out.
- Shadow pass for every out-of-frame card: same textured quad, offset about (12, 16) px, `vec4(0,0,0, tex.a * 0.45)`, drawn before the card. Not the polaroid shader.
- Do not call `flatten_and_plan_underfill` (underfill counts stay 0).

## Analysis types (`crates/core/src/analysis.rs`)

```rust
pub struct OccupancyMap {
    pub width: u32,
    pub height: u32,
    pub occupied: Vec<u8>, // row-major, 0 empty .. 255 occupied, ~80 px on the long side
}

pub enum PhotoRole { Paper, Figure, Skip }

pub struct PhotoAnalysis {
    pub path: PathBuf,
    pub role: PhotoRole,
    pub occupancy: OccupancyMap,
    pub subject_bbox: [i32; 4], // x,y,w,h in oriented source pixels
    pub mask_png: PathBuf,
    pub complete_subject: bool,
}

pub fn complete_subject_from_mask(mask: &image::GrayImage) -> bool;
pub fn role_from_mask(mask: &image::GrayImage) -> PhotoRole;
```

`complete_subject_from_mask`: largest blob after threshold 128 and 2 px dilate; single dominant blob; dilated blob does not intersect the top image edge by more than 3 px of height; not both left and right edges. Bottom edge only is allowed.

`role_from_mask`: Skip is not decided here (decode failure). Paper if tiny blob, huge blob, two-plus large blobs, or not complete. Figure if complete and the blob is a moderate share of the frame (starting bands: about 8–50% area). Width-only rejects are a last resort for panoramic blobs, not for tall portraits. Tune on TEST IMAGES; do not treat the percents as sacred.

Vision crate:

```rust
pub fn ensure_model() -> Result<PathBuf>;
pub fn analyze_path(path: &Path, session: &mut Session) -> Result<PhotoAnalysis>;
pub fn analyze_folder(paths: &[PathBuf], cancel: &AtomicBool, progress: impl Fn(usize, usize)) -> Result<Vec<PhotoAnalysis>>;
```

Cache hit: read mask png, recompute role, no ONNX.

## Layout (`crates/core/src/layout_out_of_frame.rs`)

```rust
pub fn resolve_out_of_frame_slots(
    analyses: &[PhotoAnalysis],
    canvas_w: i32,
    canvas_h: i32,
    slice_w: i32,
    seed: i64,
) -> Vec<StripSlotDef>;
```

`StripTemplate::resolved_slots` stays seed-only. For `OutOfFrame` it returns `self.slots.clone()` (empty) and is not the placement API. Every real caller for this template uses `resolve_out_of_frame_slots` then `build_scene_from_resolved`.

Algorithm:

1. Paper pool and figure pool. Promote to at least 6 papers if the folder is thin. Target 8–10 papers, cap figures at 16.
2. Papers: dest height 1180–1350, width 1200–2000, 18–28% x overlap, y ±40, rotation ±2.2°. First paper covers slice 1. `z_index` 0..n. `cutout` false. `mask_path` None. `fit` Cover.
3. Canvas occupancy at 1/8 resolution from each paper’s mapped occupancy (cover), dilated 2 cells.
4. Figures in shuffled order. Scales: 8–12 steps, subject height 0.45–0.95 of canvas. Positions: 24 px steps in full canvas (3 cells at 1/8). Score every legal pose; pick the best, not the first. Reject if mean occupancy under the subject bbox > 0.08. Reject if vertical lines `x = k * 1080` (k=1..9) hit the inner 60% width and inner 70% height. Bottom 18% of the bbox must sit inside a paper rect **and** mean occupancy in that band < 0.08 (hide the cut in empty paper, not on a face). Prefer overflow of top or side past that paper’s edge. First figure gets a large score bonus for breaking paper 1 in slice 1. `cutout` true, `mask_path` set, `z_index` 100+i, dest = subject-aspect box.
5. After each accepted figure, stamp its bbox (or silhouette if cheap) into the occupancy buffer so the next figure cannot land on it. Figure–figure overlap above ~10% of either bbox fails.
6. Return slots. Flagship index is the first figure slot if any, else first paper. That index is stored on the snapshot, not mutated on the static `StripTemplate` as the only source of truth.

`pick_out_of_frame_fills(analyses, slots, seed)` maps paper-role paths onto paper slots and figure-role paths onto figure slots, unique files first.

`max_strip_slots()` must include the placer cap (10 papers + 16 figures). `validate_strip_unique_sources` for this id: at least one photo, not “must fill 26.”

## Snapshot and scene

`LayoutSnapshot` gains `flagship_slot_index: Option<usize>`.

```rust
LayoutSnapshot::capture_resolved(
    template, layout_seed, card_edge,
    slots, fill_required, flagship_slot_index,
    underfill_boxes, underfill_paths,
)
```

`CarouselApp::capture_layout_snapshot` passes `built.overlay_slots` (or the resolved list used to build preview), not `template.resolved_slots`. GUI `flagship_slot()` reads the snapshot when locked.

`build_scene_from_resolved(template, slots, fills, …)` does not call `resolved_slots`. `layout_retry.rs` `resolve_slots_for_layout` skips OutOfFrame (no token retry for this template).

`PreviewParams` and `StripExportParams` carry `resolved_slots: Vec<StripSlotDef>` and `fill_required: Vec<bool>` for this template (computed once at GUI/CLI entry after analysis).

## GUI / CLI

- `TEMPLATE_OPTIONS` label: `Out of frame - papers + cutout figures`.
- `CarouselApp` holds `photo_analyses: Option<Vec<PhotoAnalysis>>` and an analysis generation/cancel pair. Analyze on folder change or switching onto this template. Shuffle / new seed with analyses present: placer only. Hero replace while locked: no analysis, no re-place.
- Progress: `Reading photos n/m`. Model download has its own status line. Failures show an error; do not freeze the UI thread.
- CLI `--strip --strip-template strip_out_of_frame_v1` analyzes (cache), places, exports to `output/strip_latest`. Force borderless.

## Time (honest)

| Moment | Expect |
| --- | --- |
| First model download | ~176 MB once, plus `ort` native binaries on first build |
| First DirectML session | extra seconds of graph compile |
| First folder, GPU EP | roughly 30–90 s for ~40 photos |
| First folder, CPU only | roughly 1–4 min for ~40; ~80 photos can be several minutes |
| Shuffle with analyses in memory | placer + preview, like today |
| Same files later | mask png reads, seconds |

## Files

Create:

- `crates/core/src/analysis.rs`
- `crates/core/src/layout_out_of_frame.rs`
- `crates/core/tests/out_of_frame.rs`
- `crates/vision/Cargo.toml`
- `crates/vision/src/lib.rs`

Modify:

- `Cargo.toml` workspace members
- `crates/core/src/lib.rs`
- `crates/core/src/template.rs` (`LayoutPlacer::OutOfFrame`, factory; `resolved_slots` does not place)
- `crates/core/src/registry.rs` (Phase 4 only)
- `crates/core/src/slot.rs`, `scene.rs`, `slot_fill.rs` (`cutout`, `mask_path`, `capture_resolved`, `flagship_slot_index`)
- `crates/core/src/assign.rs` (`pick_out_of_frame_fills`, unique-source rule)
- `crates/core/src/layout_retry.rs`
- `crates/core/src/hero_pin.rs` (flagship from snapshot when present)
- `crates/core/tests/phase1.rs` (Phase 4)
- `crates/render/src/decode.rs`, `cache.rs`, `lib.rs` (`DecodeOptions`)
- `crates/render/src/card_edge.rs`, `compositor.rs`
- `crates/render/src/preview.rs`, `strip_export.rs`, `export.rs`
- `crates/app/Cargo.toml`, `src/gui/mod.rs`, `src/gui/slot_state.rs`
- `crates/app/src/main.rs` if needed
- `.gitignore` (`/target_*/`, `*.onnx`, `models/`, `masks/`)
- `README.md` (Phase 6, one template line)

## Out of scope

Mural v2 changes. Polaroid changes. Underfill for this template. Soft paper fades. SAM. BiRefNet. Stickers, tape, type. Canvas size changes. A second person-detection model. Token layout retry.

## Phase 1 — Types and vision crate

Scope: `analysis.rs` with `role_from_mask` / `complete_subject_from_mask` tested on tiny PNGs (center blob, top-touch blob, bottom-only blob, two blobs). `crates/vision` download, session, cache, `analyze_path`. Integration test skips if the ONNX file is absent.

Done when: `cargo test -p core --lib analysis` (or the module tests) and `cargo test -p vision` pass. Second analyze of the same file does not need a live session when the mask png exists.

Verify: chopped-head is not Figure; bottom-only can be Figure; two blobs are Paper.

## Phase 2 — Decode alpha, mask, shadow

Scope: `DecodeOptions.preserve_alpha` + `mask_path`. Cache key distinguishes cutout. `prepare_render_cards` keeps alpha for `cutout`. Compositor shadow then card. Decontaminate on cutout decode. Force-borderless is a later GUI concern; render must not flatten cutout cards even if someone passes wedges.

Done when: render test — two overlapping quads, top has a hole of alpha 0, bottom shows through. Second test — shadow offset darkens pixels beside the card. `decode_card_rgba` with a fixture mask produces non-255 alpha.

Verify: `cargo test -p render --lib`. Polaroid tests still pass.

Can run beside Phase 1.

## Phase 3 — Placer only

Scope: `layout_out_of_frame.rs` and `crates/core/tests/out_of_frame.rs` with fake `PhotoAnalysis` values. No `TEMPLATE_IDS` change.

Tests: figure dest is not centered on a paper occupancy peak; figure bottom band is in empty paper; no inner-band split by x=1080, 2160, …; papers overlap on x; second figure cannot occupy the first figure’s bbox; a pose that only hides feet on a high-occupancy face is rejected.

Done when: `cargo test -p core --test out_of_frame` passes.

Can run beside Phase 1 and 2.

## Phase 4 — Pipeline and registry

Scope: template factory, then register. `pick_out_of_frame_fills`. `build_scene_from_resolved`. `LayoutSnapshot::capture_resolved` + `flagship_slot_index`. `PreviewParams` / `StripExportParams` carry resolved slots. CLI analyze → place → export. `max_strip_slots` includes the cap. `validate_strip_unique_sources` does not demand 26 photos. `layout_retry` ignores this placer. Hero pin uses snapshot flagship when locked.

Done when: `cargo test -p core --test phase1` sees seven templates, and `cargo run -p app --release -- "TEST IMAGES" --strip --strip-template strip_out_of_frame_v1 --output output/strip_latest --strip-layout-seed 7` writes `carousel_*_wide.jpg` and 10 slices (any JPG folder if TEST IMAGES is missing). Few photos still export.

Verify: wide jpg is large papers, not 35 mural tiles; some people have the paper showing around them; lock-style snapshot in a core test keeps `cutout` flags without calling `resolved_slots`.

Depends on 1–3.

## Phase 5 — GUI (split; one subagent each)

`gui/mod.rs` is already ~1700 lines. Do not land combobox + worker + placer + lock in one agent.

### 5a — SlotState dynamic length

`crates/app/src/gui/slot_state.rs`: `resize_to`, `len`, `set_assignments_from_paths` resizes to `paths.len()`. Unit tests for shrink/grow/undo length. No combobox, no ONNX.

Brief: `.superpowers/sdd/phase-5a-brief.md`

### 5b — Analysis worker

New `crates/app/src/gui/analysis.rs`. `photo_analyses` + cancel/generation. Analyze on folder change or switching onto this placer. Status `Reading photos n/m` and `Downloading model…`. Failures do not freeze the UI thread. `refill_from_folder` must not call smart/dumb fills for OutOfFrame (empty mural). Do not place yet.

Brief: `.superpowers/sdd/phase-5b-brief.md`

### 5c — Combobox, borderless, shuffle + preview

`TEMPLATE_OPTIONS` label `Out of frame - papers + cutout figures`. Force `CardEdge::Borderless`. Store `placed_slots` / `placed_fill_required` so unlocked `effective_num_slots` is not empty. Shuffle and New layout seed: `resolve_out_of_frame_slots` + `pick_out_of_frame_fills` with in-memory analyses (no ONNX). `PreviewParams.resolved_slots` set. Resize `SlotState` to the placed count.

Brief: `.superpowers/sdd/phase-5c-brief.md`

### 5d — Lock, flagship, hero, GUI export

`capture_layout_snapshot` uses `capture_resolved` + overlay slots (never seed-only `resolved_slots`). `flagship_slot()` reads the snapshot when locked. Hero / Replace do not re-analyze or re-place. GUI export passes locked snapshot / resolved slots; unique-source is ≥1 photo.

Done when: picking the template on a folder shows progress then a preview; second shuffle does not download or run ONNX; lock keeps papers still when the hero changes; preview stays off the UI thread.

Depends on 4, then 5a→5d in order.

## Phase 6 — Eyes on the strip (split)

### 6a — README

One template line + ≥1 photo / first-run ISNet note. No export.

Brief: `.superpowers/sdd/phase-6a-brief.md`

### 6b — Visual audit (no code)

Release CLI export to `output/strip_latest` with `--strip-template strip_out_of_frame_v1`. Score papers vs mural tiles, hard edges, cutouts in empty space, slice 1 hook, slide lines vs faces, feet hidden in paper. Write fail notes. Do not patch the placer here.

Brief: `.superpowers/sdd/phase-6b-brief.md`

### 6c — Placer fixes (only if 6b failed)

Tighten `out_of_frame` tests, fix `layout_out_of_frame.rs`, re-export seed 7. Skip entirely if 6b passed.

Brief: `.superpowers/sdd/phase-6c-brief.md`

Depends on 5d for a GUI path; 6b can run from Phase 4 CLI.

## Estimated duration

- Phase 1: 5–8 agent-hours
- Phase 2: 5–7 agent-hours
- Phase 3: 6–9 agent-hours
- Phase 4: 6–9 agent-hours
- Phase 5a: 1–2 agent-hours (SlotState)
- Phase 5b: 2–3 agent-hours (analysis worker)
- Phase 5c: 2–3 agent-hours (combobox + shuffle/preview)
- Phase 5d: 1–2 agent-hours (lock/hero/export)
- Phase 6a: <1 agent-hour (README)
- Phase 6b: 1–2 agent-hours (export + eyes)
- Phase 6c: 0–4 agent-hours (placer fixes only if 6b fails)
