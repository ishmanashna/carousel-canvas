//! GPU preview compose at reduced scale (GUI mural strip).
//!
//! CPU work (decode / underfill plan) is separable from wgpu so the GUI can keep
//! decode on workers and uploads/draws on the main thread.

use std::path::PathBuf;

use core::{
    build_scene_from_fills, build_scene_from_fills_with_underfill, carousel_tail_band_x_range,
    pick_underfill_paths, plan_and_assign_mural_underfill, Scene, SlotFill, StripSlotDef,
    StripTemplate,
};
use image::RgbImage;

use crate::card_edge::prepare_render_cards;
use crate::color::parse_color_rgb;
use crate::compositor::Compositor;
use crate::decode_scene_cards;
use crate::export::CardEdge;
use crate::error::Result;
use crate::strip_export::decode_underfill_cards;
use crate::DecodeCache;
use crate::DecodedCard;

pub const PREVIEW_MAX_COMPOSE_WIDTH: f64 = 4000.0;

#[derive(Debug, Clone)]
pub struct PreviewBuildResult {
    pub rgb: RgbImage,
    pub overlay_slots: Vec<StripSlotDef>,
    pub canvas_width: i32,
    pub canvas_height: i32,
}

#[derive(Debug, Clone)]
pub struct PreviewParams {
    pub template: StripTemplate,
    pub fills: Vec<Option<SlotFill>>,
    pub source_paths: Vec<PathBuf>,
    pub layout_seed: i64,
    pub card_edge: CardEdge,
    pub border_rgb: Option<[u8; 3]>,
}

/// CPU phase 1: required-slot scene + decoded textures (no wgpu).
#[derive(Debug, Clone)]
pub struct PreviewPhase1 {
    pub params: PreviewParams,
    pub compose_scale: f64,
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

    let (required_scene, overlay_slots) =
        build_scene_from_fills(&params.template, &params.fills, layout_seed, compose_scale);
    let cache = DecodeCache::new();
    let required_decoded = decode_scene_cards(&required_scene, &cache)?;
    let underfill_enabled = params.card_edge == CardEdge::Borderless
        && params.template.background_underfill_count() > 0;

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

/// Main-thread: required-slot flatten for underfill planning.
pub fn preview_required_flatten(
    compositor: &Compositor,
    phase1: &PreviewPhase1,
) -> Result<Vec<u8>> {
    let req_render = prepare_render_cards(
        &phase1.required_scene.cards,
        &phase1.required_decoded,
        CardEdge::Borderless,
        phase1.bg_rgb,
        phase1.border_rgb,
    );
    compositor.render_required_slots_flat_rgb(
        &req_render,
        &phase1.required_scene.background,
        phase1.required_scene.canvas_width,
        phase1.required_scene.canvas_height,
    )
}

/// Worker: plan underfill + decode boxes (needs flatten from main).
pub fn prepare_preview_phase2(
    phase1: PreviewPhase1,
    flat: Vec<u8>,
) -> Result<PreviewGpuReady> {
    let params = &phase1.params;
    let layout_seed = Some(params.layout_seed);
    let cache = DecodeCache::new();
    let cw = phase1.required_scene.canvas_width as u32;
    let ch = phase1.required_scene.canvas_height as u32;
    let tol = params.template.gap_fill_beige_tolerance.max(12);
    let n_uf = params.template.background_underfill_count() as usize;
    let mut uf_rng = core::underfill_rng_from_layout_seed(params.layout_seed);
    let uf_pool = pick_underfill_paths(
        &params.source_paths,
        &core::fills_to_paths(&params.fills),
        n_uf,
        &mut uf_rng,
    );
    let (tail_x0, tail_x1) = carousel_tail_band_x_range(
        params.template.canvas_width,
        params.template.slice_width,
        params.template.slice_count as i32,
        params.template.overlap_px,
        params.template.background_tail_slice_count,
    );
    let tail_tol_bonus = if params.template.background_tail_slice_count <= 1 {
        52
    } else {
        42
    };
    let scaled_tail_x0 = (tail_x0 as f64 * phase1.compose_scale).round() as i32;
    let scaled_tail_x1 = (tail_x1 as f64 * phase1.compose_scale).round() as i32;
    let planned = plan_and_assign_mural_underfill(
        &flat,
        cw,
        ch,
        phase1.bg_rgb,
        tol,
        params.layout_seed,
        params.template.background_underfill_layers,
        params.template.background_underfill_boost_layers,
        params.template.background_underfill_repeat_layers,
        params.template.background_tail_underfill_layers,
        scaled_tail_x0,
        scaled_tail_x1,
        tail_tol_bonus,
        &uf_pool,
    );
    let uf_decoded = decode_underfill_cards(&planned.boxes, &planned.paths, &cache)?;
    let scene = build_scene_from_fills_with_underfill(
        &params.template,
        &params.fills,
        layout_seed,
        phase1.compose_scale,
        &planned.boxes,
        &planned.paths,
        1.0,
    );
    let mut all = uf_decoded;
    let uf_len = all.len();
    for card in phase1.required_decoded {
        all.push(DecodedCard {
            index: uf_len + card.index,
            image: card.image,
        });
    }
    Ok(PreviewGpuReady {
        scene,
        decoded: all,
        overlay_slots: phase1.overlay_slots,
        bg_rgb: phase1.bg_rgb,
        border_rgb: phase1.border_rgb,
        card_edge: params.card_edge,
        background_rgb: phase1.background_rgb,
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
    }
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
    })
}

/// Full preview pipeline on one thread (tests / simple callers).
pub fn render_preview_gpu(
    compositor: &Compositor,
    params: &PreviewParams,
) -> Result<PreviewBuildResult> {
    let phase1 = prepare_preview_phase1(params.clone())?;
    let ready = if phase1.underfill_enabled {
        let flat = preview_required_flatten(compositor, &phase1)?;
        prepare_preview_phase2(phase1, flat)?
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

