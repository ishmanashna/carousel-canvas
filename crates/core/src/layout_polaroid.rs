//! Organic polaroid scatter (port of `app.strip.polaroid_scatter`).

use crate::python_rng::PythonRandom;
use crate::slot::StripSlotDef;

fn balanced_tilt_pool(rng: &mut PythonRandom, count: usize) -> Vec<f64> {
    if count == 0 {
        return vec![];
    }
    let n_pos = count / 2;
    let n_neg = count - n_pos;
    let mut angles: Vec<f64> = Vec::with_capacity(count);

    fn draw_mag(rng: &mut PythonRandom) -> f64 {
        let tier = rng.random();
        if tier < 0.22 {
            rng.uniform(2.4, 5.2)
        } else if tier < 0.62 {
            rng.uniform(5.0, 10.5)
        } else {
            rng.uniform(9.0, 16.2)
        }
    }

    for _ in 0..n_pos {
        angles.push(draw_mag(rng));
    }
    for _ in 0..n_neg {
        angles.push(-draw_mag(rng));
    }
    let flats = (count / 5).max(1).min(4);
    for _ in 0..flats {
        let idx = rng.randbelow(angles.len() as u32) as usize;
        angles[idx] = rng.uniform(-2.8, 2.8);
    }
    for i in (1..angles.len()).rev() {
        let j = rng.randbelow(i as u32 + 1) as usize;
        angles.swap(i, j);
    }
    for _ in 0..angles.len() * 2 {
        for k in 0..angles.len().saturating_sub(1) {
            if angles[k] * angles[k + 1] > 0.0
                && angles[k].abs() > 2.5
                && angles[k + 1].abs() > 2.5
            {
                if rng.random() < 0.42 {
                    angles.swap(k, k + 1);
                }
                break;
            }
        }
    }
    angles
}

fn ensure_slice_coverage(
    rng: &mut PythonRandom,
    xs: &mut [i32],
    ys: &mut [i32],
    card_w: i32,
    card_h: i32,
    slice_w: i32,
    slice_count: usize,
    ch: i32,
    gy_lo: i32,
    gy_hi: i32,
    wild_indices: &[usize],
    min_px: i32,
) {
    let n = xs.len();

    fn max_horiz_in_slice(
        k: usize,
        xs: &[i32],
        ys: &[i32],
        card_w: i32,
        card_h: i32,
        slice_w: i32,
        ch: i32,
        n: usize,
    ) -> i32 {
        let lo = k as i32 * slice_w;
        let hi = (k as i32 + 1) * slice_w;
        let mut best = 0;
        for i in 0..n {
            let ix0 = lo.max(xs[i]);
            let ix1 = hi.min(xs[i] + card_w);
            if ix1 > ix0 && ys[i] < ch && ys[i] + card_h > 0 {
                best = best.max(ix1 - ix0);
            }
        }
        best
    }

    for k in 0..slice_count {
        if max_horiz_in_slice(k, xs, ys, card_w, card_h, slice_w, ch, n) >= min_px {
            continue;
        }
        if wild_indices.is_empty() {
            break;
        }
        let pick = wild_indices[rng.randbelow(wild_indices.len() as u32) as usize];
        let margin = rng.randint(18, 50);
        let lo = k as i32 * slice_w + margin;
        let hi = (k as i32 + 1) * slice_w - card_w - margin;
        let hi = if hi < lo { lo } else { hi };
        xs[pick] = if hi >= lo {
            rng.randint(lo, hi)
        } else {
            lo
        };
        ys[pick] = (ys[pick] + rng.randint(-35, 35))
            .max(gy_lo)
            .min(gy_hi);
    }
}

fn separate_wild_centers(
    rng: &mut PythonRandom,
    xs: &mut [i32],
    ys: &mut [i32],
    indices: &[usize],
    card_w: i32,
    card_h: i32,
    x_lo: i32,
    x_hi: i32,
    gy_lo: i32,
    gy_hi: i32,
    min_center_dist: f64,
    iters: usize,
) {
    if indices.len() < 2 {
        return;
    }
    for _ in 0..iters {
        let mut shuffled = indices.to_vec();
        rng.shuffle_usize(&mut shuffled);
        for &a in &shuffled {
            let axc = xs[a] as f64 + card_w as f64 * 0.5;
            let ayc = ys[a] as f64 + card_h as f64 * 0.5;
            for &b in &shuffled {
                if b <= a {
                    continue;
                }
                let bxc = xs[b] as f64 + card_w as f64 * 0.5;
                let byc = ys[b] as f64 + card_h as f64 * 0.5;
                let dx = axc - bxc;
                let dy = ayc - byc;
                let d = (dx * dx + dy * dy).sqrt();
                if d <= 1e-3 || d >= min_center_dist {
                    continue;
                }
                let push = (min_center_dist - d) * 0.38;
                let ux = dx / d * push;
                let uy = dy / d * push;
                xs[a] = (xs[a] as f64 + ux).round() as i32;
                ys[a] = (ys[a] as f64 + uy).round() as i32;
                xs[b] = (xs[b] as f64 - ux).round() as i32;
                ys[b] = (ys[b] as f64 - uy).round() as i32;
                xs[a] = xs[a].max(x_lo).min(x_hi);
                xs[b] = xs[b].max(x_lo).min(x_hi);
                ys[a] = ys[a].max(gy_lo).min(gy_hi);
                ys[b] = ys[b].max(gy_lo).min(gy_hi);
            }
        }
    }
}

fn clone_slot_at(s: &StripSlotDef, x: i32, y: i32, rot: f64) -> StripSlotDef {
    let mut out = s.clone();
    out.x = x;
    out.y = y;
    out.rotation_deg = rot;
    out
}

/// Port of `resolve_organic_polaroid_slots`.
pub fn resolve_organic_polaroid_slots(
    slots: &[StripSlotDef],
    canvas_width: i32,
    canvas_height: i32,
    slice_width: i32,
    slice_count: usize,
    seed: i64,
) -> Vec<StripSlotDef> {
    let n = slots.len();
    if n == 0 {
        return slots.to_vec();
    }
    let cw = canvas_width;
    let ch = canvas_height;
    let card_w = slots[0].w;
    let card_h = slots[0].h;
    for s in slots {
        if s.w != card_w || s.h != card_h {
            panic!("organic_polaroid expects uniform slot width/height");
        }
    }

    let mut rng = PythonRandom::new(((seed as u32) ^ 0xC0FFEE71) as u32);

    let mut xs = vec![0i32; n];
    let mut ys = vec![0i32; n];
    let mut rots = vec![0.0f64; n];

    let pad_y_bot = ((card_h as f64 * 0.14) as i32).max(88);
    let pad_y_top = ((card_h as f64 * 0.05) as i32).max(40);
    let mut gy_lo = pad_y_top.max(8);
    let mut gy_hi = ch - card_h - pad_y_bot;
    if gy_hi < gy_lo {
        gy_lo = 0;
        gy_hi = (ch - card_h).max(0);
    }

    // Slot 0: hero centered in first slice.
    let hm = 22;
    let cx = slice_width as f64 / 2.0;
    let cy = ch as f64 / 2.0;
    xs[0] = (cx - card_w as f64 / 2.0 + rng.uniform(-10.0, 10.0)).round() as i32;
    ys[0] = (cy - card_h as f64 / 2.0 + rng.uniform(-14.0, 14.0)).round() as i32;
    xs[0] = xs[0].max(hm).min(slice_width - card_w - hm);
    ys[0] = ys[0].max(hm).min(ch - card_h - hm);
    rots[0] = rng.uniform(-2.6, 2.6);

    let hero_cy = ys[0] as f64 + card_h as f64 * 0.5;

    // Slots 1..9: one anchor per carousel column.
    let n_anchor = 9.min(n - 1);
    for k in 1..=n_anchor {
        let lo = k as i32 * slice_width + rng.randint(22, 48);
        let hi = (k as i32 + 1) * slice_width - card_w - rng.randint(22, 48);
        let hi = if hi < lo { lo } else { hi };
        xs[k] = if hi >= lo {
            rng.randint(lo, hi)
        } else {
            lo
        };
        if k <= 5 {
            if rng.random() < 0.68 {
                let half = rng.randint(55, 130);
                let y_target =
                    (hero_cy - card_h as f64 * 0.5 + rng.randint(-half, half) as f64).round()
                        as i32;
                ys[k] = y_target.max(gy_lo).min(gy_hi);
            } else {
                ys[k] = rng.randint(gy_lo, gy_hi);
            }
        } else {
            ys[k] = rng.randint(gy_lo, gy_hi);
        }
        let base = rng.uniform(5.0, 10.2) * if k % 2 == 0 { -1.0 } else { 1.0 };
        rots[k] = base + rng.uniform(-2.0, 2.0);
        if k <= 3 {
            rots[k] = (rots[k] * 0.82 + rng.uniform(-1.2, 1.2)).max(-11.0).min(11.0);
        } else {
            rots[k] = rots[k].max(-14.0).min(14.0);
        }
    }

    let wild: Vec<usize> = (n_anchor + 1..n).collect();
    let upside: Vec<usize> = if wild.len() >= 2 {
        rng.sample_indices(wild.len(), 2)
            .into_iter()
            .map(|i| wild[i])
            .collect()
    } else {
        vec![]
    };

    let bleed_eligible: Vec<usize> = wild
        .iter()
        .copied()
        .filter(|i| !upside.contains(i))
        .collect();
    let n_bleed = 6.min(4.max(bleed_eligible.len() * 4 / 7));
    let bleed_set: Vec<usize> = if bleed_eligible.is_empty() {
        vec![]
    } else {
        let k = n_bleed.min(bleed_eligible.len());
        rng.sample_indices(bleed_eligible.len(), k)
            .into_iter()
            .map(|i| bleed_eligible[i])
            .collect()
    };

    let tilt_n = wild.len() - upside.len();
    let tilt_wild = balanced_tilt_pool(&mut rng, tilt_n);
    let mut tw = 0;
    let x_lo = (card_w as f64 * 0.04) as i32;
    let x_hi = cw - card_w - (card_w as f64 * 0.04) as i32;

    for &i in &wild {
        if upside.contains(&i) {
            rots[i] = 180.0 + rng.uniform(-3.2, 3.2);
        } else {
            rots[i] = tilt_wild[tw] + rng.uniform(-1.2, 1.2);
            tw += 1;
            rots[i] = rots[i].max(-16.5).min(16.5);
        }
    }

    let wild_strat: Vec<usize> = wild
        .iter()
        .copied()
        .filter(|i| !bleed_set.contains(i))
        .collect();
    let n_strat = wild_strat.len();
    let n_bands = (n_strat + 2) / 2;
    let n_bands = n_bands.max(5).min(9);
    let mut band_order: Vec<usize> = (0..n_bands).collect();
    rng.shuffle_usize(&mut band_order);

    let mut wild_strat_sorted = wild_strat.clone();
    wild_strat_sorted.sort_unstable();

    for (j, &i) in wild_strat_sorted.iter().enumerate() {
        xs[i] = rng.randint(x_lo, x_lo.max(x_hi));
        let bi = band_order[j % n_bands];
        let span = gy_hi - gy_lo + 1;
        let slice_y = gy_lo + (bi as i32 * span) / n_bands.max(1) as i32;
        let slice_y2 = gy_lo + ((bi as i32 + 1) * span) / n_bands.max(1) as i32;
        let slice_y2 = slice_y2.max(slice_y + (card_h as f64 * 0.22) as i32);
        ys[i] = rng.randint(slice_y, slice_y.max(slice_y2 - card_h));
        ys[i] = ys[i].max(gy_lo).min(gy_hi);
        if rng.random() < 0.34 {
            let bump = rng.choice_i32(&[-1, 1]) * rng.randint(10, 44);
            ys[i] = (ys[i] + bump).max(gy_lo).min(gy_hi);
        }
    }

    for &i in &bleed_set {
        xs[i] = rng.randint(x_lo, x_lo.max(x_hi));
        let y_top = -(card_h as f64 * 0.32) as i32;
        let y_bot = ch - (card_h as f64 * 0.36) as i32;
        ys[i] = rng.randint(y_top, y_top.max(y_bot));
    }

    let wild_non_bleed: Vec<usize> = wild
        .iter()
        .copied()
        .filter(|i| !bleed_set.contains(i))
        .collect();
    let min_d = 0.36 * card_w.min(card_h) as f64;
    separate_wild_centers(
        &mut rng,
        &mut xs,
        &mut ys,
        &wild_non_bleed,
        card_w,
        card_h,
        x_lo,
        x_hi,
        gy_lo,
        gy_hi,
        min_d,
        11,
    );

    ensure_slice_coverage(
        &mut rng,
        &mut xs,
        &mut ys,
        card_w,
        card_h,
        slice_width,
        slice_count,
        ch,
        gy_lo,
        gy_hi,
        &wild,
        160,
    );

    slots
        .iter()
        .enumerate()
        .map(|(i, s)| clone_slot_at(s, xs[i], ys[i], rots[i]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slot::StripSlotDef;
    use crate::template::template_strip_polaroid_table_v1;

    #[test]
    fn hero_stays_in_slice_zero() {
        let tpl = template_strip_polaroid_table_v1();
        let slots = resolve_organic_polaroid_slots(
            &tpl.slots,
            tpl.canvas_width,
            tpl.canvas_height,
            tpl.slice_width,
            tpl.slice_count,
            42,
        );
        let hero = &slots[0];
        assert!(hero.x >= 0);
        assert!(hero.x + hero.w <= tpl.slice_width);
        let cx = hero.x + hero.w / 2;
        assert!(cx >= 0 && cx <= tpl.slice_width);
    }

    #[test]
    fn layout_not_regular_grid() {
        let tpl = template_strip_polaroid_table_v1();
        let slots = resolve_organic_polaroid_slots(
            &tpl.slots,
            tpl.canvas_width,
            tpl.canvas_height,
            tpl.slice_width,
            tpl.slice_count,
            7,
        );
        let xs: Vec<i32> = slots.iter().map(|s| s.x).collect();
        let unique_x = xs.iter().collect::<std::collections::HashSet<_>>().len();
        assert!(unique_x > 8, "expected scattered x positions, got {unique_x}");
        let ys: Vec<i32> = slots.iter().map(|s| s.y).collect();
        let y_span = ys.iter().max().unwrap() - ys.iter().min().unwrap();
        assert!(y_span > 200);
    }

    #[test]
    fn deterministic_for_seed() {
        let tpl = template_strip_polaroid_table_v1();
        let a = resolve_organic_polaroid_slots(
            &tpl.slots,
            tpl.canvas_width,
            tpl.canvas_height,
            tpl.slice_width,
            tpl.slice_count,
            99,
        );
        let b = resolve_organic_polaroid_slots(
            &tpl.slots,
            tpl.canvas_width,
            tpl.canvas_height,
            tpl.slice_width,
            tpl.slice_count,
            99,
        );
        for (sa, sb) in a.iter().zip(b.iter()) {
            assert_eq!(sa.x, sb.x);
            assert_eq!(sa.y, sb.y);
            assert!((sa.rotation_deg - sb.rotation_deg).abs() < 1e-9);
        }
    }

    #[test]
    fn uniform_card_size_required() {
        let slots = vec![
            StripSlotDef::new(0, 0, 100, 100),
            StripSlotDef::new(0, 0, 200, 100),
        ];
        let result = std::panic::catch_unwind(|| {
            resolve_organic_polaroid_slots(&slots, 10800, 1350, 1080, 10, 0);
        });
        assert!(result.is_err());
    }
}
