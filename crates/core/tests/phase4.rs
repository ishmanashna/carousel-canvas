use core::{
    carousel_tail_band_x_range, get_template_by_id, plan_mural_underfill_boxes,
    underfill_rng_seed,
};

#[test]
fn underfill_rng_seed_xor() {
    assert_eq!(underfill_rng_seed(7), (7 as u32) ^ 0x1B873F91);
}

#[test]
fn mural_underfill_planner_on_beige() {
    let cw = 400u32;
    let ch = 80u32;
    let bg = [236u8, 232u8, 227u8];
    let mut flat = vec![0u8; cw as usize * ch as usize * 3];
    for i in (0..flat.len()).step_by(3) {
        flat[i] = bg[0];
        flat[i + 1] = bg[1];
        flat[i + 2] = bg[2];
    }
    for y in 20..45 {
        for x in 120..180 {
            let idx = (y * cw as usize + x) * 3;
            flat[idx] = 40;
            flat[idx + 1] = 40;
            flat[idx + 2] = 40;
        }
    }
    let boxes = plan_mural_underfill_boxes(
        &flat,
        cw,
        ch,
        bg,
        48,
        7,
        1,
        0,
        0,
        0,
        cw as i32 - 80,
        cw as i32,
        52,
    );
    assert!(!boxes.is_empty());
}

#[test]
fn tail_band_single_slice() {
    let (x0, x1) = carousel_tail_band_x_range(10800, 1080, 10, 0, 1);
    assert_eq!((x0, x1), (9720, 10800));
}

#[test]
fn mural_v2_jitter_moves_hero() {
    let tpl = get_template_by_id("strip_mural_v2").unwrap();
    let slots0 = tpl.resolved_slots(Some(0));
    let slots7 = tpl.resolved_slots(Some(7));
    let hero = tpl.layout_flagship_slot_index.unwrap();
    assert_ne!(slots0[hero].x, slots7[hero].x);
}
