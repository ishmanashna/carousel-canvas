# Out of frame fix plan

> **For agentic workers:** Use subagent-driven development. One Composer 2.5 implementer per task (not Fast). Do not git commit unless the user asks. Do not `cargo test` the whole workspace.

**Goal:** Make `strip_out_of_frame_v1` match the locked product rules on a seed-7 TEST IMAGES export, without breaking the already-working analyze → place → preview → export pipeline.

**Architecture:** Pipeline stays. Fixes are (1) GUI crash guards, (2) placer occupancy that uses silhouettes not dest bboxes, (3) hard reject of slide-line occupancy, (4) a guaranteed slice-1 step-out figure, (5) decode fringe color. Re-export seed 7 after placer tasks.

**Tech stack:** Same as `docs/OUT_OF_FRAME_PLAN.md` (`crates/core` placer, `crates/render` decode, `crates/app` GUI).

## What the wave found (2026-09-14)

Six Composer 2.5 reviewers, reports in `.superpowers/sdd/wave-review-*.md`.

| Surface | Result |
| --- | --- |
| Tests | **64/64 pass** (`core` phase1+out_of_frame, hero_pin, capture_resolved, analysis, `render --lib`, `vision`, `app`). Pipeline compiles and unit-tests. |
| CLI/GUI wiring | Analyze, place, borderless, lock `capture_resolved`, CLI export **work**. |
| Visual (`carousel_1789341021_721`) | Papers + hard edges **pass**. Slice-1 hook, slide-line faces, people-on-people, foot-in-paper **fail**. |
| Spec vs code | 38 Met / 9 Partial / 1 Missing (slice-1 hook not guaranteed). |

Do **not** treat “slice 1 is a full paper of a band” as a decode bug. Papers may contain people. Failures are: a **figure** sitting on a face, a **seam** through a face, and **no cutout stepping off paper 1**.

## Out of scope

Mural/polaroid. Poisson blend. YOLO. Non-Windows ORT bootstrap. Folder-reload skipping ONNX (mask cache already makes rescans cheap). Whole-workspace `cargo test`. Git commit.

## Global constraints

- Canvas 10800×1350, slices 1080, always `CardEdge::Borderless`.
- `resolved_slots(seed)` stays empty; real API is `resolve_out_of_frame_slots`.
- Occupancy under the **subject**, not the empty margin of the dest rect.
- After accept, stamp **silhouette** so the next figure cannot sit on that person.
- Slide line `x = k * 1080` (k=1..9) through occupied face/mid-torso → illegal (papers and figures). Empty paper on a seam is OK.
- Slice 1: one dominant paper **and** one figure breaking its edge.
- Core has no ONNX dep.

---

### Task 1 — GUI: no panic on stale overlay

**Why:** Shuffle resizes `assignments` immediately; `preview_overlay_slots` stays at the old count until preview finishes. Click uses `assignments[slot]` (`gui/mod.rs` ~1438–1595).

**Files:** `crates/app/src/gui/mod.rs`

- [ ] In `place_out_of_frame` (success and error): `preview_overlay_slots.clear()` (or set to the new slots) so hit-test cannot return a dead index.
- [ ] Every `assignments[slot]` in `draw_preview` uses `.get(slot)` / length guard (same as `fills_for_preview`).
- [ ] `cargo test -p app`

**Done when:** Fewer-slot shuffle cannot panic on click. No placer changes.

---

### Task 2 — Figure occupancy from mask, not dest mean

**Why:** `mean_in_canvas_rect` over the whole dest box stays &lt; 0.08 while the head sits on a dense paper (visual slice 08 people-on-people). Plan: occupancy under the subject.

**Files:** `crates/core/src/layout_out_of_frame.rs`, `crates/core/tests/out_of_frame.rs`

- [ ] Map figure `PhotoAnalysis.occupancy` (or mask) into dest with the same cover transform decode will use.
- [ ] Reject if **max** (or mean of inner 60%×70% of the **subject**, not the full rect) on paper occupancy &gt; 0.08.
- [ ] After accept, `stamp` those occupied cells (silhouette), not a solid dest rectangle.
- [ ] Figure–figure: mask intersection ≳ 10% of either silhouette fails (bbox as prefilter only).
- [ ] Test: dest rect mostly empty but inner subject over a peak → **reject**. Test: two silhouettes overlap, bboxes barely overlap → **reject**.
- [ ] `cargo test -p core --test out_of_frame`

**Done when:** Those tests pass. Do not retune paper sizes here.

---

### Task 3 — Slide lines: occupancy column, hard reject

**Why:** Seams 02\|03, 05\|06, 07\|08 cut faces. `slices_cut_inner_band` is dest-bbox; paper nudge restores `original_x` if it fails.

**Files:** same as Task 2

- [ ] For papers and figures: sample occupancy on `x = k * slice_w` through the inner ~70% height of the **subject** (cover-mapped). Occupied → illegal.
- [ ] After paper nudge, if still hitting: resample pose or **drop that paper x**, do not keep the original.
- [ ] Optional cheap: ignore ±2.2° rotation in occupancy **or** rasterize rotated occupancy; do not leave stamp axis-aligned while compositor rotates if seams still fail.
- [ ] Test: occupancy peak on x=2160 in face band → paper/figure pose rejected after nudge.
- [ ] `cargo test -p core --test out_of_frame`

**Done when:** Integration-style test on fake analyses: no accepted paper/figure returns `paper_occupancy_hits_slice_lines` / equivalent true.

---

### Task 4 — Slice 1 hook that cannot no-op

**Why:** `ensure_slice1_hook_figure` can return without inserting; bonus only applies to shuffle index 0. Seed-7 slice 1 is paper-only.

**Files:** same placer + `pick_out_of_frame_fills`

- [ ] Dest size from subject bbox; pan/trim so the bbox maps into dest (fixes dest≠rendered silhouette).
- [ ] Hook search uses **current** occupancy + placed figures (not empty `placed`).
- [ ] Replace-or-skip: never insert a cutout with no remaining figure path (`figure_slots.len() <= figure pool`). Prefer swapping out the worst non-hook figure.
- [ ] If a figure exists that can legally break paper 1 in `[0, slice_w)`, the returned slots **must** include that cutout.
- [ ] Test already named `slice1_includes_cutout_when_empty_occupancy_exists` must fail if the hook is missing on the fake gallery.
- [ ] `cargo test -p core --test out_of_frame`

**Done when:** Fake gallery with empty slice-1 paper pocket always yields a cutout overlapping `[0, 1080)`.

---

### Task 5 — Fills follow placement, not a second shuffle

**Why:** Promoting figures to papers then independently shuffling fill pools can leave extra cutout slots `None`.

**Files:** `layout_out_of_frame.rs` (`pick_out_of_frame_fills` or return `(slots, paths)`), `assign.rs` wrapper, CLI/GUI if signature changes

- [ ] Each placed slot remembers the analysis index (or path) used at pose time.
- [ ] Fills copy those paths; shuffle only unused extras.
- [ ] Test: hook + promotion cannot produce more cutout slots than figure-role paths.
- [ ] `cargo test -p core --test out_of_frame`

---

### Task 6 — Fringe decontam vs canvas beige

**Why:** Cutout unmix uses default white; template bg is `#ece8e3`. Visual did not flag huge halos; still a spec mismatch.

**Files:** `crates/render/src/decode.rs`, `cache.rs`, preview/export where `DecodeOptions` is built

- [ ] Pass scene/template background into cutout decontam (OOF `#ece8e3`).
- [ ] Put that RGB in `CacheKey` so old white-fringe cache entries are not reused.
- [ ] `cargo test -p render --lib`

**Done when:** Fixture mask decode against beige does not unmix as if background were white.

---

### Task 7 — Eyes (no design, only score)

**Depends on:** Tasks 2–5 (6 optional).

```text
cargo run -p app --release -- "TEST IMAGES" --strip --strip-template strip_out_of_frame_v1 --output output/strip_latest --strip-layout-seed 7 --strip-card-edge borderless
```

Score the new `carousel_*_wide.jpg` + slices 01, 02, 03, 05, 06, 08:

| Rule | Must |
| --- | --- |
| Slice 1 | Paper + figure clearly off the print edge onto cream |
| Seams 02\|03, 05\|06, 07\|08 | No face / mid-torso split |
| Slice 08 | No sharp profile glued on another face |
| Feet | Hidden in empty paper, not mid-air on a face |

If a rule still fails, that is a **Task 2–4 bug**, not a new template. Write notes in `.superpowers/sdd/phase-fix-eyes.md`. Do not “tune percents” without a failing test.

---

## Suggested order

1 → 2 → 3 → 4 → 5 → 6 → 7

1 and 6 can run after 1 without waiting on placer if you serialize file owners: **do not** parallel-edit `layout_out_of_frame.rs`. Task 1 (`gui/mod.rs`) and Task 6 (`render`) may run **after** Task 1 only if no other GUI writer is active; Task 6 may run parallel with 2–5.

## Won’t fix this round

- GUI replace-on-cutout keeping old `mask_path` (block replace or re-analyze one file — small follow-up).
- Reload folder always calling `start_analysis` (cache hits; UX only).
- `curl`/`tar` model download robustness.
- Full GUI click-through (no egui harness in-repo).
