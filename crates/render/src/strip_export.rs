//! Strip export orchestration (mural v2 underfill pipeline).

use std::path::PathBuf;

use core::{
    build_scene, build_scene_from_fills, build_scene_from_fills_with_underfill,
    build_scene_from_snapshot, build_scene_with_underfill, carousel_tail_band_x_range,
    compute_bleed_px, fills_to_paths, pick_best_layout_seed_with_token_retry, pick_underfill_paths,
    plan_and_assign_mural_underfill, strip_layout_token_retry_enabled,
    underfill_rng_from_layout_seed, LayoutSnapshot, PlannedUnderfill, Scene, SlotFill,
    StripTemplate,
};

use crate::card_edge::prepare_render_cards;
use crate::color::parse_color_rgb;
use crate::compositor::Compositor;
use crate::decode::decode_card_rgba;
use crate::error::{DecodeError, Result};
use crate::export::{export_scene_tiles, ExportOptions, ExportResult};
use crate::{decode_scene_cards, DecodeCache, DecodeOptions, DecodedCard};

#[derive(Debug, Clone)]
pub struct StripExportParams {
    pub template: StripTemplate,
    pub fills: Vec<Option<PathBuf>>,
    /// When set (GUI), pan/flip from manual edits are applied at export.
    pub slot_fills: Option<Vec<Option<SlotFill>>>,
    pub source_paths: Vec<PathBuf>,
    pub layout_seed: i64,
    pub card_edge: crate::export::CardEdge,
    pub border_rgb: Option<[u8; 3]>,
    pub no_layout_retry: bool,
    /// When set (GUI manual lock), export frozen geometry + underfill without replan.
    pub locked_layout: Option<LayoutSnapshot>,
    pub output_dir: PathBuf,
}

fn fill_paths_for_underfill(params: &StripExportParams) -> Vec<Option<PathBuf>> {
    if let Some(ref sf) = params.slot_fills {
        fills_to_paths(sf)
    } else {
        params.fills.clone()
    }
}

fn build_required_scene(
    template: &StripTemplate,
    params: &StripExportParams,
    layout_seed: Option<i64>,
) -> core::Scene {
    if let Some(ref sf) = params.slot_fills {
        build_scene_from_fills(template, sf, layout_seed, 1.0).0
    } else {
        build_scene(template, &params.fills, layout_seed)
    }
}

fn build_full_scene_with_underfill(
    template: &StripTemplate,
    params: &StripExportParams,
    layout_seed: Option<i64>,
    underfill_boxes: &[core::UnderfillBox],
    underfill_paths: &[PathBuf],
) -> core::Scene {
    if let Some(ref sf) = params.slot_fills {
        build_scene_from_fills_with_underfill(
            template,
            sf,
            layout_seed,
            1.0,
            underfill_boxes,
            underfill_paths,
            1.0,
        )
    } else {
        build_scene_with_underfill(
            template,
            &params.fills,
            layout_seed,
            underfill_boxes,
            underfill_paths,
        )
    }
}

/// Flatten required slots at full template resolution and plan underfill (shared by export + preview).
pub fn flatten_and_plan_underfill(
    compositor: &Compositor,
    template: &StripTemplate,
    required_scene: &Scene,
    required_decoded: &[DecodedCard],
    bg_rgb: [u8; 3],
    border_rgb: [u8; 3],
    layout_seed: i64,
    source_paths: &[PathBuf],
    fill_paths: &[Option<PathBuf>],
) -> Result<PlannedUnderfill> {
    let req_render = prepare_render_cards(
        &required_scene.cards,
        required_decoded,
        crate::export::CardEdge::Borderless,
        bg_rgb,
        border_rgb,
    );
    let flat = compositor.render_required_slots_flat_rgb(
        &req_render,
        &required_scene.background,
        template.canvas_width,
        template.canvas_height,
    )?;
    let cw = template.canvas_width as u32;
    let ch = template.canvas_height as u32;
    let tol = template.gap_fill_beige_tolerance.max(12);
    let n_uf = template.background_underfill_count() as usize;
    let mut uf_rng = underfill_rng_from_layout_seed(layout_seed);
    let uf_pool = pick_underfill_paths(source_paths, fill_paths, n_uf, &mut uf_rng);
    let (tail_x0, tail_x1) = carousel_tail_band_x_range(
        template.canvas_width,
        template.slice_width,
        template.slice_count as i32,
        template.overlap_px,
        template.background_tail_slice_count,
    );
    let tail_tol_bonus = if template.background_tail_slice_count <= 1 {
        52
    } else {
        42
    };
    Ok(plan_and_assign_mural_underfill(
        &flat,
        cw,
        ch,
        bg_rgb,
        tol,
        layout_seed,
        template.background_underfill_layers,
        template.background_underfill_boost_layers,
        template.background_underfill_repeat_layers,
        template.background_tail_underfill_layers,
        tail_x0,
        tail_x1,
        tail_tol_bonus,
        &uf_pool,
    ))
}

pub fn decode_underfill_cards(
    boxes: &[core::UnderfillBox],
    paths: &[PathBuf],
    cache: &DecodeCache,
) -> Result<Vec<DecodedCard>> {
    let mut decoded = Vec::with_capacity(boxes.len());
    for (i, (box_, path)) in boxes.iter().zip(paths.iter()).enumerate() {
        let dest_w = box_.w.max(1) as u32;
        let dest_h = box_.h.max(1) as u32;
        let options = DecodeOptions::default();
        if let Some(hit) = cache.get(path, dest_w, dest_h, &options) {
            decoded.push(DecodedCard {
                index: i,
                image: hit,
            });
            continue;
        }
        let image = decode_card_rgba(path, dest_w, dest_h, &options)?;
        cache.insert(path, dest_w, dest_h, &options, image.clone());
        decoded.push(DecodedCard {
            index: i,
            image: std::sync::Arc::new(image),
        });
    }
    Ok(decoded)
}

fn merge_decoded(required: Vec<DecodedCard>, underfill: Vec<DecodedCard>) -> Vec<DecodedCard> {
    let mut all = underfill;
    let uf_len = all.len();
    for card in required {
        all.push(DecodedCard {
            index: uf_len + card.index,
            image: card.image,
        });
    }
    all
}

pub fn effective_layout_seed(params: &StripExportParams, bg_rgb: [u8; 3]) -> i64 {
    let ce = match params.card_edge {
        crate::export::CardEdge::Borderless => "borderless",
        crate::export::CardEdge::Wedges => "wedges",
        crate::export::CardEdge::Border => "border",
    };
    if params.no_layout_retry || !strip_layout_token_retry_enabled(params.template.id, ce) {
        return params.layout_seed;
    }
    pick_best_layout_seed_with_token_retry(
        &params.fills,
        &params.template,
        params.layout_seed,
        ce,
        core::MURAL_V2_LAYOUT_RETRY_ATTEMPTS,
        core::TOKEN_COMPOSE_SCALE,
        bg_rgb,
    )
}

fn procedural_background_for_template(
    template: &StripTemplate,
    layout_seed: i64,
) -> Option<image::RgbImage> {
    template.procedural_background.map(|kind| {
        crate::procedural_background::render_procedural_background(
            kind,
            template.canvas_width as u32,
            template.canvas_height as u32,
            layout_seed,
        )
    })
}

fn run_locked_pipeline_on_gpu(
    compositor: &Compositor,
    params: &StripExportParams,
    snapshot: &LayoutSnapshot,
) -> Result<ExportResult> {
    let template = &params.template;
    let bg_rgb = parse_color_rgb(template.background);
    let border_rgb = params.border_rgb.unwrap_or(bg_rgb);
    let card_edge =
        crate::export::CardEdge::parse(&snapshot.card_edge).unwrap_or(params.card_edge);
    let fills = params
        .slot_fills
        .as_ref()
        .ok_or_else(|| DecodeError::Encode("locked export requires slot_fills".into()))?;
    eprintln!("export: locked layout (seed={})", snapshot.layout_seed);
    let scene = build_scene_from_snapshot(snapshot, fills, 1.0);
    let background_rgb =
        procedural_background_for_template(template, snapshot.layout_seed);
    let cache = DecodeCache::new();
    eprintln!("export: decoding {} cards…", scene.cards.len());
    let decoded = decode_scene_cards(&scene, &cache)?;
    let bleed_px = compute_bleed_px(&scene, &snapshot.underfill_boxes);
    eprintln!("export: rendering tiles (bleed={bleed_px})…");
    export_scene_tiles(
        compositor,
        &scene,
        &decoded,
        background_rgb.as_ref(),
        template,
        &params.output_dir,
        &ExportOptions {
            card_edge,
            bleed_px,
            border_rgb,
        },
    )
}

fn run_pipeline_on_gpu(
    compositor: &Compositor,
    params: &StripExportParams,
) -> Result<ExportResult> {
    if let Some(ref snapshot) = params.locked_layout {
        return run_locked_pipeline_on_gpu(compositor, params, snapshot);
    }
    let template = &params.template;
    let bg_rgb = parse_color_rgb(template.background);
    let border_rgb = params.border_rgb.unwrap_or(bg_rgb);
    eprintln!("export: layout retry…");
    let eff_seed = effective_layout_seed(params, bg_rgb);
    eprintln!("export: effective seed {eff_seed}");
    let layout_seed = Some(eff_seed);
    let background_rgb = procedural_background_for_template(template, eff_seed);

    let required_scene = build_required_scene(template, params, layout_seed);
    let cache = DecodeCache::new();
    eprintln!(
        "export: decoding {} required cards…",
        required_scene.cards.len()
    );
    let required_decoded = decode_scene_cards(&required_scene, &cache)?;

    let underfill_enabled = params.card_edge == crate::export::CardEdge::Borderless
        && template.background_underfill_count() > 0;

    if underfill_enabled {
        eprintln!("export: required-slot flatten for underfill…");
        let n_uf = template.background_underfill_count() as usize;
        eprintln!("export: planning underfill ({n_uf} pool)…");
        let planned = flatten_and_plan_underfill(
            compositor,
            template,
            &required_scene,
            &required_decoded,
            bg_rgb,
            border_rgb,
            eff_seed,
            &params.source_paths,
            &fill_paths_for_underfill(params),
        )?;
        eprintln!(
            "export: decoding {} underfill cards…",
            planned.boxes.len()
        );
        let uf_decoded = decode_underfill_cards(&planned.boxes, &planned.paths, &cache)?;
        let scene = build_full_scene_with_underfill(
            template,
            params,
            layout_seed,
            &planned.boxes,
            &planned.paths,
        );
        let bleed_px = compute_bleed_px(&scene, &planned.boxes);
        eprintln!("export: rendering tiles (bleed={bleed_px})…");
        let decoded = merge_decoded(required_decoded, uf_decoded);
        return export_scene_tiles(
            compositor,
            &scene,
            &decoded,
            background_rgb.as_ref(),
            template,
            &params.output_dir,
            &ExportOptions {
                card_edge: params.card_edge,
                bleed_px,
                border_rgb,
            },
        );
    }

    let bleed_px = compute_bleed_px(&required_scene, &[]);
    eprintln!("export: rendering tiles (bleed={bleed_px})…");
    export_scene_tiles(
        compositor,
        &required_scene,
        &required_decoded,
        background_rgb.as_ref(),
        template,
        &params.output_dir,
        &ExportOptions {
            card_edge: params.card_edge,
            bleed_px,
            border_rgb,
        },
    )
}

fn dx12_adapter_selector(
    adapters: &[wgpu::Adapter],
    _compatible_surface: Option<&wgpu::Surface<'_>>,
) -> std::result::Result<wgpu::Adapter, String> {
    fn score(adapter: &wgpu::Adapter) -> u8 {
        match adapter.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu => 4,
            wgpu::DeviceType::IntegratedGpu => 3,
            wgpu::DeviceType::VirtualGpu => 2,
            wgpu::DeviceType::Cpu => 1,
            wgpu::DeviceType::Other => 0,
        }
    }
    adapters
        .iter()
        .max_by_key(|a| score(a))
        .cloned()
        .or_else(|| adapters.first().cloned())
        .ok_or_else(|| "no DX12 or WARP adapter".to_string())
}

/// Offscreen DX12 compositor for export and preview underfill flatten (not tied to eframe).
pub fn create_offscreen_compositor() -> Result<Compositor> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..Default::default()
    });
    let adapters: Vec<_> = instance
        .enumerate_adapters(wgpu::Backends::DX12)
        .into_iter()
        .collect();
    let adapter = dx12_adapter_selector(&adapters, None).map_err(DecodeError::Gpu)?;
    eprintln!(
        "export: adapter {:?} ({})",
        adapter.get_info().device_type,
        adapter.get_info().name
    );
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("carousel-export"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: Default::default(),
        },
        None,
    ))
    .map_err(|e| DecodeError::Gpu(format!("request_device: {e}")))?;
    Ok(Compositor::new(device, queue))
}

pub fn run_strip_export(params: StripExportParams) -> Result<ExportResult> {
    eprintln!(
        "export: template={} seed={:?} edge={:?}",
        params.template.id, params.layout_seed, params.card_edge
    );
    let compositor = create_offscreen_compositor()?;
    let result = run_pipeline_on_gpu(&compositor, &params)?;
    eprintln!(
        "export: wrote {} slices + {}",
        result.slice_paths.len(),
        result.wide_path.display()
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::get_template_by_id;

    #[test]
    fn effective_seed_no_retry_for_10col() {
        let tpl = get_template_by_id("strip_10col").unwrap();
        let params = StripExportParams {
            template: tpl,
            fills: vec![],
            slot_fills: None,
            source_paths: vec![],
            layout_seed: 7,
            card_edge: crate::export::CardEdge::Borderless,
            border_rgb: None,
            no_layout_retry: false,
            locked_layout: None,
            output_dir: PathBuf::from("output"),
        };
        let bg = parse_color_rgb("#ece8e3");
        assert_eq!(effective_layout_seed(&params, bg), 7);
    }
}
