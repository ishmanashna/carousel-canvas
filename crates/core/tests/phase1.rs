use std::path::PathBuf;

use core::{
    build_seamless_mosaic_v1_slots, build_scene, get_template_by_id, mosaic_canvas_extent,
    pick_smart_fills, validate_strip_unique_sources, CoreError, LayoutPlacer, TEMPLATE_IDS,
};

#[test]
fn all_seven_templates_registered() {
    let expected: &[(&str, usize, usize)] = &[
        ("strip_mural_v2", 35, 35),
        ("strip_polaroid_table_v1", 24, 20),
        ("strip_seamless_mosaic_v1", 11, 12), // seed-0 slots; required count uses max without seed
        ("strip_seamless_v1", 9, 9),
        ("strip_mural_v1", 8, 8),
        ("strip_10col", 10, 10),
        ("strip_out_of_frame_v1", 0, 26),
    ];
    assert_eq!(TEMPLATE_IDS.len(), 7);
    for (id, total, required) in expected {
        let tpl = get_template_by_id(id).expect(id);
        assert_eq!(tpl.num_slots(), *total, "{id} total slots");
        assert_eq!(tpl.strip_image_slot_count(None), *required, "{id} required");
        assert_eq!(tpl.canvas_width, 10800);
        assert_eq!(tpl.canvas_height, 1350);
        assert_eq!(tpl.slice_width, 1080);
        assert_eq!(tpl.slice_height, 1350);
        assert_eq!(tpl.slice_count, 10);
        assert_eq!(tpl.overlap_px, 0);
    }
}

#[test]
fn mural_v2_hero_index() {
    let tpl = get_template_by_id("strip_mural_v2").unwrap();
    assert_eq!(tpl.num_slots(), 35);
    assert_eq!(tpl.layout_flagship_slot_index, Some(34));
    assert_eq!(tpl.layout_flagship_rim_slot_indices, vec![32, 33]);
    assert_eq!(tpl.background_underfill_layers, 10);
    assert_eq!(tpl.background_underfill_boost_layers, 5);
    assert_eq!(tpl.background_underfill_repeat_layers, 9);
    assert_eq!(tpl.background_tail_underfill_layers, 18);
    assert_eq!(tpl.background_tail_slice_count, 1);
    assert_eq!(tpl.gap_fill_max_layers, 0);
}

#[test]
fn polaroid_fill_required_mask() {
    let tpl = get_template_by_id("strip_polaroid_table_v1").unwrap();
    let req = tpl.effective_slot_fill_required();
    assert_eq!(req.len(), 24);
    assert_eq!(req.iter().filter(|r| **r).count(), 20);
    assert_eq!(req.iter().filter(|r| !**r).count(), 4);
}

#[test]
fn seamless_v1_has_negative_x() {
    let tpl = get_template_by_id("strip_seamless_v1").unwrap();
    assert!(tpl.slots.iter().any(|s| s.x < 0));
}

#[test]
fn mosaic_seeds_extent_and_slot_count() {
    for seed in [0i64, 1, 7, 42, 100, 5] {
        let slots = build_seamless_mosaic_v1_slots(seed);
        assert!((9..=12).contains(&slots.len()), "seed {seed}");
        assert_eq!(mosaic_canvas_extent(&slots), 10800, "seed {seed}");
    }
}

#[test]
fn uniqueness_fails_when_too_few_files() {
    let tpl = get_template_by_id("strip_mural_v2").unwrap();
    let paths: Vec<PathBuf> = (0..5).map(|i| PathBuf::from(format!("p{i}.jpg"))).collect();
    let err = validate_strip_unique_sources(&tpl, &paths, false, None).unwrap_err();
    match err {
        CoreError::NotEnoughUniquePhotos { need, have, .. } => {
            assert_eq!(need, 35);
            assert_eq!(have, 5);
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn out_of_frame_resolved_slots_empty_seed_only() {
    let tpl = get_template_by_id("strip_out_of_frame_v1").unwrap();
    assert_eq!(tpl.layout_placer, LayoutPlacer::OutOfFrame);
    assert_eq!(tpl.num_slots(), 0);
    assert_eq!(tpl.strip_image_slot_count(None), 26);
    assert_eq!(tpl.resolved_slots(Some(7)).len(), 0);
    assert_eq!(tpl.background_underfill_count(), 0);
    let paths: Vec<PathBuf> = vec![PathBuf::from("one.jpg")];
    validate_strip_unique_sources(&tpl, &paths, false, Some(7)).unwrap();
}

#[test]
fn smart_assign_fills_mural_v2() {
    let tpl = get_template_by_id("strip_mural_v2").unwrap();
    let paths: Vec<PathBuf> = (0..50)
        .map(|i| {
            let prefix = if i % 3 == 0 { "portrait" } else { "landscape" };
            PathBuf::from(format!("{prefix}_{i:02}.jpg"))
        })
        .collect();
    let fills = pick_smart_fills(&paths, &tpl, Some(7), 99).unwrap();
    assert_eq!(fills.len(), 35);
    assert_eq!(fills.iter().filter(|f| f.is_some()).count(), 35);
    let scene = build_scene(&tpl, &fills, Some(7));
    assert_eq!(scene.cards.len(), 35);
    assert_eq!(tpl.layout_flagship_slot_index, Some(34));
}
