//! Procedural strip backgrounds (port of `app.strip.procedural_backgrounds`).

use std::path::PathBuf;

use image::{Rgb, RgbImage};

use core::python_rng::PythonRandom;

const WOOD_POLAROID_TABLE: &str = "wood_polaroid_table";

fn asset_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/polaroid_table_wood.png")
}

pub fn render_procedural_background(kind: &str, width: u32, height: u32, seed: i64) -> RgbImage {
    match kind {
        WOOD_POLAROID_TABLE => wood_polaroid_table(width, height, seed),
        other => panic!("Unknown procedural background: {other:?}"),
    }
}

fn blend_rgb(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f64 + (b[0] as f64 - a[0] as f64) * t).round() as u8,
        (a[1] as f64 + (b[1] as f64 - a[1] as f64) * t).round() as u8,
        (a[2] as f64 + (b[2] as f64 - a[2] as f64) * t).round() as u8,
    ]
}

fn sample_tile_mean_rgb(tile: &RgbImage) -> [u8; 3] {
    let thumb = image::imageops::resize(tile, 32, 32, image::imageops::FilterType::Triangle);
    let mut r = 0u32;
    let mut g = 0u32;
    let mut b = 0u32;
    for px in thumb.pixels() {
        r += px[0] as u32;
        g += px[1] as u32;
        b += px[2] as u32;
    }
    let n = 32 * 32;
    [(r / n) as u8, (g / n) as u8, (b / n) as u8]
}

fn box_blur_rgb(img: &RgbImage, radius: u32) -> RgbImage {
    if radius == 0 {
        return img.clone();
    }
    let w = img.width();
    let h = img.height();
    let mut out = RgbImage::new(w, h);
    let r = radius as i32;
    for y in 0..h {
        for x in 0..w {
            let mut rs = 0u32;
            let mut gs = 0u32;
            let mut bs = 0u32;
            let mut n = 0u32;
            for dy in -r..=r {
                for dx in -r..=r {
                    let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as u32;
                    let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as u32;
                    let p = img.get_pixel(sx, sy);
                    rs += p[0] as u32;
                    gs += p[1] as u32;
                    bs += p[2] as u32;
                    n += 1;
                }
            }
            out.put_pixel(
                x,
                y,
                Rgb([
                    (rs / n) as u8,
                    (gs / n) as u8,
                    (bs / n) as u8,
                ]),
            );
        }
    }
    out
}

fn build_brick_row(
    tile: &RgbImage,
    tw: u32,
    th: u32,
    row_h: u32,
    out_w: u32,
    rng: &mut PythonRandom,
    oh: i32,
    brick_x: i32,
    tile_sy: u32,
) -> RgbImage {
    let mean = sample_tile_mean_rgb(tile);
    let mut row = RgbImage::from_pixel(out_w, row_h, Rgb(mean));
    let sy = tile_sy.min(th.saturating_sub(row_h));
    let mut x = -brick_x;
    while x < out_w as i32 {
        let px = x.max(0) as u32;
        let rem = out_w - px;
        if rem == 0 {
            break;
        }
        let pw = tw.min(rem);
        if pw < 1 {
            break;
        }
        let sx = rng.randint(0, (tw - pw).max(0) as i32) as u32;
        let piece = image::imageops::crop_imm(tile, sx, sy, pw, row_h).to_image();
        let pw2 = piece.width().min(out_w - px);
        let piece = image::imageops::crop_imm(&piece, 0, 0, pw2, row_h).to_image();
        image::imageops::overlay(&mut row, &piece, px as i64, 0);
        x += tw as i32 - oh;
    }
    row
}

fn stitch_brick_wood(tile: &RgbImage, w: u32, h: u32, seed: i64, mean_rgb: [u8; 3]) -> RgbImage {
    let (tw0, th0) = tile.dimensions();
    let target_tw = (tw0.max(480).min(960)) as f64;
    let th_r = ((th0 as f64 * target_tw / tw0 as f64).round() as u32).max(1);
    let tile = image::imageops::resize(tile, target_tw as u32, th_r, image::imageops::FilterType::CatmullRom);
    let (tw, th) = tile.dimensions();
    let mut rng = PythonRandom::new(((seed * 0xC001D00D) as u32) & 0xFFFFFFFF);
    let oh = (tw as i32 / 10).clamp(32, 92);
    let ov = (th as i32 / 11).clamp(20, 76);

    let mut out = RgbImage::from_pixel(w, h, Rgb(mean_rgb));
    let mut y_cursor = 0u32;
    let mut row_i = 0u32;
    let period = (tw as i32 - oh).max(1);
    let max_row_h = th.min(h);
    let tile_sy = rng.randint(0, (th - max_row_h.max(1)).max(0) as i32) as u32;

    while y_cursor < h {
        let remain = h - y_cursor;
        let mut row_h = th.min(remain);
        if remain > 0 && row_h < 10 {
            row_h = remain;
        }
        if row_h < 6 && remain > 0 {
            row_h = remain;
        }

        let base_brick = (row_i as i32 * (tw as i32 / 2)) % period;
        let brick_j = rng.randint(0, (period / 4).min(120));
        let brick_x = (base_brick + brick_j) % period;

        let row_img = build_brick_row(
            &tile,
            tw,
            th,
            row_h,
            w,
            &mut rng,
            oh,
            brick_x,
            tile_sy,
        );

        let touches_bottom = y_cursor + row_h >= h;
        if row_i == 0 || touches_bottom {
            image::imageops::overlay(&mut out, &row_img, 0, y_cursor as i64);
        } else {
            let ov_use = ov
                .min(row_h as i32 - 2)
                .min(y_cursor as i32)
                .min(row_h as i32 / 2)
                .max(0);
            if ov_use > 2 {
                paste_blend_vertical(&mut out, &row_img, 0, y_cursor, ov_use as u32);
            } else {
                image::imageops::overlay(&mut out, &row_img, 0, y_cursor as i64);
            }
        }

        if touches_bottom {
            break;
        }

        let ov_step = ov.min((row_h as i32 - 2).max(1));
        y_cursor += (row_h - ov_step as u32).max(1);
        row_i += 1;
    }
    out
}

fn paste_blend_vertical(dest: &mut RgbImage, patch: &RgbImage, px: u32, py: u32, overlap: u32) {
    if py == 0 || overlap <= 2 {
        image::imageops::overlay(dest, patch, px as i64, py as i64);
        return;
    }
    let ov = overlap.min(py).min(patch.height());
    if ov <= 2 {
        image::imageops::overlay(dest, patch, px as i64, py as i64);
        return;
    }
    let w0 = patch.width();
    for row in 0..ov {
        let t = row as f64 / (ov as f64 - 1.0).max(1.0);
        for x in 0..w0 {
            let top = dest.get_pixel(px + x, py - ov + row);
            let bot = patch.get_pixel(x, row);
            let blended = blend_rgb(top.0, bot.0, t);
            dest.put_pixel(px + x, py - ov + row, Rgb(blended));
        }
    }
    if patch.height() > ov {
        let rest = image::imageops::crop_imm(patch, 0, ov, w0, patch.height() - ov).to_image();
        image::imageops::overlay(dest, &rest, px as i64, py as i64);
    }
}

fn wood_from_asset(w: u32, h: u32, seed: i64) -> Option<RgbImage> {
    let path = asset_path();
    if !path.is_file() {
        return None;
    }
    let tile = image::open(&path).ok()?.into_rgb8();
    let mean_rgb = sample_tile_mean_rgb(&tile);
    let out = stitch_brick_wood(&tile, w, h, seed, mean_rgb);
    Some(box_blur_rgb(&out, 1))
}

fn wood_polaroid_table_procedural(w: u32, h: u32, seed: i64) -> RgbImage {
    let mut rng = PythonRandom::new(((seed * 1103515245 + 12345) as u32) & 0x7FFFFFFF);
    let base_lo = [98, 68, 46];
    let base_hi = [132, 94, 64];
    let mut img = RgbImage::from_pixel(w, h, Rgb(base_lo));

    let mut x = 0i32;
    while x < w as i32 {
        let pw = rng.randint(620, 1100);
        let t0 = rng.random();
        let c0 = blend_rgb(base_lo, base_hi, t0);
        let x1 = (x + pw).min(w as i32);
        if x1 <= x {
            break;
        }
        for yy in 0..h {
            for xx in x as u32..x1 as u32 {
                img.put_pixel(xx, yy, Rgb(c0));
            }
        }
        if x1 < w as i32 {
            let seam_x = ((x1 + rng.randint(-1, 1)) as u32).min(w - 1);
            let seam_w = rng.randint(1, 2) as u32;
            for yy in 0..h {
                for sx in seam_x..seam_x.saturating_add(seam_w).min(w) {
                    img.put_pixel(sx, yy, Rgb([52, 36, 24]));
                }
            }
        }
        if x1 >= w as i32 {
            break;
        }
        x = x1 + rng.randint(-3, 5);
        if x >= w as i32 {
            break;
        }
    }
    if x < w as i32 {
        let t0 = rng.random();
        let c0 = blend_rgb(base_lo, base_hi, t0);
        for yy in 0..h {
            for xx in x as u32..w {
                img.put_pixel(xx, yy, Rgb(c0));
            }
        }
    }

    let grain_h = (h / 18).max(72);
    let mut grain = RgbImage::new(480, grain_h);
    for _ in 0..2500 {
        let gx = rng.randbelow(grain.width()) as u32;
        let gy = rng.randbelow(grain.height()) as u32;
        let gc = blend_rgb(base_lo, base_hi, rng.uniform(0.15, 0.85));
        grain.put_pixel(gx, gy, Rgb(gc));
    }
    grain = box_blur_rgb(&grain, 4);
    grain = image::imageops::resize(&grain, w, h, image::imageops::FilterType::Triangle);

    let mut out = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let a = img.get_pixel(x, y).0;
            let b = grain.get_pixel(x, y).0;
            let t = 0.07;
            let px = [
                (a[0] as f64 * (1.0 - t) + b[0] as f64 * t).round() as u8,
                (a[1] as f64 * (1.0 - t) + b[1] as f64 * t).round() as u8,
                (a[2] as f64 * (1.0 - t) + b[2] as f64 * t).round() as u8,
            ];
            out.put_pixel(x, y, Rgb(px));
        }
    }
    out
}

fn wood_polaroid_table(w: u32, h: u32, seed: i64) -> RgbImage {
    if let Some(baked) = wood_from_asset(w, h, seed) {
        return baked;
    }
    wood_polaroid_table_procedural(w, h, seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wood_procedural_dimensions() {
        let img = render_procedural_background(WOOD_POLAROID_TABLE, 10800, 1350, 7);
        assert_eq!(img.dimensions(), (10800, 1350));
    }

    #[test]
    fn wood_deterministic() {
        let a = render_procedural_background(WOOD_POLAROID_TABLE, 400, 200, 42);
        let b = render_procedural_background(WOOD_POLAROID_TABLE, 400, 200, 42);
        assert_eq!(a.as_raw(), b.as_raw());
    }

    #[test]
    fn wood_not_flat_fill() {
        let img = render_procedural_background(WOOD_POLAROID_TABLE, 800, 400, 3);
        let p0 = img.get_pixel(0, 0).0;
        let p1 = img.get_pixel(400, 200).0;
        assert_ne!(p0, p1);
    }

    #[test]
    fn wood_asset_png_is_shipped() {
        let path = asset_path();
        assert!(
            path.is_file(),
            "expected polaroid wood tile at {}",
            path.display()
        );
        assert!(wood_from_asset(400, 200, 7).is_some());
    }
}
