//! Seeded layout variation: flagship-first jitter for mural v2.

use crate::python_rng::PythonRandom;
use crate::slot::StripSlotDef;

fn intersection_area(
    ax: i32,
    ay: i32,
    aw: i32,
    ah: i32,
    bx: i32,
    by: i32,
    bw: i32,
    bh: i32,
) -> f64 {
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

fn clone_slot(s: &StripSlotDef, nx: i32, ny: i32) -> StripSlotDef {
    let mut out = s.clone();
    out.x = nx;
    out.y = ny;
    out
}

fn population_stdev(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let var = values.iter().map(|v| (*v - mean).powi(2)).sum::<f64>() / n;
    var.sqrt()
}

/// Port of `app.strip.layout_jitter.jitter_strip_slots`.
pub fn jitter_strip_slots(
    slots: &[StripSlotDef],
    jitter_px: i32,
    seed: i64,
    canvas_w: i32,
    canvas_h: i32,
    cover_slot_index: Option<usize>,
    cover_max_jitter: i32,
    flagship_slot_index: Option<usize>,
    flagship_max_jitter: i32,
    flagship_rim_indices: &[usize],
    min_y_spread: f64,
    max_attempts: usize,
    vertical_bleed_top: i32,
    vertical_bleed_bottom: i32,
    horizontal_bleed: i32,
) -> Vec<StripSlotDef> {
    if jitter_px <= 0 || slots.is_empty() {
        return slots.to_vec();
    }

    let n = slots.len();
    let seed_i = seed as i32;

    fn clamp_xy(
        s: &StripSlotDef,
        nx: i32,
        ny: i32,
        canvas_w: i32,
        canvas_h: i32,
        vertical_bleed_top: i32,
        vertical_bleed_bottom: i32,
        horizontal_bleed: i32,
    ) -> (i32, i32) {
        let low_x = -horizontal_bleed.min(s.w);
        let high_x = canvas_w - s.w + horizontal_bleed.min(s.w / 2);
        let nx = nx.max(low_x).min(high_x);
        let ny = ny
            .max(-vertical_bleed_top)
            .min(canvas_h - s.h + vertical_bleed_bottom);
        (nx, ny)
    }

    // Flagship-first path (mural v2).
    if let Some(fsi) = flagship_slot_index {
        if fsi < n {
            let defer: std::collections::HashSet<usize> = flagship_rim_indices
                .iter()
                .copied()
                .chain(std::iter::once(fsi))
                .collect();
            let indexed: Vec<(usize, &StripSlotDef)> = slots
                .iter()
                .enumerate()
                .filter(|(i, _)| !defer.contains(i))
                .collect();
            let m = indexed.len();
            let mut rank_by: std::collections::HashMap<usize, usize> =
                std::collections::HashMap::new();
            let mut sorted_indexed = indexed.clone();
            sorted_indexed.sort_by(|a, b| {
                let cy_a = a.1.y as f64 + a.1.h as f64 / 2.0;
                let cy_b = b.1.y as f64 + b.1.h as f64 / 2.0;
                cy_a.partial_cmp(&cy_b).unwrap_or(std::cmp::Ordering::Equal)
            });
            for (rank, (orig_i, _)) in sorted_indexed.iter().enumerate() {
                rank_by.insert(*orig_i, rank);
            }
            let bias_amp = jitter_px as f64 + 36.0;

            fn score_layout_local(tpl: &[StripSlotDef], min_y_spread: f64) -> f64 {
                let cy: Vec<f64> = tpl.iter().map(|s| s.y as f64 + s.h as f64 / 2.0).collect();
                let cx: Vec<f64> = tpl.iter().map(|s| s.x as f64 + s.w as f64 / 2.0).collect();
                let span = if cy.is_empty() {
                    0.0
                } else {
                    cy.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                        - cy.iter().cloned().fold(f64::INFINITY, f64::min)
                };
                let ps = population_stdev(&cy);
                if span + ps < 1e-6 {
                    return -1e9;
                }
                let mut pen = 0.0;
                let dmin = 140.0;
                for i in 0..tpl.len() {
                    for j in (i + 1)..tpl.len() {
                        let d = ((cx[i] - cx[j]).powi(2) + (cy[i] - cy[j]).powi(2)).sqrt();
                        if d < dmin {
                            pen += dmin - d;
                        }
                    }
                }
                let mut raw = span + 0.5 * ps - 0.28 * pen;
                if ps < min_y_spread {
                    raw -= (min_y_spread - ps) * 2.5;
                }
                raw
            }

            let mut best_cand: Option<Vec<StripSlotDef>> = None;
            let mut best_s = -1e18;
            for attempt in 0..max_attempts {
                let mut rng =
                    PythonRandom::new(seed_i.wrapping_add(attempt as i32 * 1_000_003) as u32);
                let mut cand: Vec<Option<StripSlotDef>> = vec![None; n];
                for i in defer.iter() {
                    let s = &slots[*i];
                    cand[*i] = Some(clone_slot(s, s.x, s.y));
                }
                for (orig_i, s) in &indexed {
                    let rank = rank_by[orig_i];
                    let t = rank as f64 / (m.max(1) - 1) as f64;
                    let strat_y = ((t * 2.0 - 1.0) * bias_amp) as i32;
                    let dy = strat_y + rng.randint(-jitter_px, jitter_px);
                    let dx = rng.randint(-jitter_px, jitter_px);
                    let (nx, ny) = clamp_xy(
                        s,
                        s.x + dx,
                        s.y + dy,
                        canvas_w,
                        canvas_h,
                        vertical_bleed_top,
                        vertical_bleed_bottom,
                        horizontal_bleed,
                    );
                    cand[*orig_i] = Some(clone_slot(s, nx, ny));
                }
                let cand_vec: Vec<StripSlotDef> = cand.into_iter().map(|o| o.unwrap()).collect();
                let sc = score_layout_local(&cand_vec, min_y_spread);
                if sc > best_s {
                    best_s = sc;
                    best_cand = Some(cand_vec);
                }
            }

            let mut out = best_cand.unwrap_or_else(|| slots.to_vec());

            let sf = &slots[fsi];
            let mut rng_f = PythonRandom::new(seed_i.wrapping_add(602_167) as u32);
            let fj = flagship_max_jitter.max(0);
            let fdx = if fj > 0 { rng_f.randint(-fj, fj) } else { 0 };
            let fdy = if fj > 0 { rng_f.randint(-fj, fj) } else { 0 };
            let (fx, fy) = clamp_xy(
                sf,
                sf.x + fdx,
                sf.y + fdy,
                canvas_w,
                canvas_h,
                vertical_bleed_top,
                vertical_bleed_bottom,
                horizontal_bleed,
            );
            out[fsi] = clone_slot(sf, fx, fy);

            let rims: Vec<usize> = flagship_rim_indices
                .iter()
                .copied()
                .filter(|&i| i < n && i != fsi)
                .collect();
            for (ri, ridx) in rims.iter().enumerate() {
                let rim = &slots[*ridx];
                let mut rng_r =
                    PythonRandom::new(seed_i.wrapping_add(7711 + ri as i32 * 97) as u32);
                let rx = rng_r.randint(-7, 7);
                let ry = rng_r.randint(-7, 7);
                let rrot = rng_r.uniform(-1.15, 1.15);
                let (nx, ny) = if ri == 0 {
                    let nx = fx + sf.w - rim.w + 48 + rx;
                    let ny = fy + 62 + ry;
                    clamp_xy(
                        rim,
                        nx,
                        ny,
                        canvas_w,
                        canvas_h,
                        vertical_bleed_top,
                        vertical_bleed_bottom,
                        horizontal_bleed,
                    )
                } else {
                    let nx = fx - 42 + rx;
                    let ny = fy + sf.h - rim.h - 28 + ry;
                    clamp_xy(
                        rim,
                        nx,
                        ny,
                        canvas_w,
                        canvas_h,
                        vertical_bleed_top,
                        vertical_bleed_bottom,
                        horizontal_bleed,
                    )
                };
                let mut rim_slot = rim.clone();
                rim_slot.x = nx;
                rim_slot.y = ny;
                rim_slot.rotation_deg += rrot;
                out[*ridx] = rim_slot;
            }
            return out;
        }
    }

    // Cover-aware greedy path.
    if let Some(csi) = cover_slot_index {
        if csi < n {
            let mut placed: Vec<(i32, i32, i32, i32)> = Vec::new();
            let mut out: Vec<Option<StripSlotDef>> = vec![None; n];

            let s_cov = &slots[csi];
            let mut rng0 = PythonRandom::new(seed_i.wrapping_add(404_231) as u32);
            let cdx = rng0.randint(-cover_max_jitter, cover_max_jitter);
            let cdy = rng0.randint(-cover_max_jitter, cover_max_jitter);
            let (cx, cy) = clamp_xy(
                s_cov,
                s_cov.x + cdx,
                s_cov.y + cdy,
                canvas_w,
                canvas_h,
                vertical_bleed_top,
                vertical_bleed_bottom,
                horizontal_bleed,
            );
            placed.push((cx, cy, s_cov.w, s_cov.h));
            out[csi] = Some(clone_slot(s_cov, cx, cy));

            let mut others: Vec<usize> = (0..n).filter(|i| *i != csi).collect();
            others.sort_by_key(|i| slots[*i].z_index);
            others.reverse();

            let mut ranked: Vec<usize> = others.clone();
            ranked.sort_by_key(|i| slots[*i].y + slots[*i].h / 2);
            let rank_of: std::collections::HashMap<usize, usize> = ranked
                .iter()
                .enumerate()
                .map(|(r, &idx)| (idx, r))
                .collect();
            let bias_amp = jitter_px as f64 + 32.0;
            let denom = others.len().max(1) - 1;

            for orig_i in others {
                let s = &slots[orig_i];
                let rank = rank_of[&orig_i];
                let t = rank as f64 / denom as f64;
                let strat_y = ((t * 2.0 - 1.0) * bias_amp) as i32;
                let mut best_sc = 1e30;
                let mut best_xy = (s.x, s.y);
                for k in 0..56 {
                    let mut rng = PythonRandom::new(
                        seed_i.wrapping_add(orig_i as i32 * 7919 + k as i32 * 131) as u32,
                    );
                    let dy = strat_y + rng.randint(-jitter_px, jitter_px);
                    let dx = rng.randint(-jitter_px, jitter_px);
                    let (nx, ny) = clamp_xy(
                        s,
                        s.x + dx,
                        s.y + dy,
                        canvas_w,
                        canvas_h,
                        vertical_bleed_top,
                        vertical_bleed_bottom,
                        horizontal_bleed,
                    );
                    let oa = placed
                        .iter()
                        .map(|(px, py, pw, ph)| {
                            intersection_area(nx, ny, s.w, s.h, *px, *py, *pw, *ph)
                        })
                        .sum::<f64>();
                    let dist = 0.11 * ((nx - s.x).abs() + (ny - s.y).abs()) as f64;
                    let sc = oa + dist;
                    if sc < best_sc {
                        best_sc = sc;
                        best_xy = (nx, ny);
                    }
                }
                out[orig_i] = Some(clone_slot(s, best_xy.0, best_xy.1));
                placed.push((best_xy.0, best_xy.1, s.w, s.h));
            }
            return out.into_iter().map(|o| o.unwrap()).collect();
        }
    }

    // Fallback: stratified + global score.
    let mut indexed: Vec<(usize, &StripSlotDef)> = slots.iter().enumerate().collect();
    indexed.sort_by_key(|(_, s)| s.y + s.h / 2);
    let mut rank_by_orig: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
    for (rank, (orig_i, _)) in indexed.iter().enumerate() {
        rank_by_orig.insert(*orig_i, rank);
    }
    let bias_amp = jitter_px as f64 + 36.0;

    fn score_layout(tpl: &[StripSlotDef], min_y_spread: f64) -> f64 {
        let cy: Vec<f64> = tpl.iter().map(|s| s.y as f64 + s.h as f64 / 2.0).collect();
        let cx: Vec<f64> = tpl.iter().map(|s| s.x as f64 + s.w as f64 / 2.0).collect();
        let span = if cy.is_empty() {
            0.0
        } else {
            cy.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                - cy.iter().cloned().fold(f64::INFINITY, f64::min)
        };
        let ps = population_stdev(&cy);
        if span + ps < 1e-6 {
            return -1e9;
        }
        let mut pen = 0.0;
        let dmin = 140.0;
        for i in 0..tpl.len() {
            for j in (i + 1)..tpl.len() {
                let d = ((cx[i] - cx[j]).powi(2) + (cy[i] - cy[j]).powi(2)).sqrt();
                if d < dmin {
                    pen += dmin - d;
                }
            }
        }
        let mut raw = span + 0.5 * ps - 0.28 * pen;
        if ps < min_y_spread {
            raw -= (min_y_spread - ps) * 2.5;
        }
        raw
    }

    let mut best: Option<Vec<StripSlotDef>> = None;
    let mut best_s = -1e18;
    for attempt in 0..max_attempts {
        let mut rng =
            PythonRandom::new(seed_i.wrapping_add(attempt as i32 * 1_000_003) as u32);
        let mut cand: Vec<StripSlotDef> = Vec::with_capacity(n);
        for (orig_i, s) in slots.iter().enumerate() {
            let rank = rank_by_orig[&orig_i];
            let t = rank as f64 / (n.max(1) - 1) as f64;
            let strat_y = ((t * 2.0 - 1.0) * bias_amp) as i32;
            let dy = strat_y + rng.randint(-jitter_px, jitter_px);
            let dx = rng.randint(-jitter_px, jitter_px);
            let (nx, ny) = clamp_xy(
                s,
                s.x + dx,
                s.y + dy,
                canvas_w,
                canvas_h,
                vertical_bleed_top,
                vertical_bleed_bottom,
                horizontal_bleed,
            );
            cand.push(clone_slot(s, nx, ny));
        }
        let sc = score_layout(&cand, min_y_spread);
        if sc > best_s {
            best_s = sc;
            best = Some(cand);
        }
    }
    best.unwrap_or_else(|| slots.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slot::StripSlotDef;

    #[test]
    fn jitter_is_deterministic() {
        let slots = vec![
            StripSlotDef::new(100, 100, 200, 200).with_z(0),
            StripSlotDef::new(500, 200, 300, 300).with_z(1),
        ];
        let a = jitter_strip_slots(
            &slots,
            20,
            7,
            10800,
            1350,
            None,
            14,
            None,
            18,
            &[],
            70.0,
            48,
            200,
            220,
            520,
        );
        let b = jitter_strip_slots(
            &slots,
            20,
            7,
            10800,
            1350,
            None,
            14,
            None,
            18,
            &[],
            70.0,
            48,
            200,
            220,
            520,
        );
        assert_eq!(a.len(), b.len());
        for (sa, sb) in a.iter().zip(b.iter()) {
            assert_eq!(sa.x, sb.x);
            assert_eq!(sa.y, sb.y);
        }
    }
}
