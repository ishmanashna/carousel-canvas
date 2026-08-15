use crate::python_rng::PythonRandom;
use crate::slot::StripSlotDef;

const M2H_W: i32 = 1200;
const M2H_FRAC: f64 = 0.5;
const MOSAIC_H_TAIL_W: i32 = 1700;
const FIRST_V_TRIM_LEFT: f64 = 0.08;
const CANVAS_HEIGHT: i32 = 1350;
const CANVAS_WIDTH: i32 = 10800;

fn v_slot(x: i32, y: i32, w: i32, h: i32) -> StripSlotDef {
    StripSlotDef::new(x, y, w, h).with_prefer_portrait()
}

fn h_slot(x: i32, y: i32, w: i32, h: i32) -> StripSlotDef {
    StripSlotDef::new(x, y, w, h)
        .with_prefer_landscape()
        .with_cover_height_first()
}

/// Seeded mosaic: lead V (trim), optional 2V, col-2 H|2H|3H, three portrait cols, 2H stack, tail H H.
pub fn build_seamless_mosaic_v1_slots(layout_seed: i64) -> Vec<StripSlotDef> {
    let mut r = PythonRandom::new((layout_seed as u32) & 0xffff_ffff);
    let col2 = ["H", "2H", "3H"][r.randrange(3) as usize];
    let twov_idx = r.choice_i32(&[-1, 1, 2, 3]);
    let trim = FIRST_V_TRIM_LEFT;
    let mut slots = Vec::new();
    let mut x = 0;

    slots.push(
        StripSlotDef::new(x, 0, 1000, CANVAS_HEIGHT)
            .with_prefer_portrait()
            .with_source_trim_left_frac(trim),
    );
    x += 1000;

    match col2 {
        "H" => {
            slots.push(h_slot(x, 0, 2200, CANVAS_HEIGHT));
        }
        "2H" => {
            slots.push(
                StripSlotDef::new(x, 0, 2200, 675)
                    .with_prefer_landscape()
                    .with_horizontal_center_band_frac(0.5),
            );
            slots.push(
                StripSlotDef::new(x, 675, 2200, 675)
                    .with_prefer_landscape()
                    .with_horizontal_center_band_frac(0.5),
            );
        }
        _ => {
            let h3 = CANVAS_HEIGHT / 3;
            let bf = 1.0 / 3.0;
            for i in 0..3 {
                slots.push(
                    StripSlotDef::new(x, i * h3, 2200, h3)
                        .with_prefer_landscape()
                        .with_horizontal_center_band_frac(bf),
                );
            }
        }
    }
    x += 2200;

    for j in 0..3 {
        let col = 1 + j;
        if twov_idx == col {
            slots.push(v_slot(x, 0, 500, CANVAS_HEIGHT));
            slots.push(v_slot(x + 500, 0, 500, CANVAS_HEIGHT));
        } else {
            slots.push(v_slot(x, 0, 1000, CANVAS_HEIGHT));
        }
        x += 1000;
    }

    slots.push(
        StripSlotDef::new(x, 0, M2H_W, 675)
            .with_prefer_landscape()
            .with_horizontal_center_band_frac(M2H_FRAC),
    );
    slots.push(
        StripSlotDef::new(x, 675, M2H_W, 675)
            .with_prefer_landscape()
            .with_horizontal_center_band_frac(M2H_FRAC),
    );
    x += M2H_W;

    slots.push(h_slot(x, 0, MOSAIC_H_TAIL_W, CANVAS_HEIGHT));
    x += MOSAIC_H_TAIL_W;
    slots.push(h_slot(x, 0, MOSAIC_H_TAIL_W, CANVAS_HEIGHT));
    x += MOSAIC_H_TAIL_W;

    debug_assert_eq!(x, CANVAS_WIDTH);
    slots
}

pub fn mosaic_v1_max_slot_count() -> usize {
    (0..64)
        .map(|s| build_seamless_mosaic_v1_slots(s).len())
        .max()
        .unwrap_or(0)
}

pub fn mosaic_canvas_extent(slots: &[StripSlotDef]) -> i32 {
    slots.iter().map(|s| s.right()).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_seeds_extent_and_len() {
        let cases = [(0, 11), (1, 9), (7, 11), (42, 11), (100, 10)];
        for (seed, expected_len) in cases {
            let slots = build_seamless_mosaic_v1_slots(seed);
            assert_eq!(slots.len(), expected_len, "seed {seed}");
            assert_eq!(mosaic_canvas_extent(&slots), CANVAS_WIDTH, "seed {seed}");
            assert!((9..=12).contains(&slots.len()), "seed {seed}");
        }
    }
}
