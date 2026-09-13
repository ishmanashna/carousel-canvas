//! Manual slot assignment with pan / flip (GUI + export).

use std::path::PathBuf;

use crate::polaroid_card::polaroid_slot_seed;
use crate::scene::{Rect, Scene, SceneCard};
use crate::slot::StripSlotDef;
use crate::template::StripTemplate;
use crate::underfill::UnderfillBox;
use crate::Fit;

/// Frozen layout from a successful Auto placement (Phase 3 manual edit lock).
#[derive(Debug, Clone)]
pub struct LayoutSnapshot {
    pub template: StripTemplate,
    pub canvas_width: i32,
    pub canvas_height: i32,
    /// Procedural background + polaroid slot_seed only; never re-run `resolved_slots`.
    pub layout_seed: i64,
    /// `"borderless"`, `"wedges"`, or `"border"` (core-friendly; no render dependency).
    pub card_edge: String,
    pub slots: Vec<StripSlotDef>,
    pub fill_required: Vec<bool>,
    pub underfill_boxes: Vec<UnderfillBox>,
    pub underfill_paths: Vec<PathBuf>,
}

impl LayoutSnapshot {
    /// Capture geometry after Auto preview/export planning at full canvas resolution.
    pub fn capture(
        template: &StripTemplate,
        layout_seed: i64,
        card_edge: &str,
        underfill_boxes: Vec<UnderfillBox>,
        underfill_paths: Vec<PathBuf>,
    ) -> Self {
        let slots = template.resolved_slots(Some(layout_seed));
        let fill_required = template.strip_effective_fill_required(Some(layout_seed));
        Self {
            template: template.clone(),
            canvas_width: template.canvas_width,
            canvas_height: template.canvas_height,
            layout_seed,
            card_edge: card_edge.to_string(),
            slots,
            fill_required,
            underfill_boxes,
            underfill_paths,
        }
    }
}

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

/// Compose from a frozen snapshot (Manual mode). Bypasses `template.resolved_slots()`.
/// Overlay / hit-test slots remain unscaled full-canvas defs on `LayoutSnapshot::slots`.
pub fn build_scene_from_snapshot(
    snapshot: &LayoutSnapshot,
    fills: &[Option<SlotFill>],
    compose_scale: f64,
) -> Scene {
    let scale = compose_scale.max(1e-6);
    let cw = if (scale - 1.0).abs() < 1e-9 {
        snapshot.canvas_width
    } else {
        ((snapshot.canvas_width as f64 * scale).round() as i32).max(1)
    };
    let ch = if (scale - 1.0).abs() < 1e-9 {
        snapshot.canvas_height
    } else {
        ((snapshot.canvas_height as f64 * scale).round() as i32).max(1)
    };

    let eff_seed = snapshot.layout_seed;
    let mut cards = Vec::new();
    for (i, slot) in snapshot.slots.iter().enumerate() {
        if i >= snapshot.fill_required.len() || !snapshot.fill_required[i] {
            continue;
        }
        if let Some(fill) = fills.get(i).and_then(|f| f.as_ref()) {
            cards.push(scene_card_from_slot(slot, fill, eff_seed, i, scale));
        }
    }
    cards.sort_by_key(|c| c.z);

    let mut underfill_cards = Vec::new();
    for (i, (box_, path)) in snapshot
        .underfill_boxes
        .iter()
        .zip(snapshot.underfill_paths.iter())
        .enumerate()
    {
        let dest = scale_rect(&Rect::from(box_), scale);
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
    all.extend(cards);
    Scene {
        background: snapshot.template.background.to_string(),
        canvas_width: cw,
        canvas_height: ch,
        cards: all,
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use crate::get_template_by_id;

    #[test]
    fn snapshot_slots_survive_compose_scale_without_jitter() {
        let tpl = get_template_by_id("strip_mural_v2").unwrap();
        let seed = 7_i64;
        let snap = LayoutSnapshot::capture(&tpl, seed, "borderless", Vec::new(), Vec::new());
        let frozen_x = snap.slots[0].x;
        let frozen_w = snap.slots[0].w;

        let mut fills: Vec<Option<SlotFill>> = vec![None; snap.slots.len()];
        fills[0] = Some(SlotFill::new(PathBuf::from("a.jpg")));

        let full = build_scene_from_snapshot(&snap, &fills, 1.0);
        let scaled = build_scene_from_snapshot(&snap, &fills, 0.25);

        assert_eq!(snap.slots[0].x, frozen_x);
        assert_eq!(full.canvas_width, snap.canvas_width);
        assert_eq!(
            scaled.canvas_width,
            ((snap.canvas_width as f64 * 0.25).round() as i32).max(1)
        );
        let card_full = full.cards.iter().find(|c| c.z >= 0).unwrap();
        let card_scaled = scaled.cards.iter().find(|c| c.z >= 0).unwrap();
        assert_eq!(card_full.dest.x, frozen_x);
        assert_eq!(card_full.dest.w, frozen_w);
        assert_eq!(
            card_scaled.dest.x,
            ((frozen_x as f64 * 0.25).round() as i32).max(1)
        );
    }
}
