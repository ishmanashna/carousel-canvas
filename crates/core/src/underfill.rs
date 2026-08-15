//! Beige-pocket underfill planner (port of `app/strip/underfill.py`).

use std::path::PathBuf;

use crate::python_rng::PythonRandom;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnderfillBox {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub rotation_deg: f64,
}

fn color_distance_sum(rgb: &[u8], bg: [u8; 3]) -> i32 {
    let r = rgb[0] as i32;
    let g = rgb[1] as i32;
    let b = rgb[2] as i32;
    (r - bg[0] as i32).abs() + (g - bg[1] as i32).abs() + (b - bg[2] as i32).abs()
}

/// Fast downscale of full-canvas RGB (Triangle filter via `image`).
pub fn downscale_rgb(rgb: &[u8], cw: u32, ch: u32, max_w: u32) -> (Vec<u8>, u32, u32) {
    if cw <= max_w {
        return (rgb.to_vec(), cw, ch);
    }
    let nh = ((ch as f64) * max_w as f64 / cw as f64).round() as u32;
    let nh = nh.max(1);
    let img = image::RgbImage::from_raw(cw, ch, rgb.to_vec())
        .expect("rgb buffer size matches canvas");
    let resized = image::imageops::resize(
        &img,
        max_w,
        nh,
        image::imageops::FilterType::Triangle,
    );
    (resized.into_raw(), max_w, nh)
}

fn pick_gap_anchor(
    rgb: &[u8],
    sw: u32,
    sh: u32,
    bg: [u8; 3],
    tolerance: i32,
    gw: u32,
    gh: u32,
) -> Option<(i32, i32)> {
    let mut buckets: std::collections::HashMap<(u32, u32), u32> = std::collections::HashMap::new();
    let sx = sw as f64 / gw as f64;
    let sy = sh as f64 / gh as f64;
    let min_hits = ((sw * sh) / 6000).max(16);
    for y in 0..sh {
        for x in 0..sw {
            let idx = (y as usize * sw as usize + x as usize) * 3;
            let px = &rgb[idx..idx + 3];
            if color_distance_sum(px, bg) <= tolerance {
                let bx = ((x as f64 / sx) as u32).min(gw - 1);
                let by = ((y as f64 / sy) as u32).min(gh - 1);
                buckets.entry((bx, by)).and_modify(|v| *v += 1).or_insert(1);
            }
        }
    }
    if buckets.is_empty() {
        return None;
    }
    let (best_k, best_v) = buckets.iter().max_by_key(|(_, v)| *v).unwrap();
    if *best_v < min_hits {
        return None;
    }
    let cx = ((best_k.0 as f64 + 0.5) * sx) as i32;
    let cy = ((best_k.1 as f64 + 0.5) * sy) as i32;
    Some((cx, cy))
}

fn anchor_score(rgb: &[u8], sw: u32, sh: u32, cx: i32, cy: i32, bg: [u8; 3], tolerance: i32) -> i32 {
    let r = 22;
    let x0 = (cx - r).max(0);
    let x1 = (cx + r).min(sw as i32 - 1);
    let y0 = (cy - r).max(0);
    let y1 = (cy + r).min(sh as i32 - 1);
    let mut s = 0;
    for yy in y0..=y1 {
        for xx in x0..=x1 {
            let idx = (yy as usize * sw as usize + xx as usize) * 3;
            let px = &rgb[idx..idx + 3];
            if color_distance_sum(px, bg) <= tolerance {
                s += 1;
            }
        }
    }
    s
}

fn pick_edge_anchor(
    rgb: &[u8],
    sw: u32,
    sh: u32,
    bg: [u8; 3],
    tolerance: i32,
) -> Option<(i32, i32)> {
    if sw < 8 || sh < 8 {
        return None;
    }
    let band = ((sw as f64 * 0.23).round() as i32).max(1);
    let left_w = band.min(sw as i32);
    let right_x = (sw as i32 - band).max(0);
    let a_left = pick_gap_anchor(rgb, left_w as u32, sh, bg, tolerance, 22, 5);
    let right_rgb = crop_rgb(rgb, sw, sh, right_x, 0, sw as i32 - right_x, sh as i32);
    let a_right = pick_gap_anchor(
        &right_rgb,
        sw as u32 - right_x as u32,
        sh,
        bg,
        tolerance,
        22,
        5,
    );
    match (a_left, a_right) {
        (None, None) => None,
        (Some(l), None) => Some(l),
        (None, Some(r)) => Some((r.0 + right_x, r.1)),
        (Some((lx, ly)), Some((rx, ry))) => {
            let rx_full = rx + right_x;
            let ls = anchor_score(rgb, sw, sh, lx, ly, bg, tolerance);
            let rs = anchor_score(rgb, sw, sh, rx_full, ry, bg, tolerance);
            if rs >= ls {
                Some((rx_full, ry))
            } else {
                Some((lx, ly))
            }
        }
    }
}

fn crop_rgb(
    rgb: &[u8],
    cw: u32,
    ch: u32,
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
) -> Vec<u8> {
    let mut out = vec![0u8; w as usize * h as usize * 3];
    for y in 0..h {
        for x in 0..w {
            let sx = x0 + x;
            let sy = y0 + y;
            if sx >= 0 && sy >= 0 && sx < cw as i32 && sy < ch as i32 {
                let src = (sy as usize * cw as usize + sx as usize) * 3;
                let dst = (y as usize * w as usize + x as usize) * 3;
                out[dst..dst + 3].copy_from_slice(&rgb[src..src + 3]);
            }
        }
    }
    out
}

fn blackout_rect(work: &mut [u8], cw: u32, ch: u32, x1: i32, y1: i32, x2: i32, y2: i32) {
    let x1 = x1.max(0);
    let y1 = y1.max(0);
    let x2 = x2.min(cw as i32 - 1);
    let y2 = y2.min(ch as i32 - 1);
    for y in y1..=y2 {
        for x in x1..=x2 {
            let idx = (y as usize * cw as usize + x as usize) * 3;
            work[idx] = 0;
            work[idx + 1] = 0;
            work[idx + 2] = 0;
        }
    }
}

fn underfill_box_size(cw: i32, ch: i32, fine: bool) -> (i32, i32) {
    if cw >= 9000 {
        if fine {
            let bw = (cw as f64 * 0.072).round() as i32;
            let bh = (ch as f64 * 0.44).round() as i32;
            (
                bw.clamp(320, 760),
                bh.clamp(240, 620),
            )
        } else {
            let bw = (cw as f64 * 0.092).round() as i32;
            let bh = (ch as f64 * 0.54).round() as i32;
            (
                bw.clamp(420, 980),
                bh.clamp(320, 760),
            )
        }
    } else if fine {
        let bw = (cw as f64 * 0.068).round() as i32;
        let bh = (ch as f64 * 0.38).round() as i32;
        (
            bw.clamp(240, 620),
            bh.clamp(200, 520),
        )
    } else {
        let bw = (cw as f64 * 0.082).round() as i32;
        let bh = (ch as f64 * 0.46).round() as i32;
        (
            bw.clamp(280, 760),
            bh.clamp(220, 640),
        )
    }
}

pub fn plan_background_underfill_boxes(
    base_rgb: &[u8],
    cw: u32,
    ch: u32,
    bg: [u8; 3],
    tolerance: i32,
    n: i32,
    layout_seed: i64,
    boost_n: i32,
) -> Vec<UnderfillBox> {
    if n <= 0 {
        return Vec::new();
    }
    let seed = layout_seed as i32;
    let mut rng = PythonRandom::new((seed.wrapping_add(612_903)) as u32);
    let cw_i = cw as i32;
    let ch_i = ch as i32;
    let mut placements = Vec::new();
    let total = n + boost_n.max(0);
    let n_coarse = ((n.max(1) as f64 * 0.6).round() as i32).max(1);
    let mut buffers = WorkBuffers::from_rgb(base_rgb, cw, ch, 620);
    let mut cur_max_w = 620u32;

    for i in 0..total {
        let in_boost = i >= n;
        let fine = i >= n_coarse;
        let max_w = if fine { 700 } else { 620 };
        if max_w != cur_max_w {
            buffers.refresh_small(max_w);
            cur_max_w = max_w;
        }
        let sw = buffers.sw;
        let sh = buffers.sh;
        let anchor = if in_boost {
            pick_edge_anchor(&buffers.small, sw, sh, bg, tolerance)
        } else {
            pick_gap_anchor(&buffers.small, sw, sh, bg, tolerance, 22, 5)
        };
        if anchor.is_none() {
            break;
        }
        let (ax_s, ay_s) = anchor.unwrap();
        let cx_full = ((ax_s as f64 * cw_i as f64) / sw.max(1) as f64).round() as i32;
        let cy_full = ((ay_s as f64 * ch_i as f64) / sh.max(1) as f64).round() as i32;
        let (bw, bh) = underfill_box_size(cw_i, ch_i, fine);
        let jitter_x = if fine { 32 } else { 52 };
        let jitter_y = if fine { 26 } else { 44 };
        let sx0 = (cx_full - bw / 2 + rng.randint(-jitter_x, jitter_x))
            .max(0)
            .min(cw_i - bw);
        let sy0 = (cy_full - bh / 2 + rng.randint(-jitter_y, jitter_y))
            .max(0)
            .min(ch_i - bh);
        let rot = if in_boost {
            rng.uniform(-1.55, 1.55)
        } else if fine {
            rng.uniform(-1.95, 1.95)
        } else {
            rng.uniform(-2.35, 2.35)
        };
        placements.push(UnderfillBox {
            x: sx0,
            y: sy0,
            w: bw,
            h: bh,
            rotation_deg: rot,
        });
        let block_pad = bw.max(bh) / (if fine { 3 } else { 2 });
        buffers.blackout_centre(cx_full, cy_full, block_pad);
    }
    placements
}

pub fn plan_background_underfill_repeat_boxes(
    base_rgb: &[u8],
    cw: u32,
    ch: u32,
    bg: [u8; 3],
    tolerance: i32,
    n: i32,
    layout_seed: i64,
    beige_tolerance_bonus: i32,
) -> Vec<UnderfillBox> {
    if n <= 0 {
        return Vec::new();
    }
    let seed = layout_seed as i32;
    let mut rng = PythonRandom::new((seed.wrapping_add(918_221)) as u32);
    let cw_i = cw as i32;
    let ch_i = ch as i32;
    let mut placements = Vec::new();
    let tol = tolerance + beige_tolerance_bonus.max(0);
    let mut buffers = WorkBuffers::from_rgb(base_rgb, cw, ch, 760);

    for _ in 0..n {
        let sw = buffers.sw;
        let sh = buffers.sh;
        let anchor = pick_edge_anchor(&buffers.small, sw, sh, bg, tol)
            .or_else(|| pick_gap_anchor(&buffers.small, sw, sh, bg, tol, 22, 5));
        if anchor.is_none() {
            break;
        }
        let (ax_s, ay_s) = anchor.unwrap();
        let cx_full = ((ax_s as f64 * cw_i as f64) / sw.max(1) as f64).round() as i32;
        let cy_full = ((ay_s as f64 * ch_i as f64) / sh.max(1) as f64).round() as i32;
        let (bw, bh) = if cw_i >= 9000 {
            (
                ((cw_i as f64 * 0.178).round() as i32).clamp(720, 1980),
                ((ch_i as f64 * 0.86).round() as i32).clamp(520, ch_i.min(1180) - 24),
            )
        } else {
            (
                ((cw_i as f64 * 0.125).round() as i32).clamp(440, 1180),
                ((ch_i as f64 * 0.62).round() as i32).clamp(340, 900),
            )
        };
        let sx0 = (cx_full - bw / 2 + rng.randint(-58, 58))
            .max(0)
            .min(cw_i - bw);
        let sy0 = (cy_full - bh / 2 + rng.randint(-44, 44))
            .max(0)
            .min(ch_i - bh);
        let rot = rng.uniform(-1.15, 1.15);
        placements.push(UnderfillBox {
            x: sx0,
            y: sy0,
            w: bw,
            h: bh,
            rotation_deg: rot,
        });
        let pad = (bw.max(bh) as f64 * 0.68).round() as i32;
        buffers.blackout_centre(cx_full, cy_full, pad);
    }
    placements
}

pub fn carousel_tail_band_x_range(
    canvas_width: i32,
    slice_width: i32,
    slice_count: i32,
    overlap_px: i32,
    tail_slice_count: i32,
) -> (i32, i32) {
    if tail_slice_count <= 0 {
        return (0, canvas_width);
    }
    let ts = tail_slice_count.min(slice_count);
    let stride = if overlap_px == 0 {
        slice_width
    } else {
        slice_width - overlap_px
    };
    let i0 = (slice_count - ts).max(0);
    let x0 = i0 * stride;
    let x0 = x0.clamp(0, (canvas_width - 1).max(0));
    (x0, canvas_width)
}

pub fn plan_background_tail_underfill_boxes(
    base_rgb: &[u8],
    cw: u32,
    ch: u32,
    bg: [u8; 3],
    tolerance: i32,
    n: i32,
    layout_seed: i64,
    tail_x0: i32,
    tail_x1: i32,
    beige_tolerance_bonus: i32,
) -> Vec<UnderfillBox> {
    if n <= 0 {
        return Vec::new();
    }
    let cw_i = cw as i32;
    let ch_i = ch as i32;
    let x0 = tail_x0.clamp(0, cw_i - 1);
    let x1 = tail_x1.clamp(x0 + 1, cw_i);
    let tail_w = x1 - x0;
    if tail_w < 24 {
        return Vec::new();
    }

    let seed = layout_seed as i32;
    let mut rng = PythonRandom::new((seed.wrapping_add(441_977)) as u32);
    let mut work = base_rgb.to_vec();
    let mut placements = Vec::new();
    let tol = tolerance + beige_tolerance_bonus.max(0);

    for _ in 0..n {
        let band = crop_rgb(&work, cw, ch, x0, 0, tail_w, ch_i);
        let (small, sw, sh) = downscale_rgb(&band, tail_w as u32, ch, 820);
        let anchor = pick_gap_anchor(&small, sw, sh, bg, tol, 22, 5)
            .or_else(|| pick_edge_anchor(&small, sw, sh, bg, tol));
        if anchor.is_none() {
            break;
        }
        let (ax_s, ay_s) = anchor.unwrap();
        let cx_band = ((ax_s as f64 * tail_w as f64) / sw.max(1) as f64).round() as i32;
        let cy_full = ((ay_s as f64 * ch_i as f64) / sh.max(1) as f64).round() as i32;
        let cx_full = x0 + cx_band;

        let (mut bw, mut bh) = if cw_i >= 9000 {
            (
                ((tail_w as f64 * 1.12).round() as i32).clamp(920, 2480),
                ((ch_i as f64 * 0.92).round() as i32).clamp(520, ch_i.min(1220) - 16),
            )
        } else {
            (
                ((tail_w as f64 * 1.08).round() as i32).clamp(520, 1400),
                ((ch_i as f64 * 0.72).round() as i32).clamp(360, 940),
            )
        };
        bw = bw.min(cw_i);
        bh = bh.min(ch_i);
        if tail_w <= 1400 {
            bw = bw.min(tail_w);
        }
        let sx0 = (cx_full - bw / 2 + rng.randint(-48, 48))
            .max(x0)
            .min(x1 - bw);
        let sx0 = sx0.clamp(0, cw_i - bw);
        let sy0 = (cy_full - bh / 2 + rng.randint(-40, 40))
            .max(0)
            .min(ch_i - bh);
        let rot = rng.uniform(-1.05, 1.05);
        placements.push(UnderfillBox {
            x: sx0,
            y: sy0,
            w: bw,
            h: bh,
            rotation_deg: rot,
        });
        let pad = (bw.max(bh) as f64 * 0.72).round() as i32;
        blackout_rect(
            &mut work,
            cw,
            ch,
            cx_full - pad,
            cy_full - pad,
            cx_full + pad,
            cy_full + pad,
        );
    }
    placements
}

fn blackout_for_box(work: &mut [u8], cw: u32, ch: u32, cx: i32, cy: i32, pad: i32) {
    blackout_rect(work, cw, ch, cx - pad, cy - pad, cx + pad, cy + pad);
}

struct WorkBuffers {
    full: Vec<u8>,
    small: Vec<u8>,
    cw: u32,
    ch: u32,
    sw: u32,
    sh: u32,
}

impl WorkBuffers {
    fn from_rgb(rgb: &[u8], cw: u32, ch: u32, max_w: u32) -> Self {
        let full = rgb.to_vec();
        let (small, sw, sh) = downscale_rgb(&full, cw, ch, max_w);
        Self {
            full,
            small,
            cw,
            ch,
            sw,
            sh,
        }
    }

    fn refresh_small(&mut self, max_w: u32) {
        let (small, sw, sh) = downscale_rgb(&self.full, self.cw, self.ch, max_w);
        self.small = small;
        self.sw = sw;
        self.sh = sh;
    }

    fn blackout_centre(&mut self, cx: i32, cy: i32, pad: i32) {
        blackout_for_box(&mut self.full, self.cw, self.ch, cx, cy, pad);
        let scale_x = self.sw as f64 / self.cw as f64;
        let scale_y = self.sh as f64 / self.ch as f64;
        let scx = (cx as f64 * scale_x).round() as i32;
        let scy = (cy as f64 * scale_y).round() as i32;
        let spad_x = (pad as f64 * scale_x).ceil() as i32 + 1;
        let spad_y = (pad as f64 * scale_y).ceil() as i32 + 1;
        blackout_for_box(&mut self.small, self.sw, self.sh, scx, scy, spad_x.max(spad_y));
    }
}

/// Full mural underfill pipeline: base+boost, repeat, tail on chained work buffer.
pub fn plan_mural_underfill_boxes(
    required_flat_rgb: &[u8],
    cw: u32,
    ch: u32,
    bg: [u8; 3],
    tolerance: i32,
    layout_seed: i64,
    n_base: i32,
    n_boost: i32,
    n_repeat: i32,
    n_tail: i32,
    tail_x0: i32,
    tail_x1: i32,
    tail_tol_bonus: i32,
) -> Vec<UnderfillBox> {
    let mut all = Vec::new();
    let mut work = required_flat_rgb.to_vec();

    let base = plan_background_underfill_boxes(
        required_flat_rgb,
        cw,
        ch,
        bg,
        tolerance,
        n_base,
        layout_seed,
        n_boost,
    );
    for b in &base {
        let cx = b.x + b.w / 2;
        let cy = b.y + b.h / 2;
        let fine = false; // pad uses coarse for blackout approximation
        let block_pad = b.w.max(b.h) / 2;
        blackout_for_box(&mut work, cw, ch, cx, cy, block_pad);
        let _ = fine;
    }
    all.extend(base);

    if n_repeat > 0 {
        let rep = plan_background_underfill_repeat_boxes(
            &work,
            cw,
            ch,
            bg,
            tolerance,
            n_repeat,
            layout_seed,
            38,
        );
        for b in &rep {
            let cx = b.x + b.w / 2;
            let cy = b.y + b.h / 2;
            let pad = (b.w.max(b.h) as f64 * 0.68).round() as i32;
            blackout_for_box(&mut work, cw, ch, cx, cy, pad);
        }
        all.extend(rep);
    }

    if n_tail > 0 {
        let tail = plan_background_tail_underfill_boxes(
            &work,
            cw,
            ch,
            bg,
            tolerance,
            n_tail,
            layout_seed,
            tail_x0,
            tail_x1,
            tail_tol_bonus,
        );
        all.extend(tail);
    }

    all
}

#[derive(Debug, Clone)]
pub struct PlannedUnderfill {
    pub boxes: Vec<UnderfillBox>,
    pub paths: Vec<PathBuf>,
}

/// Plan all passes and assign photo paths (base sequential, repeat/tail XOR RNG).
pub fn plan_and_assign_mural_underfill(
    required_flat_rgb: &[u8],
    cw: u32,
    ch: u32,
    bg: [u8; 3],
    tolerance: i32,
    layout_seed: i64,
    n_base: i32,
    n_boost: i32,
    n_repeat: i32,
    n_tail: i32,
    tail_x0: i32,
    tail_x1: i32,
    tail_tol_bonus: i32,
    uf_pool: &[PathBuf],
) -> PlannedUnderfill {
    if uf_pool.is_empty() {
        return PlannedUnderfill {
            boxes: Vec::new(),
            paths: Vec::new(),
        };
    }
    let mut work = required_flat_rgb.to_vec();
    let base_boxes = plan_background_underfill_boxes(
        required_flat_rgb,
        cw,
        ch,
        bg,
        tolerance,
        n_base,
        layout_seed,
        n_boost,
    );
    let mut paths = Vec::new();
    let use_n = base_boxes
        .len()
        .min(uf_pool.len())
        .min((n_base + n_boost.max(0)) as usize);
    for j in 0..use_n {
        paths.push(uf_pool[j].clone());
        let b = &base_boxes[j];
        let cx = b.x + b.w / 2;
        let cy = b.y + b.h / 2;
        let block_pad = b.w.max(b.h) / 2;
        blackout_for_box(&mut work, cw, ch, cx, cy, block_pad);
    }

    let repeat_boxes = if n_repeat > 0 {
        plan_background_underfill_repeat_boxes(
            &work,
            cw,
            ch,
            bg,
            tolerance,
            n_repeat,
            layout_seed,
            38,
        )
    } else {
        Vec::new()
    };
    let mut rrng =
        PythonRandom::new(((layout_seed as u32) ^ 0x5EED1EAF) as u32);
    for b in &repeat_boxes {
        let idx = rrng.randbelow(uf_pool.len() as u32) as usize;
        paths.push(uf_pool[idx].clone());
        let cx = b.x + b.w / 2;
        let cy = b.y + b.h / 2;
        let pad = (b.w.max(b.h) as f64 * 0.68).round() as i32;
        blackout_for_box(&mut work, cw, ch, cx, cy, pad);
    }

    let tail_boxes = if n_tail > 0 {
        plan_background_tail_underfill_boxes(
            &work,
            cw,
            ch,
            bg,
            tolerance,
            n_tail,
            layout_seed,
            tail_x0,
            tail_x1,
            tail_tol_bonus,
        )
    } else {
        Vec::new()
    };
    let mut trng = PythonRandom::new(((layout_seed as u32) ^ 0x7A11BEEF) as u32);
    for _ in &tail_boxes {
        let idx = trng.randbelow(uf_pool.len() as u32) as usize;
        paths.push(uf_pool[idx].clone());
    }

    let mut boxes = base_boxes;
    boxes.extend(repeat_boxes);
    boxes.extend(tail_boxes);
    PlannedUnderfill { boxes, paths }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_band_last_slice() {
        let (x0, x1) = carousel_tail_band_x_range(10800, 1080, 10, 0, 1);
        assert_eq!(x0, 9720);
        assert_eq!(x1, 10800);
    }

    #[test]
    fn downscale_reduces_width() {
        let cw = 800u32;
        let ch = 100u32;
        let mut rgb = vec![0u8; cw as usize * ch as usize * 3];
        for i in (0..rgb.len()).step_by(3) {
            rgb[i] = 236;
            rgb[i + 1] = 232;
            rgb[i + 2] = 227;
        }
        let (_, sw, _) = downscale_rgb(&rgb, cw, ch, 620);
        assert_eq!(sw, 620);
    }

    #[test]
    fn planner_finds_boxes_on_beige() {
        let cw = 400u32;
        let ch = 80u32;
        let bg = [236u8, 232u8, 227u8];
        let mut rgb = vec![0u8; cw as usize * ch as usize * 3];
        for i in (0..rgb.len()).step_by(3) {
            rgb[i] = bg[0];
            rgb[i + 1] = bg[1];
            rgb[i + 2] = bg[2];
        }
        for y in 20..45 {
            for x in 120..180 {
                let idx = (y * cw as usize + x) * 3;
                rgb[idx] = 40;
                rgb[idx + 1] = 40;
                rgb[idx + 2] = 40;
            }
        }
        let boxes = plan_background_underfill_boxes(&rgb, cw, ch, bg, 48, 1, 7, 0);
        assert!(!boxes.is_empty());
    }
}
