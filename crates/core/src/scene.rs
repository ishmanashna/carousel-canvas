use std::path::PathBuf;

use crate::polaroid_card::polaroid_slot_seed;
use crate::slot::{Fit, StripSlotDef};
use crate::template::{LayoutPlacer, StripTemplate};
use crate::underfill::UnderfillBox;

const OOF_PAPER_EDGE_FEATHER_PX: u32 = 56;

pub(crate) fn oof_card_style(is_oof: bool, cutout: bool) -> (bool, u32) {
    (
        is_oof && cutout,
        if is_oof && !cutout {
            OOF_PAPER_EDGE_FEATHER_PX
        } else {
            0
        },
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl From<&StripSlotDef> for Rect {
    fn from(s: &StripSlotDef) -> Self {
        Self {
            x: s.x,
            y: s.y,
            w: s.w,
            h: s.h,
        }
    }
}

impl From<&UnderfillBox> for Rect {
    fn from(b: &UnderfillBox) -> Self {
        Self {
            x: b.x,
            y: b.y,
            w: b.w,
            h: b.h,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneCard {
    pub photo_path: PathBuf,
    pub dest: Rect,
    pub z: i32,
    pub rotation_deg: f64,
    pub fit: Fit,
    pub pan_x: f64,
    pub pan_y: f64,
    pub flip_h: bool,
    pub source_trim_left_frac: Option<f64>,
    pub horizontal_center_band_frac: Option<f64>,
    pub cover_height_first: bool,
    pub polaroid: bool,
    pub slot_seed: u32,
    pub cutout: bool,
    pub mask_path: Option<PathBuf>,
    /// Oriented-source crop `[x, y, w, h]` for cutout figures.
    pub source_crop: Option<[i32; 4]>,
    /// Drop-shadow pass before the card (cutout figures only for out-of-frame).
    pub cast_shadow: bool,
    /// Soften paper rectangle edges so overlaps mix instead of hard seams + shadows.
    pub edge_feather_px: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub background: String,
    pub canvas_width: i32,
    pub canvas_height: i32,
    pub cards: Vec<SceneCard>,
}

pub fn build_scene(
    template: &StripTemplate,
    fills: &[Option<PathBuf>],
    layout_seed: Option<i64>,
) -> Scene {
    let slots = template.resolved_slots(layout_seed);
    let req = template.strip_effective_fill_required(layout_seed);
    let eff_seed = layout_seed.unwrap_or(0);
    let mut cards = Vec::new();
    for (i, slot) in slots.iter().enumerate() {
        if i >= req.len() || !req[i] {
            continue;
        }
        if let Some(path) = fills.get(i).and_then(|f| f.as_ref()) {
            cards.push(SceneCard {
                photo_path: path.clone(),
                dest: Rect::from(slot),
                z: slot.z_index,
                rotation_deg: slot.rotation_deg,
                fit: slot.fit,
                pan_x: 0.0,
                pan_y: 0.0,
                flip_h: false,
                source_trim_left_frac: slot.source_trim_left_frac,
                horizontal_center_band_frac: slot.horizontal_center_band_frac,
                cover_height_first: slot.cover_height_first,
                polaroid: slot.polaroid,
                slot_seed: polaroid_slot_seed(eff_seed, i),
                cutout: slot.cutout,
                mask_path: slot.mask_path.clone(),
                source_crop: slot.source_crop,
                cast_shadow: false,
                edge_feather_px: 0,
            });
        }
    }
    cards.sort_by_key(|c| c.z);
    Scene {
        background: template.background.to_string(),
        canvas_width: template.canvas_width,
        canvas_height: template.canvas_height,
        cards,
    }
}

/// Build scene from pre-resolved slots (out-of-frame). Does not call `resolved_slots`.
pub fn build_scene_from_resolved(
    template: &StripTemplate,
    slots: &[StripSlotDef],
    fills: &[Option<PathBuf>],
    fill_required: &[bool],
    layout_seed: Option<i64>,
) -> Scene {
    let eff_seed = layout_seed.unwrap_or(0);
    let mut cards = Vec::new();
    for (i, slot) in slots.iter().enumerate() {
        if i >= fill_required.len() || !fill_required[i] {
            continue;
        }
        if let Some(path) = fills.get(i).and_then(|f| f.as_ref()) {
            let (cast_shadow, edge_feather_px) = oof_card_style(
                template.layout_placer == LayoutPlacer::OutOfFrame,
                slot.cutout,
            );
            cards.push(SceneCard {
                photo_path: path.clone(),
                dest: Rect::from(slot),
                z: slot.z_index,
                rotation_deg: slot.rotation_deg,
                fit: slot.fit,
                pan_x: 0.0,
                pan_y: 0.0,
                flip_h: false,
                source_trim_left_frac: slot.source_trim_left_frac,
                horizontal_center_band_frac: slot.horizontal_center_band_frac,
                cover_height_first: slot.cover_height_first,
                polaroid: slot.polaroid,
                slot_seed: polaroid_slot_seed(eff_seed, i),
                cutout: slot.cutout,
                mask_path: slot.mask_path.clone(),
                source_crop: slot.source_crop,
                cast_shadow,
                edge_feather_px,
            });
        }
    }
    cards.sort_by_key(|c| c.z);
    Scene {
        background: template.background.to_string(),
        canvas_width: template.canvas_width,
        canvas_height: template.canvas_height,
        cards,
    }
}

/// Build scene with underfill cards behind required slots.
pub fn build_scene_with_underfill(
    template: &StripTemplate,
    fills: &[Option<PathBuf>],
    layout_seed: Option<i64>,
    underfill_boxes: &[UnderfillBox],
    underfill_paths: &[PathBuf],
) -> Scene {
    let mut scene = build_scene(template, fills, layout_seed);
    let mut underfill_cards = Vec::new();
    for (i, (box_, path)) in underfill_boxes
        .iter()
        .zip(underfill_paths.iter())
        .enumerate()
    {
        underfill_cards.push(SceneCard {
            photo_path: path.clone(),
            dest: Rect::from(box_),
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
            cutout: false,
            mask_path: None,
            source_crop: None,
            cast_shadow: false,
            edge_feather_px: 0,
        });
    }
    underfill_cards.sort_by_key(|c| c.z);
    let mut all = underfill_cards;
    all.extend(scene.cards);
    scene.cards = all;
    scene
}

fn rotated_aabb_half_extent(w: i32, h: i32, rot_deg: f64) -> (i32, i32) {
    let rad = rot_deg.to_radians();
    let (sin, cos) = rad.sin_cos();
    let w_f = w as f64;
    let h_f = h as f64;
    let aabb_w = (w_f * cos.abs() + h_f * sin.abs()).ceil() as i32;
    let aabb_h = (w_f * sin.abs() + h_f * cos.abs()).ceil() as i32;
    ((aabb_w - w).max(0) / 2, (aabb_h - h).max(0) / 2)
}

/// Bleed for tile cameras: max rotated-slot padding and largest underfill half-extent.
pub fn compute_bleed_px(scene: &Scene, underfill_boxes: &[UnderfillBox]) -> i32 {
    let mut bleed = 0i32;
    for card in &scene.cards {
        let (hx, hy) = rotated_aabb_half_extent(
            card.dest.w,
            card.dest.h,
            card.rotation_deg,
        );
        bleed = bleed.max(hx).max(hy);
    }
    for b in underfill_boxes {
        let (hx, hy) = rotated_aabb_half_extent(b.w, b.h, b.rotation_deg);
        bleed = bleed.max(hx).max(hy);
        bleed = bleed.max(b.w / 2).max(b.h / 2);
    }
    bleed
}
