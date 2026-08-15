//! Manual slot assignment with pan / flip (GUI + export).

use std::path::PathBuf;

use crate::polaroid_card::polaroid_slot_seed;
use crate::scene::{Rect, Scene, SceneCard};
use crate::slot::StripSlotDef;
use crate::template::StripTemplate;
use crate::underfill::UnderfillBox;
use crate::Fit;

/// One photo placed in a template slot (matches Python `ManualSlotFill` minus grayscale).
#[derive(Debug, Clone, PartialEq)]
pub struct SlotFill {
    pub path: PathBuf,
    pub pan_x: f64,
    pub pan_y: f64,
    pub flip_h: bool,
}

impl SlotFill {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            pan_x: 0.0,
            pan_y: 0.0,
            flip_h: false,
        }
    }
}

pub fn scale_rect(r: &Rect, scale: f64) -> Rect {
    if (scale - 1.0).abs() < 1e-9 {
        return r.clone();
    }
    Rect {
        x: (r.x as f64 * scale).round() as i32,
        y: (r.y as f64 * scale).round() as i32,
        w: ((r.w as f64 * scale).round() as i32).max(1),
        h: ((r.h as f64 * scale).round() as i32).max(1),
    }
}

pub fn scale_underfill_box(b: &UnderfillBox, scale: f64) -> UnderfillBox {
    if (scale - 1.0).abs() < 1e-9 {
        return b.clone();
    }
    UnderfillBox {
        x: (b.x as f64 * scale).round() as i32,
        y: (b.y as f64 * scale).round() as i32,
        w: ((b.w as f64 * scale).round() as i32).max(1),
        h: ((b.h as f64 * scale).round() as i32).max(1),
        rotation_deg: b.rotation_deg,
    }
}

fn scene_card_from_slot(
    slot: &StripSlotDef,
    fill: &SlotFill,
    layout_seed: i64,
    slot_index: usize,
    scale: f64,
) -> SceneCard {
    let dest = scale_rect(&Rect::from(slot), scale);
    SceneCard {
        photo_path: fill.path.clone(),
        dest,
        z: slot.z_index,
        rotation_deg: slot.rotation_deg,
        fit: slot.fit,
        pan_x: fill.pan_x,
        pan_y: fill.pan_y,
        flip_h: fill.flip_h,
        source_trim_left_frac: slot.source_trim_left_frac,
        horizontal_center_band_frac: slot.horizontal_center_band_frac,
        cover_height_first: slot.cover_height_first,
        polaroid: slot.polaroid,
        slot_seed: polaroid_slot_seed(layout_seed, slot_index),
    }
}

/// Build a scene from GUI slot fills. ``compose_scale`` scales canvas and geometry (preview).
pub fn build_scene_from_fills(
    template: &StripTemplate,
    fills: &[Option<SlotFill>],
    layout_seed: Option<i64>,
    compose_scale: f64,
) -> (Scene, Vec<StripSlotDef>) {
    let eff_seed = layout_seed.unwrap_or(0);
    let slots = template.resolved_slots(layout_seed);
    let req = template.strip_effective_fill_required(layout_seed);
    let scale = compose_scale.max(1e-6);
    let cw = if (scale - 1.0).abs() < 1e-9 {
        template.canvas_width
    } else {
        ((template.canvas_width as f64 * scale).round() as i32).max(1)
    };
    let ch = if (scale - 1.0).abs() < 1e-9 {
        template.canvas_height
    } else {
        ((template.canvas_height as f64 * scale).round() as i32).max(1)
    };

    let mut cards = Vec::new();
    for (i, slot) in slots.iter().enumerate() {
        if i >= req.len() || !req[i] {
            continue;
        }
        if let Some(fill) = fills.get(i).and_then(|f| f.as_ref()) {
            cards.push(scene_card_from_slot(slot, fill, eff_seed, i, scale));
        }
    }
    cards.sort_by_key(|c| c.z);
    let scene = Scene {
        background: template.background.to_string(),
        canvas_width: cw,
        canvas_height: ch,
        cards,
    };
    (scene, slots)
}

/// Paths-only view of slot fills (for export assignment validation).
pub fn fills_to_paths(fills: &[Option<SlotFill>]) -> Vec<Option<PathBuf>> {
    fills.iter().map(|f| f.as_ref().map(|s| s.path.clone())).collect()
}

/// Build scene with underfill cards behind required slots.
pub fn build_scene_from_fills_with_underfill(
    template: &StripTemplate,
    fills: &[Option<SlotFill>],
    layout_seed: Option<i64>,
    compose_scale: f64,
    underfill_boxes: &[UnderfillBox],
    underfill_paths: &[PathBuf],
    underfill_compose_scale: f64,
) -> Scene {
    let (mut scene, _) = build_scene_from_fills(template, fills, layout_seed, compose_scale);
    let uf_scale = underfill_compose_scale.max(1e-6);
    let mut underfill_cards = Vec::new();
    for (i, (box_, path)) in underfill_boxes.iter().zip(underfill_paths.iter()).enumerate() {
        let dest = scale_rect(&Rect::from(box_), uf_scale);
        underfill_cards.push(SceneCard {
            photo_path: path.clone(),
            dest,
            z: -100 + i as i32,
            rotation_deg: box_.rotation_deg,
            fit: Fit::Cover,
            pan_x: 0.0,
            pan_y: 0.0,
            flip_h: false,
            source_trim_left_frac: None,
            horizontal_center_band_frac: None,
            cover_height_first: false,
            polaroid: false,
            slot_seed: 0,
        });
    }
    underfill_cards.sort_by_key(|c| c.z);
    let mut all = underfill_cards;
    all.extend(scene.cards);
    scene.cards = all;
    scene
}
