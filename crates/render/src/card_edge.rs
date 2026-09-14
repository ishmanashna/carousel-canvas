//! Wedges / border card edges (port of `composer.py` non-borderless branch).

use core::{polaroid_inner_dims, polaroid_margins, SceneCard};
use image::{imageops, Rgba, RgbaImage};

use crate::export::CardEdge;

pub const BORDER_WIDTH_PX: i32 = 8;

/// GPU texture dimension limit used across compositor upload paths.
pub const MAX_GPU_TEXTURE_DIM: u32 = 8192;

#[derive(Debug, Clone)]
pub struct RenderCard {
    pub image: RgbaImage,
    pub z: i32,
    pub rotation_deg: f32,
    pub center_x: f32,
    pub center_y: f32,
    pub polaroid: bool,
    pub slot_seed: u32,
    pub dest_w: i32,
    pub dest_h: i32,
    /// Drop-shadow pass before the card (figures and optional papers).
    pub cast_shadow: bool,
}

impl RenderCard {
    pub fn slot_center(&self) -> (f32, f32) {
        (self.center_x, self.center_y)
    }
}

fn flatten_polaroid_over_white(inner: &RgbaImage, dest_w: i32, dest_h: i32) -> RgbaImage {
    let (m_side, m_top, _m_bot) = polaroid_margins(dest_w, dest_h);
    let mut card = RgbaImage::from_pixel(
        dest_w.max(1) as u32,
        dest_h.max(1) as u32,
        Rgba([255, 255, 255, 255]),
    );
    let x = m_side.max(0) as i64;
    let y = m_top.max(0) as i64;
    imageops::overlay(&mut card, inner, x, y);
    card
}

fn add_border_rgb(img: &RgbaImage, border_px: i32, color: [u8; 3]) -> RgbaImage {
    let bp = border_px.max(1) as u32;
    let w = img.width() + 2 * bp;
    let h = img.height() + 2 * bp;
    let mut out = RgbaImage::from_pixel(w, h, Rgba([color[0], color[1], color[2], 255]));
    imageops::overlay(&mut out, img, bp as i64, bp as i64);
    out
}

/// Bilinear sample; returns `fill` when outside source bounds.
fn sample_bilinear(img: &RgbaImage, x: f32, y: f32, fill: [u8; 4]) -> [u8; 4] {
    let w = img.width() as f32;
    let h = img.height() as f32;
    if x < 0.0 || y < 0.0 || x >= w - 1.0 || y >= h - 1.0 {
        if x < 0.0 || y < 0.0 || x >= w || y >= h {
            return fill;
        }
    }
    let x0 = x.floor().max(0.0) as u32;
    let y0 = y.floor().max(0.0) as u32;
    let x1 = (x0 + 1).min(img.width().saturating_sub(1));
    let y1 = (y0 + 1).min(img.height().saturating_sub(1));
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let p00 = img.get_pixel(x0, y0).0;
    let p10 = img.get_pixel(x1, y0).0;
    let p01 = img.get_pixel(x0, y1).0;
    let p11 = img.get_pixel(x1, y1).0;
    let mut out = [0u8; 4];
    for c in 0..4 {
        let v = (1.0 - tx) * (1.0 - ty) * p00[c] as f32
            + tx * (1.0 - ty) * p10[c] as f32
            + (1.0 - tx) * ty * p01[c] as f32
            + tx * ty * p11[c] as f32;
        out[c] = v.round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// Rotate with expanded bounds and solid wedge fill (matches PIL expand + fillcolor).
pub fn rotate_expand_rgba(img: &RgbaImage, degrees: f64, fill: [u8; 4]) -> RgbaImage {
    if degrees.abs() < 1e-6 {
        return img.clone();
    }
    let rad = degrees.to_radians();
    let (sin, cos) = rad.sin_cos();
    let w = img.width() as f64;
    let h = img.height() as f64;
    let corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let cx = w * 0.5;
    let cy = h * 0.5;
    for (px, py) in corners {
        let dx = px - cx;
        let dy = py - cy;
        let rx = cx + dx * cos - dy * sin;
        let ry = cy + dx * sin + dy * cos;
        min_x = min_x.min(rx);
        min_y = min_y.min(ry);
        max_x = max_x.max(rx);
        max_y = max_y.max(ry);
    }
    let out_w = (max_x - min_x).ceil().max(1.0) as u32;
    let out_h = (max_y - min_y).ceil().max(1.0) as u32;
    let mut out = RgbaImage::from_pixel(out_w, out_h, Rgba(fill));
    let ocx = (min_x + max_x) * 0.5;
    let ocy = (min_y + max_y) * 0.5;
    for oy in 0..out_h {
        for ox in 0..out_w {
            let wx = ox as f64 + min_x;
            let wy = oy as f64 + min_y;
            let dx = wx - ocx;
            let dy = wy - ocy;
            let sx = cx + dx * cos + dy * sin;
            let sy = cy - dx * sin + dy * cos;
            let px = sample_bilinear(img, sx as f32, sy as f32, fill);
            out.put_pixel(ox, oy, Rgba(px));
        }
    }
    out
}

fn rgba_to_opaque_rgb(img: &RgbaImage) -> RgbaImage {
    let mut out = img.clone();
    for px in out.pixels_mut() {
        if px[3] < 255 {
            let a = px[3] as f32 / 255.0;
            for c in 0..3 {
                px[c] = ((px[c] as f32 * a) + 255.0 * (1.0 - a)).round() as u8;
            }
            px[3] = 255;
        }
    }
    out
}

/// Split a wide image into GPU-safe horizontal chunks (for axis-aligned draws).
pub fn split_horizontal_chunks(img: &RgbaImage, max_w: u32) -> Vec<(u32, RgbaImage)> {
    let w = img.width();
    if w <= max_w {
        return vec![(0, img.clone())];
    }
    let mut chunks = Vec::new();
    let mut x0 = 0u32;
    while x0 < w {
        let cw = (w - x0).min(max_w);
        chunks.push((x0, imageops::crop_imm(img, x0, 0, cw, img.height()).to_image()));
        x0 += cw;
    }
    chunks
}

fn feather_rect_alpha(img: &mut RgbaImage, px: u32) {
    if px == 0 {
        return;
    }
    let w = img.width();
    let h = img.height();
    if w == 0 || h == 0 {
        return;
    }
    let f = px as f32;
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32).min((w.saturating_sub(1) - x) as f32);
            let dy = (y as f32).min((h.saturating_sub(1) - y) as f32);
            let d = dx.min(dy);
            if d >= f {
                continue;
            }
            let t = (d / f).clamp(0.0, 1.0);
            let s = t * t * (3.0 - 2.0 * t);
            let p = img.get_pixel_mut(x, y);
            p[3] = (p[3] as f32 * s).round() as u8;
        }
    }
}

pub fn prepare_render_card(
    card: &SceneCard,
    decoded: &RgbaImage,
    edge: CardEdge,
    bg_rgb: [u8; 3],
    border_rgb: [u8; 3],
) -> RenderCard {
    let cx = card.dest.x as f32 + card.dest.w as f32 * 0.5;
    let cy = card.dest.y as f32 + card.dest.h as f32 * 0.5;

    let cast_shadow = card.cast_shadow;

    if edge == CardEdge::Borderless {
        let mut image = decoded.clone();
        if card.edge_feather_px > 0 && !card.cutout {
            feather_rect_alpha(&mut image, card.edge_feather_px);
        }
        return RenderCard {
            image,
            z: card.z,
            rotation_deg: card.rotation_deg as f32,
            center_x: cx,
            center_y: cy,
            polaroid: card.polaroid,
            slot_seed: card.slot_seed,
            dest_w: card.dest.w,
            dest_h: card.dest.h,
            cast_shadow,
        };
    }

    let mut img = if card.polaroid {
        let (iw, ih) = polaroid_inner_dims(card.dest.w, card.dest.h);
        let inner = if decoded.width() == iw as u32 && decoded.height() == ih as u32 {
            decoded.clone()
        } else {
            imageops::resize(
                decoded,
                iw.max(1) as u32,
                ih.max(1) as u32,
                imageops::FilterType::Triangle,
            )
        };
        flatten_polaroid_over_white(&inner, card.dest.w, card.dest.h)
    } else if card.cutout {
        decoded.clone()
    } else {
        rgba_to_opaque_rgb(decoded)
    };

    let mut wedge_fill = if card.polaroid {
        [255u8, 255, 255, 255]
    } else if card.cutout {
        [0, 0, 0, 0]
    } else {
        [bg_rgb[0], bg_rgb[1], bg_rgb[2], 255]
    };

    if edge == CardEdge::Border {
        img = add_border_rgb(&img, BORDER_WIDTH_PX, border_rgb);
        wedge_fill = [border_rgb[0], border_rgb[1], border_rgb[2], 255];
    }

    if card.polaroid {
        img = rotate_expand_rgba(&img, card.rotation_deg, wedge_fill);
    } else if card.rotation_deg.abs() > 1e-6 {
        img = rotate_expand_rgba(&img, card.rotation_deg, wedge_fill);
    }

    RenderCard {
        image: img,
        z: card.z,
        rotation_deg: 0.0,
        center_x: cx,
        center_y: cy,
        polaroid: false,
        slot_seed: card.slot_seed,
        dest_w: card.dest.w,
        dest_h: card.dest.h,
        cast_shadow,
    }
}

pub fn prepare_render_cards(
    scene_cards: &[SceneCard],
    decoded: &[crate::DecodedCard],
    edge: CardEdge,
    bg_rgb: [u8; 3],
    border_rgb: [u8; 3],
) -> Vec<RenderCard> {
    let mut cards: Vec<RenderCard> = scene_cards
        .iter()
        .zip(decoded.iter())
        .map(|(card, dec)| {
            prepare_render_card(card, dec.image.as_ref(), edge, bg_rgb, border_rgb)
        })
        .collect();
    cards.sort_by_key(|c| c.z);
    cards
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_expand_grows_bounds() {
        let img = RgbaImage::from_pixel(100, 200, Rgba([10, 20, 30, 255]));
        let out = rotate_expand_rgba(&img, 15.0, [0, 0, 0, 255]);
        assert!(out.width() > 100);
        assert!(out.height() > 200);
    }

    #[test]
    fn split_wide_image() {
        let img = RgbaImage::new(10_000, 100);
        let chunks = split_horizontal_chunks(&img, 8192);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].1.width(), 8192);
        assert_eq!(chunks[1].1.width(), 1808);
    }

    #[test]
    fn border_adds_pixels() {
        let img = RgbaImage::from_pixel(50, 50, Rgba([1, 2, 3, 255]));
        let out = add_border_rgb(&img, 8, [9, 9, 9]);
        assert_eq!(out.width(), 66);
        assert_eq!(out.height(), 66);
    }
}
