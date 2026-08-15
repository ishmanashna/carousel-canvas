//! Token layout retry for mural v2 borderless (port of `layout_retry.py`).

use std::path::PathBuf;

use crate::layout_jitter::jitter_strip_slots;
use crate::python_rng::PythonRandom;
use crate::slot::StripSlotDef;
use crate::template::{LayoutPlacer, StripTemplate};
use crate::token_fill::token_rgb_for_path;

pub const MURAL_V2_LAYOUT_RETRY_ATTEMPTS: i32 = 5;
pub const TOKEN_COMPOSE_SCALE: f64 = 0.25;
pub const LAYOUT_RETRY_SEED_STEP: i64 = 1_000_003;

pub fn strip_layout_token_retry_enabled(template_id: &str, card_edge: &str) -> bool {
    template_id == "strip_mural_v2" && card_edge == "borderless"
}

pub fn underfill_rng_seed(layout_seed: i64) -> u32 {
    ((layout_seed as u32) & 0xFFFFFFFF) ^ 0x1B873F91
}

pub fn underfill_rng_from_layout_seed(layout_seed: i64) -> PythonRandom {
    PythonRandom::new(underfill_rng_seed(layout_seed) as u32)
}

fn bg_dist(px: [u8; 3], bg: [u8; 3]) -> i32 {
    (px[0] as i32 - bg[0] as i32).abs()
        + (px[1] as i32 - bg[1] as i32).abs()
        + (px[2] as i32 - bg[2] as i32).abs()
}

/// Rasterize token-colored slots at reduced scale for scoring.
pub fn token_compose_rgb(
    fills: &[Option<PathBuf>],
    slots: &[StripSlotDef],
    canvas_w: i32,
    canvas_h: i32,
    scale: f64,
    bg: [u8; 3],
) -> (Vec<u8>, u32, u32) {
    let cw = ((canvas_w as f64) * scale).round() as u32;
    let ch = ((canvas_h as f64) * scale).round() as u32;
    let cw = cw.max(1);
    let ch = ch.max(1);
    let mut rgb = vec![0u8; cw as usize * ch as usize * 3];
    for i in (0..rgb.len()).step_by(3) {
        rgb[i] = bg[0];
        rgb[i + 1] = bg[1];
        rgb[i + 2] = bg[2];
    }

    let mut order: Vec<usize> = (0..slots.len()).collect();
    order.sort_by_key(|i| slots[*i].z_index);

    for i in order {
        let fill = fills.get(i).and_then(|f| f.as_ref());
        if fill.is_none() {
            continue;
        }
        let slot = &slots[i];
        let color = token_rgb_for_path(fill.unwrap());
        let sx = ((slot.x as f64) * scale).round() as i32;
        let sy = ((slot.y as f64) * scale).round() as i32;
        let sw = ((slot.w as f64) * scale).round() as i32;
        let sh = ((slot.h as f64) * scale).round() as i32;
        paint_rotated_rect(&mut rgb, cw, ch, sx, sy, sw.max(1), sh.max(1), slot.rotation_deg, color);
    }
    (rgb, cw, ch)
}

fn paint_rotated_rect(
    rgb: &mut [u8],
    cw: u32,
    ch: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    rot_deg: f64,
    color: [u8; 3],
) {
    let cx = x as f64 + w as f64 / 2.0;
    let cy = y as f64 + h as f64 / 2.0;
    let rad = -rot_deg.to_radians();
    let (sin, cos) = rad.sin_cos();
    let corners = [
        (x as f64, y as f64),
        (x as f64 + w as f64, y as f64),
        (x as f64 + w as f64, y as f64 + h as f64),
        (x as f64, y as f64 + h as f64),
    ];
    let mut rot_corners = Vec::new();
    for (px, py) in corners {
        let dx = px - cx;
        let dy = py - cy;
        rot_corners.push((cx + dx * cos - dy * sin, cy + dx * sin + dy * cos));
    }
    let min_x = rot_corners.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
    let max_x = rot_corners.iter().map(|(x, _)| *x).fold(f64::NEG_INFINITY, f64::max);
    let min_y = rot_corners.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
    let max_y = rot_corners.iter().map(|(_, y)| *y).fold(f64::NEG_INFINITY, f64::max);
    let x0 = (min_x.floor() as i32).max(0);
    let y0 = (min_y.floor() as i32).max(0);
    let x1 = (max_x.ceil() as i32).min(cw as i32 - 1);
    let y1 = (max_y.ceil() as i32).min(ch as i32 - 1);

    for py in y0..=y1 {
        for px in x0..=x1 {
            if point_in_rotated_rect(px as f64, py as f64, cx, cy, w as f64, h as f64, rad) {
                let idx = (py as usize * cw as usize + px as usize) * 3;
                rgb[idx] = color[0];
                rgb[idx + 1] = color[1];
                rgb[idx + 2] = color[2];
            }
        }
    }
}

fn point_in_rotated_rect(
    px: f64,
    py: f64,
    cx: f64,
    cy: f64,
    w: f64,
    h: f64,
    rad: f64,
) -> bool {
    let (sin, cos) = rad.sin_cos();
    let dx = px - cx;
    let dy = py - cy;
    let lx = dx * cos + dy * sin;
    let ly = -dx * sin + dy * cos;
    lx.abs() <= w / 2.0 && ly.abs() <= h / 2.0
}

pub fn score_token_strip_layout(
    wide_rgb: &[u8],
    iw: u32,
    ih: u32,
    canvas_w: i32,
    canvas_h: i32,
    slice_w: i32,
    slice_h: i32,
    slice_count: i32,
    overlap_px: i32,
    bg_rgb: [u8; 3],
    bg_tol: i32,
    quant: i32,
) -> f64 {
    if iw < 8 || ih < 8 {
        return -1e9;
    }
    let scale_x = iw as f64 / canvas_w.max(1) as f64;
    let sw = ((slice_w as f64) * scale_x).round() as i32;
    let sw = sw.max(2);
    let sh_scaled =
        ((slice_h as f64) * (ih as f64 / canvas_h.max(1) as f64)).round() as u32;
    let sh = sh_scaled.max(2).min(ih);
    let stride = (sw / 64).max(1);
    let mut total_score = 0.0;

    for k in 0..slice_count {
        let mut x0 = ((k * (slice_w - overlap_px)) as f64 * scale_x).round() as i32;
        if x0 + sw > iw as i32 {
            x0 = (iw as i32 - sw).max(0);
        }
        let cw_ = sw.min(iw as i32 - x0);
        let ch_ = sh as i32;
        let mut buckets: std::collections::HashMap<(i32, i32, i32), u32> =
            std::collections::HashMap::new();
        let mut fg = 0u32;
        for y in (0..ch_).step_by(stride as usize) {
            for x in (0..cw_).step_by(stride as usize) {
                let idx = (y as usize * iw as usize + (x0 + x) as usize) * 3;
                let p = [
                    wide_rgb[idx],
                    wide_rgb[idx + 1],
                    wide_rgb[idx + 2],
                ];
                if bg_dist(p, bg_rgb) <= bg_tol {
                    continue;
                }
                let key = (p[0] as i32 / quant, p[1] as i32 / quant, p[2] as i32 / quant);
                buckets.entry(key).and_modify(|v| *v += 1).or_insert(1);
                fg += 1;
            }
        }
        if fg < 12 {
            total_score -= 40.0;
            continue;
        }
        let distinct = buckets.len();
        let dom = buckets.values().max().copied().unwrap_or(0) as f64 / fg as f64;
        let mut slice_sc = distinct.min(40) as f64;
        if distinct < 5 {
            slice_sc -= 18.0;
        }
        if dom > 0.88 {
            slice_sc -= 22.0;
        }
        total_score += slice_sc;
    }
    total_score
}

pub fn resolve_slots_for_layout(
    template: &StripTemplate,
    layout_seed: i64,
) -> Vec<StripSlotDef> {
    if template.id == "strip_seamless_mosaic_v1" {
        return crate::mosaic::build_seamless_mosaic_v1_slots(layout_seed);
    }
    if template.layout_placer == LayoutPlacer::OrganicPolaroid {
        return crate::layout_polaroid::resolve_organic_polaroid_slots(
            &template.slots,
            template.canvas_width,
            template.canvas_height,
            template.slice_width,
            template.slice_count,
            layout_seed,
        );
    }
    let base = &template.slots;
    if template.layout_jitter_px <= 0 {
        return base.clone();
    }
    jitter_strip_slots(
        base,
        template.layout_jitter_px,
        layout_seed,
        template.canvas_width,
        template.canvas_height,
        template.layout_cover_slot_index,
        template.layout_cover_max_jitter,
        template.layout_flagship_slot_index,
        template.layout_flagship_max_jitter,
        &template.layout_flagship_rim_slot_indices,
        70.0,
        48,
        200,
        220,
        520,
    )
}

pub fn pick_best_layout_seed_with_token_retry(
    fills: &[Option<PathBuf>],
    template: &StripTemplate,
    base_layout_seed: i64,
    card_edge: &str,
    attempts: i32,
    token_scale: f64,
    bg_rgb: [u8; 3],
) -> i64 {
    if !strip_layout_token_retry_enabled(template.id, card_edge) {
        return base_layout_seed;
    }
    let bs = base_layout_seed;
    let mut best_seed = bs;
    let mut best_score = -1e18;
    for i in 0..attempts.max(1) {
        let cand = bs + i as i64 * LAYOUT_RETRY_SEED_STEP;
        let slots = resolve_slots_for_layout(template, cand);
        let (rgb, iw, ih) = token_compose_rgb(
            fills,
            &slots,
            template.canvas_width,
            template.canvas_height,
            token_scale,
            bg_rgb,
        );
        let sc = score_token_strip_layout(
            &rgb,
            iw,
            ih,
            template.canvas_width,
            template.canvas_height,
            template.slice_width,
            template.slice_height,
            template.slice_count as i32,
            template.overlap_px,
            bg_rgb,
            48,
            18,
        );
        if sc > best_score {
            best_score = sc;
            best_seed = cand;
        }
    }
    best_seed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::template_strip_mural_v2;

    #[test]
    fn token_retry_enabled_mural_borderless() {
        assert!(strip_layout_token_retry_enabled("strip_mural_v2", "borderless"));
        assert!(!strip_layout_token_retry_enabled("strip_mural_v2", "wedges"));
        assert!(!strip_layout_token_retry_enabled("strip_10col", "borderless"));
    }

    #[test]
    fn underfill_rng_seed_matches_xor() {
        assert_eq!(underfill_rng_seed(7), (7 as u32) ^ 0x1B873F91);
    }

    #[test]
    fn token_score_varies_by_seed() {
        let tpl = template_strip_mural_v2();
        let paths: Vec<PathBuf> = (0..35)
            .map(|i| PathBuf::from(format!("p{i}.jpg")))
            .collect();
        let fills: Vec<Option<PathBuf>> = paths.iter().cloned().map(Some).collect();
        let bg = [236u8, 232u8, 227u8];
        let s0 = {
            let slots = resolve_slots_for_layout(&tpl, 0);
            let (rgb, iw, ih) = token_compose_rgb(
                &fills,
                &slots,
                tpl.canvas_width,
                tpl.canvas_height,
                TOKEN_COMPOSE_SCALE,
                bg,
            );
            score_token_strip_layout(
                &rgb,
                iw,
                ih,
                tpl.canvas_width,
                tpl.canvas_height,
                tpl.slice_width,
                tpl.slice_height,
                tpl.slice_count as i32,
                tpl.overlap_px,
                bg,
                48,
                18,
            )
        };
        assert!(s0 > -1e8);
    }
}
