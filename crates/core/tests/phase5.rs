use core::{get_template_by_id, resolve_organic_polaroid_slots};

#[test]
fn polaroid_resolved_slots_move_with_seed() {
    let tpl = get_template_by_id("strip_polaroid_table_v1").unwrap();
    let a = tpl.resolved_slots(Some(0));
    let b = tpl.resolved_slots(Some(7));
    assert_ne!(a[0].x, b[0].x);
    assert_ne!(a[5].y, b[5].y);
}

#[test]
fn polaroid_hero_in_first_slice() {
    let tpl = get_template_by_id("strip_polaroid_table_v1").unwrap();
    let slots = tpl.resolved_slots(Some(42));
    let hero = &slots[0];
    assert!(hero.x >= 0);
    assert!(hero.x + hero.w <= tpl.slice_width);
}

#[test]
fn polaroid_not_placeholder_grid() {
    let tpl = get_template_by_id("strip_polaroid_table_v1").unwrap();
    let slots = tpl.resolved_slots(Some(11));
    assert!(slots.iter().any(|s| s.x > tpl.slice_width));
    let xs: Vec<i32> = slots.iter().map(|s| s.x).collect();
    let unique = xs
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert!(unique > 10);
}

#[test]
fn organic_placer_matches_helper() {
    let tpl = get_template_by_id("strip_polaroid_table_v1").unwrap();
    let via_tpl = tpl.resolved_slots(Some(99));
    let direct = resolve_organic_polaroid_slots(
        &tpl.slots,
        tpl.canvas_width,
        tpl.canvas_height,
        tpl.slice_width,
        tpl.slice_count,
        99,
    );
    for (a, b) in via_tpl.iter().zip(direct.iter()) {
        assert_eq!(a.x, b.x);
        assert_eq!(a.y, b.y);
    }
}

#[test]
fn mural_v2_still_jitters() {
    let tpl = get_template_by_id("strip_mural_v2").unwrap();
    let a = tpl.resolved_slots(Some(0));
    let b = tpl.resolved_slots(Some(7));
    let hero = tpl.layout_flagship_slot_index.unwrap();
    assert_ne!(a[hero].x, b[hero].x);
}
