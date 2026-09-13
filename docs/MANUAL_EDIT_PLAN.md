# Manual edit and mural preview

After Shuffle places a mural, the layout must stay put. Changing a hero or one slot photo must not replan underfill or reshuffle geometry. The preview must read as one wide strip you can zoom and drag around, with clear slice labels and honest density through slides 7–10.

Today every slot edit dirties the whole preview pipeline (decode → flatten → underfill plan → compose). Drag on a photo starts crop-pan and shows “Updating…”, while view navigation is only scrollbars. Selection uses a bright green stroke on the image. Fit zoom leaves the mural in the top-left of the scroll area. Hit-test maps overlay slots as if they lived in the scaled preview canvas, but `preview_overlay_slots` are full template coordinates — selection is wrong past the left of the strip. Preview underfill plans on a ~2200px-wide flatten with scaled tail bands; export plans at 10800×1350. Scene build always re-derives slots from `resolved_slots(layout_seed)`, so a lock that only stores the seed will re-jitter on the next compose or export.

## Locked design

Two modes.

**Auto** — Shuffle and New layout seed assign photos, resolve slots (jitter / polaroid / mosaic), plan underfill for borderless templates that define it, then compose. GUI Auto does not run token layout retry (preview already uses `layout_seed` as-is). After a successful placement, capture a `LayoutSnapshot` and enter Manual. App startup shuffle counts as a successful placement.

**Manual** — Compose and export from the snapshot only. No `resolved_slots`, no `effective_layout_seed`, no `plan_and_assign_mural_underfill`, no `pick_underfill_paths`. Unlock (confirm) discards the snapshot and returns to Auto.

`LayoutSnapshot` (core): `template_id`, `canvas_width`, `canvas_height`, `layout_seed` (procedural wood / polaroid `slot_seed` only), `card_edge`, `slots: Vec<StripSlotDef>` at full canvas size, `fill_required` bitmap matching that slot list, `underfill_boxes`, `underfill_paths`. Slot indices stay stable; z-order is `slots[i].z_index`, not list order.

Allowed in Manual: replace a required slot or hero path (texture only; underfill paths stay), flip, crop-pan, select, navigate the view, change border color (visual only). Phase 4 adds move and z-order on required slots. Forbidden until Unlock: Shuffle, New layout seed, template change, card-edge change that would enable or disable underfill, input-folder browse that would refill, clearing a required slot. Optional slots may clear. Undo/redo applies to fills (path, pan, flip) only, not to frozen geometry until Phase 4 checkpoints moves on the snapshot.

Underfill cards are not selectable and not movable. Moving a required slot in Phase 4 does not replan underfill; beige holes and overlap with frozen underfill are expected.

### Coordinate contract

All snapshot geometry is full template canvas pixels (10800×1350 for current strips). Preview and export apply `compose_scale` only when building a `Scene` for GPU. Underfill planning uses `template.canvas_width/height` and unscaled `carousel_tail_band_x_range`. Never multiply tail bounds by `compose_scale`.

Hit-test and selection draw map screen position through the displayed mural rect using `template.canvas_width` / `template.canvas_height` (or snapshot canvas size), not the scaled `Scene.canvas_width`. Slice footer sits below the texture rect; `StripView` offsets refer to the image, not the labels.

### Compose paths

Add `build_scene_from_snapshot` in core: frozen slots + fills + underfill boxes/paths + optional `compose_scale`. `build_scene_from_fills` remains for Auto placement only.

Auto underfill: required-slot decode and flatten at full canvas size (compositor already chunks at 8192). Planner `downscale_rgb` on that full-res buffer is fine. Display compose stays at `preview_compose_scale` (~2200px). Full-res flatten for planning runs on an export-style offscreen wgpu device on a worker (same as CLI export), not inside `eframe::App::update`. Scaled display compose stays on the eframe compositor.

Manual preview/export: decode dirty cards only (path, dest size, pan, flip, z). Recompose from snapshot. `StripExportParams.locked_layout: Option<LayoutSnapshot>` — when set, skip layout retry and underfill replan.

Procedural background re-renders from frozen `layout_seed` + template kind at compose scale.

Polaroid, mosaic, seamless, 10col, mural v1 still lock: freeze the resolved slot list (mosaic length may vary by seed). Templates with zero underfill store empty underfill vectors.

## Scope

In: egui preview navigation and chrome including hit-test coords; Auto/Manual lock; hero and slot replace without replan; Manual move and z-order of required slots; preview/export from snapshot; full-res underfill planning for Auto/preview parity on late slices.

Out: restoring the left thumb gallery; selecting or editing underfill cards; new templates; changing canvas or slice count; bit-identical Python pixels; CLI redesign (CLI stays one-shot Auto with layout retry). Re-planning underfill or re-jittering while locked.

## Phase 1 — Preview navigation and chrome

Files: `crates/app/src/gui/mod.rs`.

Center the mural when zoom is 1 (pad or offset so it is not top-left). Plain drag pans `scroll_offset` and never sets `preview_dirty`. Crop-pan becomes Alt-drag on a filled required slot; rebuild compose only on mouse-up. Keep Ctrl-drag swap until Phase 4. Soften selection chrome (corner marks or a thin light stroke, not a green frame on the photo). Slice numbers 1–10 in a footer under the image, outside pixels; faint vertical guides may stay. Fix hit-test and overlay draw to use full canvas size versus the displayed image rect.

Done when fit view shows the full strip centered; dragging pans with no “Updating…”; selecting a slot near the right of the mural hits the right card; labels sit under their slides.

Verify: GUI on an already-filled mural; fit; drag; select a card in a late slice; confirm footer labels.

## Phase 2 — Full-res underfill for Auto preview

Files: `crates/render/src/preview.rs`, `crates/render/src/compositor.rs`, `crates/render/src/strip_export.rs` (share flatten/plan helpers if that avoids a second path), `crates/core/src/underfill.rs` only if tail/planner inputs were wrong at full res.

Auto preview underfill: full-res required-slot decode + full-res flatten (offscreen worker) + unscaled tail band + `plan_and_assign_mural_underfill`. Display compose remains scaled. Return planned boxes and paths from the preview pipeline so Phase 3 can snapshot them. Polaroid/10col/mural v1 skip this path when underfill count is 0.

If a same-seed CLI export is already dense on slides 7–10 and preview is not, preview must match export. If both are sparse, fix shared planner inputs until the right quarter of a mural v2 borderless strip fills.

Done when mural v2 borderless preview from TEST IMAGES shows underfill in slices 7–10 in meaningful numbers, and a release export at a fixed seed is not sparse there either.

Verify: CLI `strip_mural_v2` seed 7 to `output/strip_latest`; GUI Auto placement on TEST IMAGES; look at slides 8–10.

## Phase 3 — Snapshot type and lock UI

Files: `crates/core/src/` (new snapshot type + `build_scene_from_snapshot` in `slot_fill.rs` / `scene.rs`; export `PlannedUnderfill` if needed), `crates/app/src/gui/mod.rs`, `crates/app/src/gui/slot_state.rs` only if fill undo must ignore geometry.

After Auto placement succeeds, store `LayoutSnapshot` and enter Manual. UI shows “Layout locked”. Shuffle, New layout, template, underfill-changing card edge, and input-folder refill are disabled or confirm Unlock first. Pick hero / Replace selected change only that fill path and recompose from the snapshot. Plumb underfill boxes and paths through the Auto preview/export result into the snapshot (do not re-pick paths). Startup after folder shuffle lands in Manual.

Done when changing the hero in Manual does not move other cards or change underfill photos; Unlock + Shuffle does; a polaroid or mosaic lock still shuffles only after Unlock.

Verify: mural Shuffle → Pick hero → rest unchanged; Unlock → New layout → geometry changes; polaroid Shuffle → replace one photo → other cards stay.

## Phase 4 — Export from lock, move, layers

Files: `crates/render/src/strip_export.rs`, `crates/render/src/preview.rs`, `crates/app/src/gui/mod.rs`.

Manual export sets `locked_layout` and skips retry/replan; output matches the on-screen lock. Ctrl-drag moves `snapshot.slots[i].{x,y}` in full canvas space (clamp so the card stays mostly on canvas). Swap is Ctrl+Shift-drag. Selected required slot: Bring forward / Send backward / To front / To back rewrite unique `z_index`. Recompose dirty cards only. Checkpoint snapshot geometry for undo of moves/layers. Plain drag still pans; Alt-drag still crop-pans.

Done when a user can change hero, nudge a mid-strip card, change its layer, and export without a replan; late-slice select still works; view drag does not move cards.

Verify: Manual — Pick hero, move one slot, send backward, Export strip; ten slices match preview; view drag does not move cards.

## Estimated duration

- Phase 1: 3–5 agent-hours
- Phase 2: 5–9 agent-hours
- Phase 3: 6–10 agent-hours
- Phase 4: 6–10 agent-hours
