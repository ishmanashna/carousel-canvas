//! Out-of-frame strip layout: overlapping paper photos + cutout figures in empty space.

use std::path::PathBuf;

use crate::analysis::{OccupancyMap, PhotoAnalysis, PhotoRole};
use crate::error::{CoreError, Result};
use crate::python_rng::PythonRandom;
use crate::slot::{Fit, StripSlotDef};

const OCC_SCALE: i32 = 8;
const GRID_STEP: i32 = 24;
const OCCUPANCY_REJECT: f64 = 0.08;
const FIGURE_OVERLAP_REJECT: f64 = 0.10;
const BOTTOM_BAND_FRAC: f64 = 0.18;
const INNER_W_MARGIN: f64 = 0.20; // inner 60% width
const INNER_H_MARGIN: f64 = 0.15; // inner 70% height
const MIN_PAPERS: usize = 6;
const TARGET_PAPERS_MIN: i32 = 8;
const TARGET_PAPERS_MAX: i32 = 10;
const MAX_FIGURES: usize = 8;
const PAPER_OCC_DILATE: i32 = 6;
const PAPER_OVERSCAN_Y: i32 = 100;
const PAPER_SIDE_OVERSCAN: i32 = 80;
const PAPER_W_MIN: i32 = 1600;
const PAPER_W_MAX: i32 = 2600;
const PAPER_OVERLAP_MIN: f64 = 0.34;
const PAPER_OVERLAP_MAX: f64 = 0.50;
const FIGURE_HEIGHT_MIN_FRAC: f64 = 0.45;
const FIGURE_HEIGHT_MAX_FRAC: f64 = 0.95;
const FIGURE_SCALE_STEPS_MIN: i32 = 8;
const FIGURE_SCALE_STEPS_MAX: i32 = 12;
const FIRST_FIGURE_SLICE_BONUS: f64 = 800.0;
const EDGE_OVERFLOW_BONUS: f64 = 45.0;
const PEAK_CENTER_PENALTY: f64 = 250.0;

/// Coarse canvas occupancy grid (1/8 resolution). Exposed for layout tests.
pub struct CanvasOccupancy {
    grid_w: i32,
    grid_h: i32,
    cells: Vec<f32>,
}

impl CanvasOccupancy {
    fn new(canvas_w: i32, canvas_h: i32) -> Self {
        let grid_w = (canvas_w + OCC_SCALE - 1) / OCC_SCALE;
        let grid_h = (canvas_h + OCC_SCALE - 1) / OCC_SCALE;
        Self {
            grid_w,
            grid_h,
            cells: vec![0.0; (grid_w * grid_h) as usize],
        }
    }

    fn canvas_to_grid(&self, x: i32, y: i32) -> (i32, i32) {
        (x / OCC_SCALE, y / OCC_SCALE)
    }

    fn dilate_in_place(&mut self, radius: i32) {
        let w = self.grid_w;
        let h = self.grid_h;
        let src = self.cells.clone();
        for gy in 0..h {
            for gx in 0..w {
                let mut max_v = src[(gy * w + gx) as usize];
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let nx = gx + dx;
                        let ny = gy + dy;
                        if nx >= 0 && ny >= 0 && nx < w && ny < h {
                            max_v = max_v.max(src[(ny * w + nx) as usize]);
                        }
                    }
                }
                self.cells[(gy * w + gx) as usize] = max_v;
            }
        }
    }

    fn mean_in_canvas_rect(&self, x: i32, y: i32, w: i32, h: i32) -> f64 {
        self.mean_in_grid_rect_mapped(x, y, w, h, 1)
    }

    fn mean_in_grid_rect_mapped(&self, x: i32, y: i32, w: i32, h: i32, stride: i32) -> f64 {
        if w <= 0 || h <= 0 {
            return 0.0;
        }
        let (gx0, gy0) = self.canvas_to_grid(x, y);
        let (gx1, gy1) = self.canvas_to_grid(x + w - 1, y + h - 1);
        let gy_end = gy1.min(self.grid_h - 1);
        let gx_end = gx1.min(self.grid_w - 1);
        let mut gy = gy0.max(0);
        if gy > gy_end || gx0.max(0) > gx_end {
            return 0.0;
        }
        let mut sum = 0.0;
        let mut count = 0usize;
        while gy <= gy_end {
            let mut gx = gx0.max(0);
            while gx <= gx_end {
                sum += self.cells[(gy * self.grid_w + gx) as usize] as f64;
                count += 1;
                gx += stride;
            }
            gy += stride;
        }
        if count == 0 {
            0.0
        } else {
            sum / count as f64
        }
    }

    fn stamp_paper_cover(&mut self, slot: &StripSlotDef, occ: &OccupancyMap) {
        let sw = occ.width.max(1) as f64;
        let sh = occ.height.max(1) as f64;
        let dw = slot.w.max(1) as f64;
        let dh = slot.h.max(1) as f64;
        let scale = (dw / sw).max(dh / sh);
        let mapped_w = sw * scale;
        let mapped_h = sh * scale;
        let off_x = slot.x as f64 + (dw - mapped_w) * 0.5;
        let off_y = slot.y as f64 + (dh - mapped_h) * 0.5;

        let (gx0, gy0) = self.canvas_to_grid(slot.x, slot.y);
        let (gx1, gy1) = self.canvas_to_grid(slot.x + slot.w - 1, slot.y + slot.h - 1);
        for gy in gy0.max(0)..=gy1.min(self.grid_h - 1) {
            for gx in gx0.max(0)..=gx1.min(self.grid_w - 1) {
                let cx = gx as f64 * OCC_SCALE as f64 + OCC_SCALE as f64 * 0.5;
                let cy = gy as f64 * OCC_SCALE as f64 + OCC_SCALE as f64 * 0.5;
                if cx < off_x || cy < off_y || cx >= off_x + mapped_w || cy >= off_y + mapped_h {
                    continue;
                }
                let sx = ((cx - off_x) / scale).floor() as u32;
                let sy = ((cy - off_y) / scale).floor() as u32;
                let sx = sx.min(occ.width - 1);
                let sy = sy.min(occ.height - 1);
                let v = occ.occupied[(sy * occ.width + sx) as usize] as f32 / 255.0;
                let idx = (gy * self.grid_w + gx) as usize;
                if v > self.cells[idx] {
                    self.cells[idx] = v;
                }
            }
        }
    }

    fn stamp_figure_bbox(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let (gx0, gy0) = self.canvas_to_grid(x, y);
        let (gx1, gy1) = self.canvas_to_grid(x + w - 1, y + h - 1);
        for gy in gy0.max(0)..=gy1.min(self.grid_h - 1) {
            for gx in gx0.max(0)..=gx1.min(self.grid_w - 1) {
                self.cells[(gy * self.grid_w + gx) as usize] = 1.0;
            }
        }
    }

    fn center_vs_edge_occupancy(&self, x: i32, y: i32, w: i32, h: i32) -> (f64, f64) {
        let cx = x + w / 2;
        let cy = y + h / 2;
        let half = (w.min(h) / 6).max(GRID_STEP);
        let center = self.mean_in_canvas_rect(cx - half / 2, cy - half / 2, half, half);
        let edge = (self.mean_in_canvas_rect(x, y, w, h / 5)
            + self.mean_in_canvas_rect(x, y + h - h / 5, w, h / 5)
            + self.mean_in_canvas_rect(x, y, w / 5, h)
            + self.mean_in_canvas_rect(x + w - w / 5, y, w / 5, h))
            / 4.0;
        (center, edge)
    }

    /// Canvas center of the highest-occupancy grid cell (for tests).
    pub fn occupancy_peak_center(&self) -> (i32, i32) {
        let mut best_idx = 0usize;
        let mut best_val = 0.0f32;
        for (i, &v) in self.cells.iter().enumerate() {
            if v > best_val {
                best_val = v;
                best_idx = i;
            }
        }
        let gx = (best_idx as i32) % self.grid_w;
        let gy = (best_idx as i32) / self.grid_w;
        (
            gx * OCC_SCALE + OCC_SCALE / 2,
            gy * OCC_SCALE + OCC_SCALE / 2,
        )
    }

    fn snapshot(&self) -> Self {
        Self {
            grid_w: self.grid_w,
            grid_h: self.grid_h,
            cells: self.cells.clone(),
        }
    }
}

fn align_down(v: i32, step: i32) -> i32 {
    (v / step) * step
}

fn intersection_area(ax: i32, ay: i32, aw: i32, ah: i32, bx: i32, by: i32, bw: i32, bh: i32) -> f64 {
    let ix0 = ax.max(bx);
    let iy0 = ay.max(by);
    let ix1 = (ax + aw).min(bx + bw);
    let iy1 = (ay + ah).min(by + bh);
    if ix1 <= ix0 || iy1 <= iy0 {
        0.0
    } else {
        (ix1 - ix0) as f64 * (iy1 - iy0) as f64
    }
}

fn rect_contains_rect(px: i32, py: i32, pw: i32, ph: i32, x: i32, y: i32, w: i32, h: i32) -> bool {
    x >= px && y >= py && x + w <= px + pw && y + h <= py + ph
}

fn slices_cut_inner_band(x: i32, y: i32, w: i32, h: i32, slice_w: i32, slice_count: i32) -> bool {
    let ix0 = x + (w as f64 * INNER_W_MARGIN).round() as i32;
    let ix1 = x + w - (w as f64 * INNER_W_MARGIN).round() as i32;
    let iy0 = y + (h as f64 * INNER_H_MARGIN).round() as i32;
    let iy1 = y + h - (h as f64 * INNER_H_MARGIN).round() as i32;
    if iy1 <= iy0 {
        return false;
    }
    for k in 1..slice_count {
        let sx = k * slice_w;
        if sx >= ix0 && sx <= ix1 {
            return true;
        }
    }
    false
}

fn bottom_band_ok(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    papers: &[StripSlotDef],
    occ: &CanvasOccupancy,
) -> bool {
    let band_y = y + (h as f64 * (1.0 - BOTTOM_BAND_FRAC)).floor() as i32;
    let band_h = y + h - band_y;
    if band_h <= 0 {
        return false;
    }
    let inside = papers
        .iter()
        .any(|p| rect_contains_rect(p.x, p.y, p.w, p.h, x, band_y, w, band_h));
    if !inside {
        return false;
    }
    occ.mean_in_canvas_rect(x, band_y, w, band_h) < OCCUPANCY_REJECT
}

fn figure_figure_overlap_too_high(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    placed: &[(i32, i32, i32, i32)],
) -> bool {
    let area = (w * h) as f64;
    if area <= 0.0 {
        return true;
    }
    for &(px, py, pw, ph) in placed {
        let inter = intersection_area(x, y, w, h, px, py, pw, ph);
        let p_area = (pw * ph) as f64;
        if p_area > 0.0 && (inter / area > FIGURE_OVERLAP_REJECT || inter / p_area > FIGURE_OVERLAP_REJECT) {
            return true;
        }
    }
    false
}

fn paper_x_bounds_for_overlap(
    prev: &StripSlotDef,
    overlap_frac: f64,
) -> i32 {
    prev.x + prev.w - (prev.w as f64 * overlap_frac).round() as i32
}

fn nudge_paper_x_avoid_slice_lines(
    slot: &mut StripSlotDef,
    occ: &OccupancyMap,
    slice_w: i32,
    slice_count: i32,
    min_x: i32,
    max_x: i32,
) {
    if !paper_occupancy_hits_slice_lines(slot, occ, slice_w, slice_count) {
        return;
    }
    let original_x = slot.x;
    for step in 1..=80 {
        for sign in [-1i32, 1] {
            let nx = original_x + sign * step * GRID_STEP;
            if nx < min_x || nx > max_x {
                continue;
            }
            slot.x = nx;
            if !paper_occupancy_hits_slice_lines(slot, occ, slice_w, slice_count) {
                return;
            }
        }
    }
    slot.x = original_x;
}

fn build_paper_slots(
    papers: &[PhotoAnalysis],
    canvas_w: i32,
    canvas_h: i32,
    slice_w: i32,
    slice_count: i32,
    rng: &mut PythonRandom,
) -> Vec<(StripSlotDef, usize)> {
    let n = papers.len();
    if n == 0 {
        return vec![];
    }
    let extra = rng.randint(0, 80);
    let h = canvas_h + PAPER_OVERSCAN_Y + extra;
    let y = -((h - canvas_h) / 2);
    let mut out: Vec<(StripSlotDef, usize)> = Vec::with_capacity(n);
    for i in 0..n {
        let w = rng.randint(PAPER_W_MIN, PAPER_W_MAX);
        let (x, min_x, max_x) = if i == 0 {
            let x = -PAPER_SIDE_OVERSCAN;
            (x, -PAPER_SIDE_OVERSCAN, 0)
        } else {
            let prev_slot = &out[i - 1].0;
            let overlap_frac = rng.uniform(PAPER_OVERLAP_MIN, PAPER_OVERLAP_MAX);
            let x = paper_x_bounds_for_overlap(prev_slot, overlap_frac);
            let min_x = paper_x_bounds_for_overlap(prev_slot, PAPER_OVERLAP_MAX);
            let max_x = paper_x_bounds_for_overlap(prev_slot, PAPER_OVERLAP_MIN);
            (x, min_x.min(max_x), min_x.max(max_x))
        };
        let mut slot = StripSlotDef {
            x,
            y,
            w,
            h,
            rotation_deg: 0.0,
            z_index: i as i32,
            fit: Fit::Cover,
            prefer_portrait: false,
            prefer_landscape: false,
            polaroid: false,
            horizontal_center_band_frac: None,
            source_trim_left_frac: None,
            cover_height_first: false,
            cutout: false,
            mask_path: None,
            source_crop: None,
        };
        nudge_paper_x_avoid_slice_lines(
            &mut slot,
            &papers[i].occupancy,
            slice_w,
            slice_count,
            min_x,
            max_x,
        );
        out.push((slot, i));
    }
    if let Some((last, _)) = out.last_mut() {
        let need = canvas_w + PAPER_SIDE_OVERSCAN;
        if last.x + last.w < need {
            last.w = (need - last.x).max(1);
        }
    }
    out
}

fn score_figure_pose(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    fig_idx: usize,
    papers: &[StripSlotDef],
    occ: &CanvasOccupancy,
    slice_w: i32,
) -> f64 {
    let mut score = 0.0;
    for (i, paper) in papers.iter().enumerate() {
        if intersection_area(x, y, w, h, paper.x, paper.y, paper.w, paper.h) <= 0.0 {
            continue;
        }
        if y < paper.y {
            score += EDGE_OVERFLOW_BONUS;
        }
        if x + w > paper.x + paper.w {
            score += EDGE_OVERFLOW_BONUS;
        }
        if x < paper.x {
            score += EDGE_OVERFLOW_BONUS;
        }
        if fig_idx == 0 && i == 0 {
            let overlaps_slice1 = x < slice_w && x + w > 0;
            let overflows = y < paper.y
                || x < paper.x
                || x + w > paper.x + paper.w;
            if overlaps_slice1 && overflows {
                score += FIRST_FIGURE_SLICE_BONUS;
            }
        }
    }
    let (center, edge) = occ.center_vs_edge_occupancy(x, y, w, h);
    if center > edge + 0.04 {
        score -= PEAK_CENTER_PENALTY;
    }
    score -= center * 30.0;
    score
}

fn pose_slice1_overflows_paper0(
    x: i32,
    y: i32,
    w: i32,
    _h: i32,
    paper0: &StripSlotDef,
    slice_w: i32,
) -> bool {
    let overlaps_slice1 = x < slice_w && x + w > 0;
    let overflows = y < paper0.y
        || x < paper0.x
        || x + w > paper0.x + paper0.w;
    overlaps_slice1 && overflows
}

fn figure_slice1_overflows_paper0(fig: &StripSlotDef, paper0: &StripSlotDef, slice_w: i32) -> bool {
    pose_slice1_overflows_paper0(fig.x, fig.y, fig.w, fig.h, paper0, slice_w)
}

fn figure_cutout_slot(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    z: i32,
    analysis: &PhotoAnalysis,
) -> StripSlotDef {
    let mut slot = StripSlotDef::new(x, y, w, h)
        .with_z(z)
        .with_cutout()
        .with_mask_path(analysis.mask_png.clone());
    if let Some(crop) = crate::analysis::padded_subject_crop(analysis.subject_bbox) {
        slot = slot.with_source_crop(crop);
    }
    slot
}

fn find_best_figure_pose(
    analysis: &PhotoAnalysis,
    fig_idx: usize,
    paper_slots: &[StripSlotDef],
    paper_filter: Option<usize>,
    require_slice1_hook: bool,
    canvas_h: i32,
    slice_w: i32,
    slice_count: i32,
    occ: &CanvasOccupancy,
    placed: &[(i32, i32, i32, i32)],
    scale_steps: usize,
) -> Option<StripSlotDef> {
    let [_, _, sbw, sbh] = analysis.subject_bbox;
    if sbw <= 0 || sbh <= 0 {
        return None;
    }
    let aspect = sbw as f64 / sbh as f64;
    let paper0 = paper_slots.first()?;
    let mut best: Option<(StripSlotDef, f64)> = None;

    for step in 0..scale_steps {
        let t = if scale_steps <= 1 {
            0.5
        } else {
            step as f64 / (scale_steps - 1) as f64
        };
        let height_frac =
            FIGURE_HEIGHT_MIN_FRAC + t * (FIGURE_HEIGHT_MAX_FRAC - FIGURE_HEIGHT_MIN_FRAC);
        let fh = (canvas_h as f64 * height_frac).round() as i32;
        let fw = (fh as f64 * aspect).round() as i32;
        if fw <= 0 || fh <= 0 {
            continue;
        }
        let band_h = (fh as f64 * BOTTOM_BAND_FRAC).ceil() as i32;
        if band_h <= 0 || fh <= band_h {
            continue;
        }

        for (pi, paper) in paper_slots.iter().enumerate() {
            if paper_filter.map_or(false, |p| p != pi) {
                continue;
            }
            let x_lo = paper.x;
            let x_hi = paper.x + paper.w - fw;
            let y_lo = (paper.y + band_h - fh).max(0);
            let y_hi = (paper.y + paper.h - fh).min(canvas_h - fh);
            if x_hi < x_lo || y_hi < y_lo {
                continue;
            }

            let mut y = align_down(y_lo, GRID_STEP);
            if y < y_lo {
                y += GRID_STEP;
            }
            while y <= y_hi {
                let mut x = align_down(x_lo, GRID_STEP);
                if x < x_lo {
                    x += GRID_STEP;
                }
                while x <= x_hi {
                    if require_slice1_hook
                        && !pose_slice1_overflows_paper0(x, y, fw, fh, paper0, slice_w)
                    {
                        x += GRID_STEP;
                        continue;
                    }
                    if occ.mean_in_grid_rect_mapped(x, y, fw, fh, 2) > OCCUPANCY_REJECT {
                        x += GRID_STEP;
                        continue;
                    }
                    if occ.mean_in_canvas_rect(x, y, fw, fh) > OCCUPANCY_REJECT {
                        x += GRID_STEP;
                        continue;
                    }
                    if slices_cut_inner_band(x, y, fw, fh, slice_w, slice_count) {
                        x += GRID_STEP;
                        continue;
                    }
                    if !bottom_band_ok(x, y, fw, fh, paper_slots, occ) {
                        x += GRID_STEP;
                        continue;
                    }
                    if figure_figure_overlap_too_high(x, y, fw, fh, placed) {
                        x += GRID_STEP;
                        continue;
                    }

                    let score = score_figure_pose(x, y, fw, fh, fig_idx, paper_slots, occ, slice_w);
                    let slot = figure_cutout_slot(x, y, fw, fh, 100, analysis);
                    if best.as_ref().map_or(true, |(_, s)| score > *s) {
                        best = Some((slot, score));
                    }
                    x += GRID_STEP;
                }
                y += GRID_STEP;
            }
        }
    }
    best.map(|(slot, _)| slot)
}

fn ensure_slice1_hook_figure(
    figures: &[PhotoAnalysis],
    paper_slots: &[StripSlotDef],
    canvas_h: i32,
    slice_w: i32,
    slice_count: i32,
    paper_only_occ: &CanvasOccupancy,
    out: &mut Vec<StripSlotDef>,
) {
    if figures.is_empty() || paper_slots.is_empty() {
        return;
    }
    let paper0 = &paper_slots[0];
    if out.iter().any(|f| figure_slice1_overflows_paper0(f, paper0, slice_w)) {
        return;
    }

    let scale_steps = FIGURE_SCALE_STEPS_MAX as usize;
    for fi in 0..figures.len() {
        if let Some(mut slot) = find_best_figure_pose(
            &figures[fi],
            0,
            paper_slots,
            Some(0),
            true,
            canvas_h,
            slice_w,
            slice_count,
            paper_only_occ,
            &[],
            scale_steps,
        ) {
            out.retain(|f| {
                let inter = intersection_area(slot.x, slot.y, slot.w, slot.h, f.x, f.y, f.w, f.h);
                let slot_area = (slot.w * slot.h) as f64;
                let f_area = (f.w * f.h) as f64;
                slot_area <= 0.0
                    || f_area <= 0.0
                    || (inter / slot_area <= FIGURE_OVERLAP_REJECT
                        && inter / f_area <= FIGURE_OVERLAP_REJECT)
            });
            slot.z_index = 100;
            out.insert(0, slot);
            for (i, fig) in out.iter_mut().enumerate() {
                fig.z_index = 100 + i as i32;
            }
            return;
        }
    }
}

fn place_figures(
    figures: &[PhotoAnalysis],
    papers: &[(StripSlotDef, usize)],
    _canvas_w: i32,
    canvas_h: i32,
    slice_w: i32,
    slice_count: i32,
    occ: &mut CanvasOccupancy,
    rng: &mut PythonRandom,
) -> Vec<StripSlotDef> {
    let paper_slots: Vec<StripSlotDef> = papers.iter().map(|(s, _)| s.clone()).collect();
    let paper_only_occ = occ.snapshot();
    let mut indices: Vec<usize> = (0..figures.len()).collect();
    rng.shuffle_usize(&mut indices);
    let cap = figures.len().min(MAX_FIGURES);
    let mut placed_bboxes: Vec<(i32, i32, i32, i32)> = Vec::new();
    let mut out = Vec::new();

    for fig_idx in 0..cap {
        let fi = indices[fig_idx];
        let analysis = &figures[fi];
        let [_, _, sbw, sbh] = analysis.subject_bbox;
        if sbw <= 0 || sbh <= 0 {
            continue;
        }
        let aspect = sbw as f64 / sbh as f64;

        let scale_steps = rng.randint(FIGURE_SCALE_STEPS_MIN, FIGURE_SCALE_STEPS_MAX) as usize;
        let mut best: Option<(StripSlotDef, f64)> = None;

        for step in 0..scale_steps {
            let t = if scale_steps <= 1 {
                0.5
            } else {
                step as f64 / (scale_steps - 1) as f64
            };
            let height_frac =
                FIGURE_HEIGHT_MIN_FRAC + t * (FIGURE_HEIGHT_MAX_FRAC - FIGURE_HEIGHT_MIN_FRAC);
            let fh = (canvas_h as f64 * height_frac).round() as i32;
            let fw = (fh as f64 * aspect).round() as i32;
            if fw <= 0 || fh <= 0 {
                continue;
            }

            let band_h = (fh as f64 * BOTTOM_BAND_FRAC).ceil() as i32;
            if band_h <= 0 || fh <= band_h {
                continue;
            }

            for paper in &paper_slots {
                let x_lo = paper.x;
                let x_hi = paper.x + paper.w - fw;
                let y_lo = (paper.y + band_h - fh).max(0);
                let y_hi = (paper.y + paper.h - fh).min(canvas_h - fh);
                if x_hi < x_lo || y_hi < y_lo {
                    continue;
                }

                let mut y = align_down(y_lo, GRID_STEP);
                if y < y_lo {
                    y += GRID_STEP;
                }
                while y <= y_hi {
                    let mut x = align_down(x_lo, GRID_STEP);
                    if x < x_lo {
                        x += GRID_STEP;
                    }
                    while x <= x_hi {
                        if occ.mean_in_grid_rect_mapped(x, y, fw, fh, 2) > OCCUPANCY_REJECT {
                            x += GRID_STEP;
                            continue;
                        }
                        if occ.mean_in_canvas_rect(x, y, fw, fh) > OCCUPANCY_REJECT {
                            x += GRID_STEP;
                            continue;
                        }
                        if slices_cut_inner_band(x, y, fw, fh, slice_w, slice_count) {
                            x += GRID_STEP;
                            continue;
                        }
                        if !bottom_band_ok(x, y, fw, fh, &paper_slots, occ) {
                            x += GRID_STEP;
                            continue;
                        }
                        if figure_figure_overlap_too_high(x, y, fw, fh, &placed_bboxes) {
                            x += GRID_STEP;
                            continue;
                        }

                        let score = score_figure_pose(
                            x, y, fw, fh, fig_idx, &paper_slots, occ, slice_w,
                        );
                        let slot = figure_cutout_slot(
                            x,
                            y,
                            fw,
                            fh,
                            100 + out.len() as i32,
                            analysis,
                        );
                        if best.as_ref().map_or(true, |(_, s)| score > *s) {
                            best = Some((slot, score));
                        }
                        x += GRID_STEP;
                    }
                    y += GRID_STEP;
                }
            }
        }

        if let Some((slot, _)) = best {
            occ.stamp_figure_bbox(slot.x, slot.y, slot.w, slot.h);
            placed_bboxes.push((slot.x, slot.y, slot.w, slot.h));
            out.push(slot);
        }
    }
    ensure_slice1_hook_figure(
        figures,
        &paper_slots,
        canvas_h,
        slice_w,
        slice_count,
        &paper_only_occ,
        &mut out,
    );
    out
}

fn partition_pools(analyses: &[PhotoAnalysis]) -> (Vec<PhotoAnalysis>, Vec<PhotoAnalysis>) {
    let mut papers = Vec::new();
    let mut figures = Vec::new();
    for a in analyses {
        match a.role {
            PhotoRole::Paper => papers.push(a.clone()),
            PhotoRole::Figure => figures.push(a.clone()),
            PhotoRole::Skip => {}
        }
    }
    while papers.len() < MIN_PAPERS && !figures.is_empty() {
        papers.push(figures.remove(0));
    }
    (papers, figures)
}

/// Place overlapping paper slots and cutout figures from photo analyses.
pub fn resolve_out_of_frame_slots(
    analyses: &[PhotoAnalysis],
    canvas_w: i32,
    canvas_h: i32,
    slice_w: i32,
    seed: i64,
) -> Vec<StripSlotDef> {
    if analyses.is_empty() || canvas_w <= 0 || canvas_h <= 0 || slice_w <= 0 {
        return vec![];
    }

    let slice_count = (canvas_w + slice_w - 1) / slice_w;
    let mut rng = PythonRandom::new(((seed as u32) ^ 0x0F00_F00Du32) as u32);

    let (mut paper_pool, figure_pool) = partition_pools(analyses);
    if paper_pool.is_empty() {
        return vec![];
    }

    let target = rng
        .randint(TARGET_PAPERS_MIN, TARGET_PAPERS_MAX)
        .max(1) as usize;
    let paper_count = target.min(paper_pool.len()).max(1);
    paper_pool.truncate(paper_count);

    let paper_pairs = build_paper_slots(
        &paper_pool,
        canvas_w,
        canvas_h,
        slice_w,
        slice_count,
        &mut rng,
    );

    let mut occ = CanvasOccupancy::new(canvas_w, canvas_h);
    for (slot, idx) in &paper_pairs {
        occ.stamp_paper_cover(slot, &paper_pool[*idx].occupancy);
    }
    occ.dilate_in_place(PAPER_OCC_DILATE);

    let figure_slots = place_figures(
        &figure_pool,
        &paper_pairs,
        canvas_w,
        canvas_h,
        slice_w,
        slice_count,
        &mut occ,
        &mut rng,
    );

    let mut slots: Vec<StripSlotDef> = paper_pairs.into_iter().map(|(s, _)| s).collect();
    slots.extend(figure_slots);
    slots
}

/// First figure slot if any, else first paper.
pub fn flagship_slot_index(slots: &[StripSlotDef]) -> Option<usize> {
    slots
        .iter()
        .position(|s| s.cutout)
        .or_else(|| if slots.is_empty() { None } else { Some(0) })
}

/// Map paper-role analyses onto paper slots and figure-role onto figure slots.
pub fn pick_out_of_frame_fills(
    analyses: &[PhotoAnalysis],
    slots: &[StripSlotDef],
    seed: u32,
) -> Result<Vec<Option<PathBuf>>> {
    if slots.is_empty() {
        return Ok(Vec::new());
    }
    let mut rng = PythonRandom::new(seed);
    let mut papers: Vec<PathBuf> = analyses
        .iter()
        .filter(|a| a.role == PhotoRole::Paper)
        .map(|a| a.path.clone())
        .collect();
    let mut figures: Vec<PathBuf> = analyses
        .iter()
        .filter(|a| a.role == PhotoRole::Figure)
        .map(|a| a.path.clone())
        .collect();
    shuffle_paths(&mut papers, &mut rng);
    shuffle_paths(&mut figures, &mut rng);

    let mut out: Vec<Option<PathBuf>> = vec![None; slots.len()];
    let mut paper_i = 0usize;
    let mut figure_i = 0usize;
    for (i, slot) in slots.iter().enumerate() {
        if slot.cutout {
            if figure_i < figures.len() {
                out[i] = Some(figures[figure_i].clone());
                figure_i += 1;
            }
        } else if paper_i < papers.len() {
            out[i] = Some(papers[paper_i].clone());
            paper_i += 1;
        }
    }
    if out.iter().all(|f| f.is_none()) {
        return Err(CoreError::NeedAtLeastOneImage);
    }
    Ok(out)
}

fn shuffle_paths(paths: &mut [PathBuf], rng: &mut PythonRandom) {
    for i in (1..paths.len()).rev() {
        let j = rng.randbelow((i + 1) as u32) as usize;
        paths.swap(i, j);
    }
}

/// Mean occupancy in the figure bottom band (for tests).
pub fn bottom_band_mean(
    slot: &StripSlotDef,
    occ: &CanvasOccupancy,
) -> f64 {
    let band_y = slot.y + (slot.h as f64 * (1.0 - BOTTOM_BAND_FRAC)).floor() as i32;
    let band_h = slot.y + slot.h - band_y;
    occ.mean_in_canvas_rect(slot.x, band_y, slot.w, band_h)
}

/// Build canvas occupancy from paper slots (for tests).
pub fn build_canvas_occupancy_for_slots(
    canvas_w: i32,
    canvas_h: i32,
    papers: &[(StripSlotDef, &OccupancyMap)],
) -> CanvasOccupancy {
    let mut occ = CanvasOccupancy::new(canvas_w, canvas_h);
    for (slot, map) in papers {
        occ.stamp_paper_cover(slot, map);
    }
    occ.dilate_in_place(PAPER_OCC_DILATE);
    occ
}

/// True when cover-mapped paper occupancy in the inner height band sits on a slice line.
pub fn paper_occupancy_hits_slice_lines(
    slot: &StripSlotDef,
    occ: &OccupancyMap,
    slice_w: i32,
    slice_count: i32,
) -> bool {
    if occ.width == 0 || occ.height == 0 || slice_w <= 0 {
        return false;
    }
    let sw = occ.width.max(1) as f64;
    let sh = occ.height.max(1) as f64;
    let dw = slot.w.max(1) as f64;
    let dh = slot.h.max(1) as f64;
    let scale = (dw / sw).max(dh / sh);
    let mapped_w = sw * scale;
    let mapped_h = sh * scale;
    let off_x = slot.x as f64 + (dw - mapped_w) * 0.5;
    let off_y = slot.y as f64 + (dh - mapped_h) * 0.5;

    let iy0 = (sh * INNER_H_MARGIN).round() as u32;
    let iy1 = occ.height.saturating_sub((sh * INNER_H_MARGIN).round() as u32);
    if iy1 <= iy0 {
        return false;
    }

    let occ_reject = (OCCUPANCY_REJECT * 255.0).ceil() as u8;

    for k in 1..slice_count {
        let sx = k * slice_w;
        if sx < slot.x || sx > slot.x + slot.w {
            continue;
        }
        let ox = ((sx as f64 - off_x) / scale).floor() as i32;
        for ox_check in [ox - 1, ox, ox + 1] {
            if ox_check < 0 || ox_check >= occ.width as i32 {
                continue;
            }
            let oxu = ox_check as u32;
            for oy in iy0..iy1 {
                let cy = off_y + (oy as f64 + 0.5) * scale;
                if cy < slot.y as f64
                    || cy >= slot.y as f64 + dh
                {
                    continue;
                }
                let idx = (oy * occ.width + oxu) as usize;
                if occ.occupied[idx] >= occ_reject {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether a figure pose passes placement rules (for tests).
pub fn pose_passes_figure_rules(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    papers: &[StripSlotDef],
    occ: &CanvasOccupancy,
    slice_w: i32,
    slice_count: i32,
    placed: &[(i32, i32, i32, i32)],
) -> bool {
    if occ.mean_in_canvas_rect(x, y, w, h) > OCCUPANCY_REJECT {
        return false;
    }
    if slices_cut_inner_band(x, y, w, h, slice_w, slice_count) {
        return false;
    }
    if !bottom_band_ok(x, y, w, h, papers, occ) {
        return false;
    }
    if figure_figure_overlap_too_high(x, y, w, h, placed) {
        return false;
    }
    true
}
