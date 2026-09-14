# Strip playtest adjustments

After a layout lands, the GUI freezes it so photo swaps do not reshuffle. That freeze also greys out template, shuffle, and folder browse, so mural v2 feels stuck and there is no obvious way to try another arrangement. The mural board barely reaches slides 9–10, leftover folder photos sit unused while those slides get a few huge underfill patches, beige still shows, cover-crop plus cards hanging off the canvas chops faces, and each carousel slice lands around 400KB because it is only 1080×1350.

## Locked design

Lock stays for editing. Shuffle, template change, and folder change do a new layout with no confirm dialog. After the new placement succeeds, lock again. Hero, replace, crop-pan, and move still do not reshuffle.

Relayout always clears both `layout_locked` and `layout_snapshot` before assigning photos or starting analysis. A helper `begin_relayout` (or equivalent) is the only way Shuffle, New layout seed, template, and Browse leave the freeze. Do not leave a stale snapshot while `layout_locked` is false.

**Shuffle** (sidebar and a control next to Export) is always clickable. It calls `begin_relayout`, reshuffles photo assignments and layout seed, runs full Auto preview, then captures a new snapshot.

**New layout seed** is always clickable. It calls `begin_relayout`, keeps the current photo files, only changes `layout_seed`. Mural/polaroid/mosaic dirty preview; out of frame re-places with existing analyses. It does not call `refill_from_folder(true)`.

**Template** combobox is always enabled. `begin_relayout` runs before leave/enter out-of-frame and before `refill_from_folder`, so analysis is allowed to start. Changing template refills, places, locks.

**Browse folder** opens the picker immediately. A real choice sets the path and reloads. Cancel leaves the current lock. Drop `pending_unlock_browse`. Unlock layout stays only as an explicit discard.

**Card edge** stays disabled while locked. Live settings are ignored by locked preview/export, which read `snapshot.card_edge`. Enabling the combobox without a full replan would be a silent no-op.

Export is disabled while a relayout preview is in flight so it cannot write the old geometry with new fills.

Mural v2 required board must occupy the full 10800px width, including slices 9 and 10 (x 8640–10800). Today `mural_v2_main_board` last cards end at x+w ≈ 9192 (two rects touch slice 9, none touch slice 10). Hero/rims (z 85–90, indices 32–34) stay in slice 1. Jitter only nudges those boxes (`horizontal_bleed` 520) and cannot invent a right column. Tail underfill currently starts at x 9720, so x 9192–9720 has neither collage nor tail pass.

Leftover files after the static board go onto the mural as extra collage tiles (same medium size and slight tilt, z in 0–84, below the hero, above underfill). They are real `StripSlotDef` entries on the snapshot, not gap-fill patches and not underfill. Cap extras at `min(leftover, 40)`. Stop earlier if beige is gone. Underfill still sits behind and only fills holes the collage did not cover.

Static board and extras both compose and flatten (so underfill sees them). Uniqueness when repeats are off is the static board only. Extras are leftover unused files, so they do not raise the minimum unique count. If the folder has fewer files than the static board and repeats are off, fill that many slots, leave the rest empty (`fill_required` false), and spread the filled cards across the full width. Do not cyclic-expand to a full board of repeats unless the user allowed repeats.

One placement result feeds preview, lock, and export: `slots`, `fill_required`, and `fills` the same length. Mural lock uses `LayoutSnapshot::capture_resolved` with that list, not `capture` which re-derives from the template and would drop extras. Raise `max_strip_slots` / `SlotState` to the static board plus 40 extras.

Underfill tail band is the last two slices (`background_tail_slice_count` 2 → x 8640–10800). Keep planning while beige remains, including the bottom of slice 10. Do not `break` tail/repeat loops on the first missed anchor. Flatten for planning uses the same vertical bleed as tile export so cards at y≈−70 are not treated as beige.

Cover-crop keeps the subject in `slot ∩ canvas`. No ONNX requirement for mural. If `pan_x` and `pan_y` are still 0, decode uses a portrait face-band fallback (upper-middle of a tall source) mapped into the on-canvas remainder. User pan wins. Jitter pulls a card back if most of it sits off the canvas. Out-of-frame cutout rules do not apply to mural v2.

Each **slice** JPEG is capped at 8MB. That cap is per slice file, not the wide mural. Today a 1080×1350 slice at quality 98 is about 400KB because there are not enough pixels — the 8MB ceiling never engages. Quality comes from more pixels, not padding and not a higher byte cap.

**Export strip** (default) composes at `export_scale` 2.0: slices 2160×2700, wide 21600×2700, JPEG quality 98. Encode each slice at q98; lower quality only if that slice would exceed 8MB. Wide master is q98 with no byte cap (do not run it through the slice cap or the old q95/q65 ladder). **Export fast** is 1080×1350 for speed. Preview stays downscaled. Name the field `export_scale`; do not reuse preview `compose_scale`. CLI default matches GUI default (2×); `--strip-fast` does 1×. Plan underfill at 1× and scale boxes into the export scene.

Chroma stays 4:2:0 with the current encoder. A 4:4:4 backend is out of this plan.

## Scope

In: lock UX; mural width coverage; leftover photos as extra snapshot slots; underfill tail and leftover beige; subject-aware cover and canvas clip; 2× export as default with 8MB per-slice ceiling.

Out: new templates; changing slice count; selecting or dragging underfill cards; Instagram upload automation; padding files toward 8MB; applying 8MB to the wide mural; removing layout lock for hero/replace/move; ONNX on mural place; enabling card-edge while locked without a full replan.

## Order

Phase 1 and Phase 5 may run together. Then 2a, then 2b, then 3, then 4. Phase 3 needs extras in the flatten. Phase 4 waits so `layout_jitter.rs` is not edited by two people at once. Phase 5 must not touch `flatten_and_plan_underfill`.

---

### Phase 1 — Layout actions always work

Files: `crates/app/src/gui/mod.rs`, `crates/app/src/gui/analysis.rs`, `crates/app/src/gui/slot_state.rs` (undo clear on reshuffle only).

Add `begin_relayout` that clears `layout_locked`, `layout_snapshot`, and pending geometry. Status text depends on the action, not “Unlock layout to shuffle.”

Remove `layout_locked` early-returns from `reload_folder` and `refill_from_folder`; they call `begin_relayout` when locked. Template change calls `begin_relayout` before `enter_out_of_frame_if_needed` so `start_analysis` is not skipped (`analysis.rs` still returns when locked). Browse opens the folder picker with no modal. New layout seed uses `begin_relayout` then the existing seed-only path.

Shuffle button next to Export, same as Strip options. Keep card-edge disabled while locked. Keep Unlock layout for an explicit discard. Delete Browse’s `pending_unlock_browse` path. Disable Export while `preview_building` after a relayout until `apply_preview_result` captures the new snapshot.

Done when: locked mural can switch to polaroid or out of frame without a dialog and out of frame actually analyzes; Shuffle produces a new arrangement; New layout seed keeps the same files; Browse to another folder loads it; Pick hero still does not move other cards; Export after Shuffle is the new layout.

Verify: GUI locked mural, template switch including out of frame, Shuffle, New layout seed, Browse, Pick hero, Export.

---

### Phase 2a — Right-hand collage column

Files: `crates/core/src/template.rs` (`mural_v2_main_board`), `crates/core/src/layout_jitter.rs` (slice 9–10 coverage in the existing score, not a second scorer), `crates/core/tests/phase1.rs`.

Add a column of main-board cards in the same size/tilt band as slots 28–31, x near 8400–9800, so rects cover 9192–9720 and reach ~10800. Append them in the main board (indices before the hero/rims). Do not move `layout_flagship_slot_index`. Update `num_slots` / `strip_image_slot_count` expectations in `phase1.rs` and any render bench that asserts 35 cards. Jitter scoring penalizes a layout whose static cards cover less than about 55% of each of the last two slice rectangles.

Done when: resolved mural v2 slots (no extras) intersect both slice 9 and slice 10 in meaningful area; hero still sits in slice 1.

Verify: `cargo test -p core --test phase1`. CLI `strip_mural_v2` seed 7 to `output/strip_latest`; look at slides 9–10.

---

### Phase 2b — Leftover photos as extra snapshot tiles

Files: `crates/core/src/assign.rs`, `crates/core/src/slot_fill.rs`, `crates/core/src/registry.rs` (`max_strip_slots`), `crates/app/src/gui/mod.rs`, `crates/app/src/gui/slot_state.rs`, `crates/core/tests/phase8.rs` if uniqueness copy still says 35 after 2a.

After static assignment, leftover unused files become extra `StripSlotDef` + fills, z 0–84, at most 40, placed on beige and the uncovered right end. One `PlacedLayout` (slots, fill_required, fills) is what preview, `capture_resolved`, and export consume. `pick_underfill_paths` treats extra paths as used collage. `SlotState` resizes to `slots.len()`.

Small folder, repeats off: `need = unique file count`, empty leftover static slots, filled cards still reach the right end (2a column plus extras recycled from used files only if needed for coverage, and only then if the user allowed repeats — otherwise shift filled static cards during place so they are not all packed left). Prefer shifting/placing the filled static set across the width over repeating.

Done when: a folder larger than the static board shows extra tiles in slides 9–10 and in the snapshot; locked replace/hit-test works on an extra tile; export from lock keeps those tiles; underfill pool does not reuse an extra’s file while unused files remain.

Verify: GUI Shuffle on a large folder; lock, click an extra tile, replace it; Export strip. CLI seed 7.

---

### Phase 3 — Beige holes, including the slice-10 footer

Files: `crates/core/src/template.rs` (`background_tail_slice_count` 2), `crates/core/src/underfill.rs`, `crates/render/src/strip_export.rs` (tail tolerance bonus, flatten includes extra collage cards), `crates/render/src/compositor.rs` (`render_required_slots_flat_rgb` vertical bleed matching tile export), `crates/core/tests/phase1.rs` (tail count 1 → 2), `crates/core/tests/phase4.rs` (`tail_band_single_slice` → `(8640, 10800)`).

Tail band is slices 9–10. Retry missed beige anchors with a wider tolerance; stop when the band is filled or the layer budget is spent. Bottom holes use `sy0 = canvas_height - bh`. Flatten occupancy includes Phase 2 collage (static + extras) and vertical bleed so y&lt;0 cards are not beige. Preview and export already share `flatten_and_plan_underfill` at full canvas size; do not scale the tail band.

Done when: borderless mural v2 at a fixed seed has no white or beige footer on slice 10, and leftover beige in slices 7–10 sits under collage rather than as canvas.

Verify: CLI seed 7 mural v2 to `output/strip_latest`; open `_09.jpg` and `_10.jpg`. GUI place, same slides. `cargo test -p core --test phase1 --test phase4`.

---

### Phase 4 — Stop chopping subjects

Files: `crates/render/src/fit.rs`, decode path when pan is 0, `crates/core/src/layout_jitter.rs` (on-canvas clamp in the same score/clamp path as 2a, not a second jitter). Do not call vision/ONNX. Do not import out-of-frame pose rules.

Visible dest is `slot ∩ canvas`. If pan is still 0, bias cover so a tall source’s upper-middle lands in that intersection. After jitter `clamp_xy`, if on-canvas area is small, pull y (and x) back. Stored user pan is left alone.

Done when: a close-up face in a large card is not cut through the eyes by the top of the strip. Neighbor overlap is still collage.

Verify: GUI + export of the same mural; inspect large portraits and the top edge of slices. Unit test cover pan for a tall source in a slot that hangs above y=0.

---

### Phase 5 — Export uses pixels, 8MB per slice only

Files: `crates/render/src/jpeg.rs`, `crates/render/src/export.rs`, `crates/render/src/strip_export.rs`, `crates/app/src/gui/mod.rs`, `crates/app/src/main.rs`. Do not edit underfill planning.

`StripExportParams.export_scale` default 2.0. Locked and unlocked pipelines build the scene at that scale. `export_scene_tiles` gets scaled slice width/height so the camera matches the scene (`export.rs` currently passes raw `template.slice_width`). Decode after the scaled scene so dest sizes are 2×. Underfill boxes planned at 1× are scaled into the scene. Procedural backgrounds render at the scaled canvas.

Slices: `encode_slice_jpeg` at quality 98 with `CAROUSEL_SLICE_MAX_BYTES` (8MB) per file. Search down only when q98 exceeds 8MB. Wide: quality 98, no byte cap, replace `encode_jpeg_uncapped`’s q95/q65 ladder. Do not add a 15MB cap. Do not pad.

GUI: **Export strip** is 2160×2700. **Export fast** is 1080×1350. CLI default 2×; `--strip-fast` for 1×. Help text must not claim 1080×1350 as the only output size.

Done when: default export writes ten 2160×2700 slices that look sharper at 100% than fast 1×; files are larger because of pixels (often around 1–4MB on real photos), never padded to 8MB; a slice over 8MB at q98 is reduced until under cap; the wide master is not run through the slice cap.

Verify: GUI Export strip and Export fast to `output/strip_latest`; CLI default and `--strip-fast`; compare `_01.jpg` at 100%; confirm `_wide.jpg` can exceed 8MB.

## Estimated duration

- Phase 1: 3–5 agent-hours
- Phase 2a: 3–5 agent-hours
- Phase 2b: 5–8 agent-hours
- Phase 3: 4–7 agent-hours
- Phase 4: 4–6 agent-hours
- Phase 5: 4–6 agent-hours
