use std::path::{Path, PathBuf};

use core::{Scene, StripTemplate};
use image::RgbImage;

use crate::card_edge::prepare_render_cards;
use crate::color::parse_color_rgb;
use crate::compositor::Compositor;
use crate::error::{DecodeError, Result};
use crate::jpeg::{encode_jpeg_uncapped, encode_slice_jpeg, CAROUSEL_SLICE_MAX_BYTES};
use crate::DecodedCard;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardEdge {
    #[default]
    Borderless,
    Wedges,
    Border,
}

impl CardEdge {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "borderless" => Ok(Self::Borderless),
            "wedges" => Ok(Self::Wedges),
            "border" => Ok(Self::Border),
            other => Err(DecodeError::Encode(format!("unknown card_edge: {other}"))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub card_edge: CardEdge,
    pub bleed_px: i32,
    pub border_rgb: [u8; 3],
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            card_edge: CardEdge::Borderless,
            bleed_px: 0,
            border_rgb: [255, 255, 255],
        }
    }
}

#[derive(Debug)]
pub struct ExportResult {
    pub wide_path: PathBuf,
    pub slice_paths: Vec<PathBuf>,
}

fn stitch_wide(tiles: &[RgbImage]) -> Result<RgbImage> {
    let h = tiles[0].height();
    let w: u32 = tiles.iter().map(|t| t.width()).sum();
    let mut wide = RgbImage::new(w, h);
    let mut x = 0u32;
    for tile in tiles {
        for ty in 0..h {
            for tx in 0..tile.width() {
                wide.put_pixel(x + tx, ty, *tile.get_pixel(tx, ty));
            }
        }
        x += tile.width();
    }
    Ok(wide)
}

fn write_export_files(output_dir: &Path, tiles: &[RgbImage]) -> Result<ExportResult> {
    std::fs::create_dir_all(output_dir).map_err(DecodeError::from)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let rnd = (stamp % 900) + 100;
    let base = format!("carousel_{stamp}_{rnd}");
    let wide = stitch_wide(tiles)?;
    let wide_path = output_dir.join(format!("{base}_wide.jpg"));
    std::fs::write(&wide_path, encode_jpeg_uncapped(&wide)?).map_err(DecodeError::from)?;
    let mut slice_paths = Vec::with_capacity(tiles.len());
    for (i, tile) in tiles.iter().enumerate() {
        let p = output_dir.join(format!("{base}_{:02}.jpg", i + 1));
        std::fs::write(&p, encode_slice_jpeg(tile, CAROUSEL_SLICE_MAX_BYTES)?).map_err(DecodeError::from)?;
        slice_paths.push(p);
    }
    Ok(ExportResult { wide_path, slice_paths })
}

pub fn export_scene_tiles(
    compositor: &Compositor,
    scene: &Scene,
    decoded: &[DecodedCard],
    background_rgb: Option<&RgbImage>,
    template: &StripTemplate,
    output_dir: &Path,
    options: &ExportOptions,
) -> Result<ExportResult> {
    let bg_rgb = parse_color_rgb(&scene.background);
    let render_cards = prepare_render_cards(
        &scene.cards,
        decoded,
        options.card_edge,
        bg_rgb,
        options.border_rgb,
    );
    let tiles = compositor.render_all_tiles(
        &render_cards,
        &scene.background,
        background_rgb,
        template.slice_count,
        template.slice_width as u32,
        template.slice_height as u32,
        options.bleed_px,
    )?;
    write_export_files(output_dir, &tiles)
}
