use core::{build_scene_from_fills, get_template_by_id, SlotFill};
use std::path::PathBuf;

#[test]
fn slot_fill_pan_applied_to_scene() {
    let tpl = get_template_by_id("strip_10col").unwrap();
    let mut fills = vec![None; tpl.num_slots()];
    fills[0] = Some(SlotFill {
        path: PathBuf::from("a.jpg"),
        pan_x: 0.5,
        pan_y: -0.25,
        flip_h: true,
    });
    let (scene, _) = build_scene_from_fills(&tpl, &fills, Some(0), 1.0);
    assert_eq!(scene.cards.len(), 1);
    assert!((scene.cards[0].pan_x - 0.5).abs() < 1e-6);
    assert!(scene.cards[0].flip_h);
}
