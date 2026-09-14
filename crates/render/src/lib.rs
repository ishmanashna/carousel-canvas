mod cache;
mod card_edge;
mod color;
mod compositor;
mod decode;
mod error;
mod export;
mod fit;
mod jpeg;
mod polaroid;
mod preview;
mod procedural_background;
mod strip_export;

pub use cache::DecodeCache;
pub use card_edge::{prepare_render_cards, RenderCard, BORDER_WIDTH_PX, MAX_GPU_TEXTURE_DIM};
pub use color::parse_color_rgb;
pub use compositor::{Compositor, tile_camera_rect};
pub use decode::decode_card_rgba;
pub use error::{DecodeError, Result};
pub use export::{CardEdge, ExportOptions, ExportResult};
pub use fit::{
    contain_resize_panned, cover_height_first_panned, cover_resize_and_crop_panned,
    trim_source_left, two_h_height_then_center_band,
};
pub use jpeg::{encode_jpeg_uncapped, encode_slice_jpeg, CAROUSEL_SLICE_MAX_BYTES};
pub use preview::{
    compose_preview_gpu, prepare_preview_from_snapshot, prepare_preview_phase1,
    prepare_preview_phase2, preview_compose_scale, preview_ready_without_underfill,
    render_preview_gpu, PreviewBuildResult, PreviewGpuReady, PreviewParams, PreviewPhase1,
};
pub use strip_export::{
    create_offscreen_compositor, decode_underfill_cards, flatten_and_plan_underfill,
    run_strip_export, StripExportParams,
};

use std::path::Path;
use std::sync::Arc;

use core::{polaroid_inner_dims, Fit, Scene, SceneCard, POLAROID_INNER_FILL};
use image::RgbaImage;
use rayon::prelude::*;

/// Decode/fit options for a single card (matches strip composer cache keys).
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeOptions {
    pub fit: Fit,
    pub pan_x: f64,
    pub pan_y: f64,
    pub flip_h: bool,
    pub source_trim_left_frac: Option<f64>,
    pub horizontal_center_band_frac: Option<f64>,
    pub cover_height_first: bool,
    pub contain_fill_rgb: [u8; 3],
    pub require_portrait: bool,
    pub require_landscape: bool,
    /// Keep RGBA through decode (cutout figures); apply `mask_path` when set.
    pub preserve_alpha: bool,
    pub mask_path: Option<std::path::PathBuf>,
    /// Oriented-source crop `[x, y, w, h]` so Cover fits the subject, not the full photo.
    pub source_crop: Option<[i32; 4]>,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            fit: Fit::Cover,
            pan_x: 0.0,
            pan_y: 0.0,
            flip_h: false,
            source_trim_left_frac: None,
            horizontal_center_band_frac: None,
            cover_height_first: false,
            contain_fill_rgb: [255, 255, 255],
            require_portrait: false,
            require_landscape: false,
            preserve_alpha: false,
            mask_path: None,
            source_crop: None,
        }
    }
}

impl From<&SceneCard> for DecodeOptions {
    fn from(card: &SceneCard) -> Self {
        Self {
            fit: card.fit,
            pan_x: card.pan_x,
            pan_y: card.pan_y,
            flip_h: card.flip_h,
            source_trim_left_frac: card.source_trim_left_frac,
            horizontal_center_band_frac: card.horizontal_center_band_frac,
            cover_height_first: card.cover_height_first,
            preserve_alpha: card.cutout,
            mask_path: card.mask_path.clone(),
            source_crop: card.source_crop,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecodedCard {
    pub index: usize,
    pub image: Arc<RgbaImage>,
}

/// Decode every scene card in parallel, using the shared cache.
pub fn decode_scene_cards(scene: &Scene, cache: &DecodeCache) -> Result<Vec<DecodedCard>> {
    let results: Vec<Result<DecodedCard>> = scene
        .cards
        .par_iter()
        .enumerate()
        .map(|(index, card)| {
            let (dest_w, dest_h) = if card.polaroid {
                let (iw, ih) = polaroid_inner_dims(card.dest.w, card.dest.h);
                (iw.max(1) as u32, ih.max(1) as u32)
            } else {
                (card.dest.w.max(1) as u32, card.dest.h.max(1) as u32)
            };
            let mut options = DecodeOptions::from(card);
            if card.polaroid {
                options.contain_fill_rgb = POLAROID_INNER_FILL;
            }
            if let Some(hit) = cache.get(&card.photo_path, dest_w, dest_h, &options) {
                return Ok(DecodedCard { index, image: hit });
            }
            let image = decode_card_rgba(&card.photo_path, dest_w, dest_h, &options)?;
            cache.insert(&card.photo_path, dest_w, dest_h, &options, image.clone());
            Ok(DecodedCard {
                index,
                image: Arc::new(image),
            })
        })
        .collect();

    let mut decoded = Vec::with_capacity(results.len());
    for item in results {
        decoded.push(item?);
    }
    decoded.sort_by_key(|c| c.index);
    Ok(decoded)
}

/// Decode options from a slot definition (for callers that have template slots, not scene cards).
pub fn decode_options_from_slot(slot: &core::StripSlotDef) -> DecodeOptions {
    DecodeOptions {
        fit: slot.fit,
        source_trim_left_frac: slot.source_trim_left_frac,
        horizontal_center_band_frac: slot.horizontal_center_band_frac,
        cover_height_first: slot.cover_height_first,
        require_portrait: slot.prefer_portrait,
        require_landscape: slot.prefer_landscape,
        preserve_alpha: slot.cutout,
        mask_path: slot.mask_path.clone(),
        source_crop: slot.source_crop,
        ..Default::default()
    }
}

pub fn render_version() -> &'static str {
    "render-0.7.0-phase7"
}

/// Phase 1 compatibility stub.
pub fn render_stub_version() -> &'static str {
    render_version()
}

/// Convenience for direct path decode with cache lookup/insert.
pub fn decode_card_rgba_cached(
    path: &Path,
    dest_w: u32,
    dest_h: u32,
    options: &DecodeOptions,
    cache: &DecodeCache,
) -> Result<Arc<RgbaImage>> {
    if let Some(hit) = cache.get(path, dest_w, dest_h, options) {
        return Ok(hit);
    }
    let image = decode_card_rgba(path, dest_w, dest_h, options)?;
    cache.insert(path, dest_w, dest_h, options, image.clone());
    Ok(Arc::new(image))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn cache_hit_returns_same_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.png");
        let img = image::RgbImage::from_pixel(8, 8, image::Rgb([1, 2, 3]));
        image::DynamicImage::ImageRgb8(img).save(&path).unwrap();
        let cache = DecodeCache::new();
        let options = DecodeOptions::default();
        let a = decode_card_rgba_cached(&path, 4, 4, &options, &cache).unwrap();
        let b = decode_card_rgba_cached(&path, 4, 4, &options, &cache).unwrap();
        assert_eq!(cache.len(), 1);
        assert_eq!(a.as_raw(), b.as_raw());
    }

    #[test]
    fn decode_options_from_scene_card() {
        let card = SceneCard {
            photo_path: PathBuf::from("x.jpg"),
            dest: core::Rect {
                x: 0,
                y: 0,
                w: 100,
                h: 200,
            },
            z: 0,
            rotation_deg: 0.0,
            fit: Fit::Contain,
            pan_x: 0.5,
            pan_y: -0.25,
            flip_h: true,
            source_trim_left_frac: Some(0.1),
            horizontal_center_band_frac: None,
            cover_height_first: false,
            polaroid: false,
            slot_seed: 0,
            cutout: false,
            mask_path: None,
            source_crop: None,
            cast_shadow: false,
            edge_feather_px: 0,
        };
        let opts = DecodeOptions::from(&card);
        assert_eq!(opts.fit, Fit::Contain);
        assert!(opts.flip_h);
        assert_eq!(opts.source_trim_left_frac, Some(0.1));
    }
}
