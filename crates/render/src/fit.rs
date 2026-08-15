//! Cover/contain and strip-specific source transforms (matches `app/image_ops.py`).

use image::imageops::{self, FilterType};
use image::{DynamicImage, GenericImageView, Rgb, RgbImage, RgbaImage};

pub fn trim_source_left(img: DynamicImage, source_trim_left_frac: Option<f64>) -> DynamicImage {
    let Some(raw) = source_trim_left_frac else {
        return img;
    };
    if raw <= 0.0 {
        return img;
    }
    let frac = raw.clamp(0.0, 0.49);
    let (iw, ih) = img.dimensions();
    let cut = (iw as f64 * frac).round() as u32;
    if cut > 0 && cut < iw.saturating_sub(1) {
        return img.crop_imm(cut, 0, iw - cut, ih);
    }
    img
}

pub fn two_h_height_then_center_band(
    img: DynamicImage,
    slot_height: u32,
    band_frac: f64,
) -> DynamicImage {
    let (iw, ih) = img.dimensions();
    let th = slot_height.max(1);
    let s = th as f64 / (ih.max(1) as f64);
    let nw = ((iw as f64 * s).round() as u32).max(1);
    let img = resize_bilinear(&img, nw, th);
    let frac = band_frac.clamp(0.05, 1.0);
    let band_h = ((th as f64 * frac).round() as u32).max(1);
    let top = (th.saturating_sub(band_h)) / 2;
    let crop_h = band_h.min(th.saturating_sub(top));
    img.crop_imm(0, top, nw, crop_h)
}

pub fn cover_resize_and_crop_panned(
    img: &DynamicImage,
    target_width: u32,
    target_height: u32,
    pan_x: f64,
    pan_y: f64,
) -> DynamicImage {
    let (tw, th) = (target_width.max(1), target_height.max(1));
    let (iw, ih) = img.dimensions();
    let width_ratio = tw as f64 / iw.max(1) as f64;
    let height_ratio = th as f64 / ih.max(1) as f64;
    let ratio = width_ratio.max(height_ratio);
    let new_width = ((iw as f64 * ratio).round() as u32).max(1);
    let new_height = ((ih as f64 * ratio).round() as u32).max(1);
    let resized = resize_bilinear(img, new_width, new_height);
    let excess_w = new_width.saturating_sub(tw);
    let excess_h = new_height.saturating_sub(th);
    let left = if excess_w > 0 {
        let v = ((1.0 + pan_x) * excess_w as f64 / 2.0).round() as u32;
        v.clamp(0, excess_w)
    } else {
        0
    };
    let top = if excess_h > 0 {
        let v = ((1.0 + pan_y) * excess_h as f64 / 2.0).round() as u32;
        v.clamp(0, excess_h)
    } else {
        0
    };
    resized.crop_imm(left, top, tw, th)
}

pub fn cover_height_first_panned(
    img: &DynamicImage,
    target_width: u32,
    target_height: u32,
    pan_x: f64,
    pan_y: f64,
) -> DynamicImage {
    let (tw, th) = (target_width.max(1), target_height.max(1));
    let (iw, ih) = img.dimensions();
    let s = th as f64 / ih.max(1) as f64;
    let nw = ((iw as f64 * s).round() as u32).max(1);
    let mut working = resize_bilinear(img, nw, th);
    if nw >= tw {
        let excess_w = nw.saturating_sub(tw);
        let left = if excess_w > 0 {
            let v = ((1.0 + pan_x) * excess_w as f64 / 2.0).round() as u32;
            v.clamp(0, excess_w)
        } else {
            0
        };
        return working.crop_imm(left, 0, tw, th);
    }
    let nh_new = (th as f64 * (tw as f64 / nw as f64))
        .round()
        .max(th as f64) as u32;
    working = resize_bilinear(&working, tw, nh_new);
    let excess_h = nh_new.saturating_sub(th);
    let top = if excess_h > 0 {
        let v = ((1.0 + pan_y) * excess_h as f64 / 2.0).round() as u32;
        v.clamp(0, excess_h)
    } else {
        0
    };
    working.crop_imm(0, top, tw, th)
}

pub fn contain_resize_panned(
    img: &DynamicImage,
    target_width: u32,
    target_height: u32,
    pan_x: f64,
    pan_y: f64,
    fill_rgb: [u8; 3],
) -> DynamicImage {
    let (tw, th) = (target_width.max(1), target_height.max(1));
    let (iw, ih) = img.dimensions();
    let scale = (tw as f64 / iw.max(1) as f64).min(th as f64 / ih.max(1) as f64);
    let nw = ((iw as f64 * scale).round() as u32).max(1);
    let nh = ((ih as f64 * scale).round() as u32).max(1);
    let resized = resize_bilinear(img, nw, nh);
    let mut plate = RgbImage::from_pixel(tw, th, Rgb(fill_rgb));
    let excess_x = tw.saturating_sub(nw);
    let excess_y = th.saturating_sub(nh);
    let px = if excess_x > 0 {
        let v = ((1.0 + pan_x) * excess_x as f64 / 2.0).round() as u32;
        v.clamp(0, excess_x)
    } else {
        0
    };
    let py = if excess_y > 0 {
        let v = ((1.0 + pan_y) * excess_y as f64 / 2.0).round() as u32;
        v.clamp(0, excess_y)
    } else {
        0
    };
    imageops::overlay(&mut plate, &resized.to_rgb8(), px.into(), py.into());
    DynamicImage::ImageRgb8(plate)
}

pub fn resize_bilinear(img: &DynamicImage, new_w: u32, new_h: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(imageops::resize(
        &img.to_rgb8(),
        new_w.max(1),
        new_h.max(1),
        FilterType::Triangle,
    ))
}

pub fn rgb_to_rgba(img: DynamicImage) -> RgbaImage {
    img.to_rgba8()
}

pub fn flip_horizontal(img: DynamicImage) -> DynamicImage {
    DynamicImage::ImageRgb8(imageops::flip_horizontal(&img.to_rgb8()))
}

/// Estimate the largest intermediate dimension needed before the final crop.
pub fn estimate_max_intermediate_dim(
    src_w: u32,
    src_h: u32,
    dest_w: u32,
    dest_h: u32,
    source_trim_left_frac: Option<f64>,
    horizontal_center_band_frac: Option<f64>,
    cover_height_first: bool,
    fit: core::Fit,
) -> u32 {
    let mut w = src_w.max(1);
    let mut h = src_h.max(1);
    if let Some(frac) = source_trim_left_frac {
        if frac > 0.0 {
            let cut = (w as f64 * frac.clamp(0.0, 0.49)).round() as u32;
            w = w.saturating_sub(cut).max(1);
        }
    }
    let tw = dest_w.max(1);
    let th = dest_h.max(1);
    if horizontal_center_band_frac.is_some() {
        let s = th as f64 / h as f64;
        w = ((w as f64 * s).round() as u32).max(1);
        h = th;
    }
    let max_dim = match fit {
        core::Fit::Contain => {
            let scale = (tw as f64 / w as f64).min(th as f64 / h as f64);
            let nw = ((w as f64 * scale).round() as u32).max(1);
            let nh = ((h as f64 * scale).round() as u32).max(1);
            nw.max(nh).max(tw).max(th)
        }
        core::Fit::Cover if cover_height_first => {
            let nw = ((w as f64 * (th as f64 / h as f64)).round() as u32).max(1);
            if nw >= tw {
                nw.max(th)
            } else {
                let nh = (th as f64 * (tw as f64 / nw as f64)).round() as u32;
                tw.max(nh).max(th)
            }
        }
        core::Fit::Cover => {
            let ratio = (tw as f64 / w as f64).max(th as f64 / h as f64);
            let nw = ((w as f64 * ratio).round() as u32).max(1);
            let nh = ((h as f64 * ratio).round() as u32).max(1);
            nw.max(nh)
        }
    };
    max_dim.max(tw).max(th)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn solid(w: u32, h: u32, color: [u8; 3]) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb(color)))
    }

    #[test]
    fn cover_center_crop_is_symmetric() {
        let img = solid(4, 2, [255, 0, 0]);
        let out = cover_resize_and_crop_panned(&img, 2, 2, 0.0, 0.0);
        assert_eq!(out.dimensions(), (2, 2));
        assert_eq!(out.to_rgb8().get_pixel(0, 0).0, [255, 0, 0]);
    }

    #[test]
    fn cover_pan_shifts_crop() {
        let mut img = RgbImage::new(4, 2);
        for x in 0..4 {
            for y in 0..2 {
                let v = if x < 2 { 10 } else { 20 };
                img.put_pixel(x, y, Rgb([v, 0, 0]));
            }
        }
        let img = DynamicImage::ImageRgb8(img);
        let left = cover_resize_and_crop_panned(&img, 2, 2, -1.0, 0.0);
        let right = cover_resize_and_crop_panned(&img, 2, 2, 1.0, 0.0);
        assert_eq!(left.to_rgb8().get_pixel(0, 0).0[0], 10);
        assert_eq!(right.to_rgb8().get_pixel(0, 0).0[0], 20);
    }

    #[test]
    fn trim_left_removes_fraction() {
        let mut img = RgbImage::new(10, 2);
        for x in 0..10 {
            img.put_pixel(x, 0, Rgb([x as u8, 0, 0]));
            img.put_pixel(x, 1, Rgb([x as u8, 0, 0]));
        }
        let trimmed = trim_source_left(DynamicImage::ImageRgb8(img), Some(0.2));
        assert_eq!(trimmed.dimensions().0, 8);
        assert_eq!(trimmed.to_rgb8().get_pixel(0, 0).0[0], 2);
    }

    #[test]
    fn center_band_keeps_middle_rows() {
        let mut img = RgbImage::new(4, 4);
        for y in 0..4 {
            for x in 0..4 {
                img.put_pixel(x, y, Rgb([(y * 40) as u8, 0, 0]));
            }
        }
        let banded = two_h_height_then_center_band(DynamicImage::ImageRgb8(img), 4, 0.5);
        assert_eq!(banded.dimensions(), (4, 2));
        assert_eq!(banded.to_rgb8().get_pixel(0, 0).0[0], 40);
        assert_eq!(banded.to_rgb8().get_pixel(0, 1).0[0], 80);
    }

    #[test]
    fn contain_letterboxes() {
        let img = solid(4, 2, [0, 255, 0]);
        let out = contain_resize_panned(&img, 4, 4, 0.0, 0.0, [255, 255, 255]);
        assert_eq!(out.dimensions(), (4, 4));
        assert_eq!(out.to_rgb8().get_pixel(0, 0).0, [255, 255, 255]);
        assert_eq!(out.to_rgb8().get_pixel(1, 1).0, [0, 255, 0]);
    }

    #[test]
    fn cover_height_first_prefers_full_height() {
        let img = solid(2, 4, [0, 0, 255]);
        let out = cover_height_first_panned(&img, 4, 4, 0.0, 0.0);
        assert_eq!(out.dimensions(), (4, 4));
        assert_eq!(out.to_rgb8().get_pixel(0, 0).0, [0, 0, 255]);
    }
}
