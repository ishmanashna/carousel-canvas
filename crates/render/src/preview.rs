//! GPU preview compose at reduced scale (GUI mural strip).
//!
//! CPU work (decode / underfill plan) is separable from wgpu so the GUI can keep
//! decode on workers and uploads/draws on the main thread. Underfill planning uses
//! full template resolution on an offscreen worker GPU (same as CLI export).

use std::path::PathBuf;

use core::{
    build_scene_from_fills, build_scene_from_fills_with_underfill, build_scene_from_resolved,
    build_scene_from_snapshot, fills_to_paths, LayoutSnapshot, Scene, SlotFill, StripSlotDef,
    StripTemplate, UnderfillBox,
};
use image::RgbImage;

use crate::card_edge::prepare_render_cards;
use crate::color::parse_color_rgb;
use crate::compositor::Compositor;
use crate::decode_scene_cards;
use crate::export::CardEdge;
use crate::error::Result;
use crate::strip_export::{create_offscreen_compositor, flatten_and_plan_underfill};
use crate::DecodeCache;
use crate::DecodedCard;

pub const PREVIEW_MAX_COMPOSE_WIDTH: f64 = 2200.0;

#[derive(Debug, Clone)]
pub struct PreviewBuildResult {
    pub rgb: RgbImage,
    pub overlay_slots: Vec<StripSlotDef>,
    pub canvas_width: i32,
    pub canvas_height: i32,
    /// Full template-space underfill boxes (for Phase 3 snapshot).
    pub underfill_boxes: Vec<UnderfillBox>,
    pub underfill_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct PreviewParams {
    pub template: StripTemplate,
    pub fills: Vec<Option<SlotFill>>,
    pub source_paths: Vec<PathBuf>,
    pub layout_seed: i64,
    pub card_edge: CardEdge,
    pub border_rgb: Option<[u8; 3]>,
    /// When false, skip beige underfill for a faster interactive preview.
    pub include_underfill: bool,
    /// Pre-resolved slots for out-of-frame (skips `template.resolved_slots`).
    pub resolved_slots: Option<Vec<StripSlotDef>>,
    pub fill_required: Option<Vec<bool>>,
}

/// CPU phase 1: required-slot scene + decoded textures (no wgpu).
#[derive(Debug, Clone)]
pub struct PreviewPhase1 {
    pub params: PreviewParams,
    pub compose_scale: f64,
    /// Required slots at full template resolution when underfill is enabled, else compose scale.
    pub required_scene: Scene,
    pub required_decoded: Vec<DecodedCard>,
    pub overlay_slots: Vec<StripSlotDef>,
    pub underfill_enabled: bool,
    pub bg_rgb: [u8; 3],
    pub border_rgb: [u8; 3],
    pub background_rgb: Option<RgbImage>,
}

/// Fully decoded scene ready for a single GPU compose pass.
#[derive(Debug, Clone)]
pub struct PreviewGpuReady {
    pub scene: Scene,
    pub decoded: Vec<DecodedCard>,
    pub overlay_slots: Vec<StripSlotDef>,
    pub bg_rgb: [u8; 3],
    pub border_rgb: [u8; 3],
    pub card_edge: CardEdge,
    pub background_rgb: Option<RgbImage>,
    pub underfill_boxes: Vec<UnderfillBox>,
    pub underfill_paths: Vec<PathBuf>,
}

pub fn preview_compose_scale(template: &StripTemplate) -> f64 {
    let w = template.canvas_width.max(1) as f64;
    (PREVIEW_MAX_COMPOSE_WIDTH / w).min(1.0)
}

fn procedural_background_for_template(
    template: &StripTemplate,
    layout_seed: i64,
    compose_scale: f64,
) -> Option<RgbImage> {
    template.procedural_background.map(|kind| {
        let cw = ((template.canvas_width as f64 * compose_scale).round() as u32).max(1);
        let ch = ((template.canvas_height as f64 * compose_scale).round() as u32).max(1);
        crate::procedural_background::render_procedural_background(kind, cw, ch, layout_seed)
    })
}

/// Decode required cards on a worker thread (Send).
pub fn prepare_preview_phase1(params: PreviewParams) -> Result<PreviewPhase1> {
    let layout_seed = Some(params.layout_seed);
    let compose_scale = preview_compose_scale(&params.template);
    let bg_rgb = parse_color_rgb(params.template.background);
    let border_rgb = params.border_rgb.unwrap_or(bg_rgb);
    let background_rgb =
        procedural_background_for_template(&params.template, params.layout_seed, compose_scale);

    let underfill_enabled = params.include_underfill
        && params.card_edge == CardEdge::Borderless
        && params.template.background_underfill_count() > 0;

    // Full-res decode for underfill flatten/plan; scaled decode for display-only path.
    let scene_scale = if underfill_enabled { 1.0 } else { compose_scale };
    let (required_scene, overlay_slots) = if let (Some(slots), Some(req)) =
        (&params.resolved_slots, &params.fill_required)
    {
        let paths = fills_to_paths(&params.fills);
        let scene = build_scene_from_resolved(
            &params.template,
            slots,
            &paths,
            req,
            layout_seed,
        );
        (scene, slots.clone())
    } else {
        build_scene_from_fills(&params.template, &params.fills, layout_seed, scene_scale)
    };
    let cache = DecodeCache::new();
    let required_decoded = decode_scene_cards(&required_scene, &cache)?;

    Ok(PreviewPhase1 {
        params,
        compose_scale,
        required_scene,
        required_decoded,
        overlay_slots,
        underfill_enabled,
        bg_rgb,
        border_rgb,
        background_rgb,
    })
}

/// Worker: offscreen full-res flatten, plan underfill, decode display scene at compose scale.
pub fn prepare_preview_phase2(phase1: PreviewPhase1) -> Result<PreviewGpuReady> {
    let params = &phase1.params;
    let layout_seed = Some(params.layout_seed);
    let compositor = create_offscreen_compositor()?;
    let planned = flatten_and_plan_underfill(
        &compositor,
        &params.template,
        &phase1.required_scene,
        &phase1.required_decoded,
        phase1.bg_rgb,
        phase1.border_rgb,
        params.layout_seed,
        &params.source_paths,
        &fills_to_paths(&params.fills),
    )?;
    let scene = build_scene_from_fills_with_underfill(
        &params.template,
        &params.fills,
        layout_seed,
        phase1.compose_scale,
        &planned.boxes,
        &planned.paths,
        phase1.compose_scale,
    );
    let cache = DecodeCache::new();
    let decoded = decode_scene_cards(&scene, &cache)?;
    Ok(PreviewGpuReady {
        scene,
        decoded,
        overlay_slots: phase1.overlay_slots,
        bg_rgb: phase1.bg_rgb,
        border_rgb: phase1.border_rgb,
        card_edge: params.card_edge,
        background_rgb: phase1.background_rgb,
        underfill_boxes: planned.boxes,
        underfill_paths: planned.paths,
    })
}

/// No-underfill path: phase1 is already GPU-ready.
pub fn preview_ready_without_underfill(phase1: PreviewPhase1) -> PreviewGpuReady {
    PreviewGpuReady {
        scene: phase1.required_scene,
        decoded: phase1.required_decoded,
        overlay_slots: phase1.overlay_slots,
        bg_rgb: phase1.bg_rgb,
        border_rgb: phase1.border_rgb,
        card_edge: phase1.params.card_edge,
        background_rgb: phase1.background_rgb,
        underfill_boxes: Vec::new(),
        underfill_paths: Vec::new(),
    }
}

/// Manual / locked preview: recompose from snapshot only (no underfill replan).
pub fn prepare_preview_from_snapshot(
    snapshot: LayoutSnapshot,
    fills: Vec<Option<SlotFill>>,
    border_rgb: Option<[u8; 3]>,
) -> Result<PreviewGpuReady> {
    let compose_scale = preview_compose_scale(&snapshot.template);
    let card_edge = CardEdge::parse(&snapshot.card_edge).unwrap_or(CardEdge::Borderless);
    let bg_rgb = parse_color_rgb(snapshot.template.background);
    let border_rgb = border_rgb.unwrap_or(bg_rgb);
    let background_rgb = procedural_background_for_template(
        &snapshot.template,
        snapshot.layout_seed,
        compose_scale,
    );
    let scene = build_scene_from_snapshot(&snapshot, &fills, compose_scale);
    let cache = DecodeCache::new();
    let decoded = decode_scene_cards(&scene, &cache)?;
    Ok(PreviewGpuReady {
        scene,
        decoded,
        overlay_slots: snapshot.slots.clone(),
        bg_rgb,
        border_rgb,
        card_edge,
        background_rgb,
        underfill_boxes: snapshot.underfill_boxes.clone(),
        underfill_paths: snapshot.underfill_paths.clone(),
    })
}

/// Main-thread: upload + draw + readback.
pub fn compose_preview_gpu(
    compositor: &Compositor,
    ready: &PreviewGpuReady,
) -> Result<PreviewBuildResult> {
    let render_cards = prepare_render_cards(
        &ready.scene.cards,
        &ready.decoded,
        ready.card_edge,
        ready.bg_rgb,
        ready.border_rgb,
    );
    let rgb = compositor.render_scene_to_rgb(
        &render_cards,
        &ready.scene.background,
        ready.background_rgb.as_ref(),
        ready.scene.canvas_width.max(1) as u32,
        ready.scene.canvas_height.max(1) as u32,
    )?;

    Ok(PreviewBuildResult {
        rgb,
        overlay_slots: ready.overlay_slots.clone(),
        canvas_width: ready.scene.canvas_width,
        canvas_height: ready.scene.canvas_height,
        underfill_boxes: ready.underfill_boxes.clone(),
        underfill_paths: ready.underfill_paths.clone(),
    })
}

/// Full preview pipeline on one thread (tests / simple callers).
pub fn render_preview_gpu(
    compositor: &Compositor,
    params: &PreviewParams,
) -> Result<PreviewBuildResult> {
    let phase1 = prepare_preview_phase1(params.clone())?;
    let ready = if phase1.underfill_enabled {
        prepare_preview_phase2(phase1)?
    } else {
        preview_ready_without_underfill(phase1)
    };
    compose_preview_gpu(compositor, &ready)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_scale_caps_wide_canvas() {
        let tpl = core::get_template_by_id("strip_mural_v2").unwrap();
        let s = preview_compose_scale(&tpl);
        assert!(s < 1.0);
        assert!((tpl.canvas_width as f64 * s).round() <= PREVIEW_MAX_COMPOSE_WIDTH);
    }
}
