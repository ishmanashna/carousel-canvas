use std::fs::File;
use std::io::Read;
use std::path::Path;

use image::imageops;
use image::metadata::Orientation;
use image::{DynamicImage, GenericImageView, GrayImage, ImageDecoder, ImageReader, RgbaImage};
use jpeg_decoder::{Decoder, PixelFormat};
use core::{harden_cutout_alpha, isolate_largest_blob};

use crate::error::{DecodeError, Result};
use crate::fit::{
    contain_resize_panned, cover_height_first_panned, cover_resize_and_crop_panned,
    estimate_max_intermediate_dim, flip_horizontal, resize_bilinear, rgb_to_rgba,
    trim_source_left, two_h_height_then_center_band,
};
use crate::DecodeOptions;

struct SourceMeta {
    orientation: Orientation,
    width: u32,
    height: u32,
    is_jpeg: bool,
    jpeg_bytes: Option<Vec<u8>>,
}

fn is_jpeg_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|s| {
            let lower = s.to_ascii_lowercase();
            lower == "jpg" || lower == "jpeg"
        })
        .unwrap_or(false)
}

fn read_source_meta(path: &Path) -> Result<SourceMeta> {
    if is_jpeg_path(path) {
        let mut bytes = Vec::new();
        File::open(path)?.read_to_end(&mut bytes)?;
        let orientation = {
            let reader =
                ImageReader::new(std::io::Cursor::new(bytes.as_slice())).with_guessed_format()?;
            let mut decoder = reader.into_decoder()?;
            decoder.orientation().unwrap_or(Orientation::NoTransforms)
        };
        let mut jd = Decoder::new(std::io::Cursor::new(&bytes));
        jd.read_info()?;
        let info = jd.info().ok_or(DecodeError::EmptySource)?;
        return Ok(SourceMeta {
            orientation,
            width: u32::from(info.width),
            height: u32::from(info.height),
            is_jpeg: true,
            jpeg_bytes: Some(bytes),
        });
    }

    let reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let (width, height) = decoder.dimensions();
    Ok(SourceMeta {
        orientation,
        width,
        height,
        is_jpeg: false,
        jpeg_bytes: None,
    })
}

fn oriented_dimensions(width: u32, height: u32, orientation: Orientation) -> (u32, u32) {
    match orientation {
        Orientation::NoTransforms
        | Orientation::Rotate180
        | Orientation::FlipHorizontal
        | Orientation::FlipVertical => (width, height),
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => (height, width),
    }
}

fn apply_orientation(mut img: DynamicImage, orientation: Orientation) -> DynamicImage {
    img.apply_orientation(orientation);
    img
}

fn flatten_alpha(img: DynamicImage) -> DynamicImage {
    match img {
        DynamicImage::ImageRgb8(rgb) => DynamicImage::ImageRgb8(rgb),
        other => DynamicImage::ImageRgb8(other.to_rgb8()),
    }
}

fn decode_jpeg_bytes_scaled(bytes: &[u8], need_max_dim: u32) -> Result<DynamicImage> {
    let mut decoder = Decoder::new(std::io::Cursor::new(bytes));
    decoder.read_info()?;
    let info = decoder.info().ok_or(DecodeError::EmptySource)?;
    if info.width == 0 || info.height == 0 {
        return Err(DecodeError::EmptySource);
    }
    let need = need_max_dim.max(1).min(u16::MAX as u32) as u16;
    decoder.scale(need, need)?;
    let pixels = decoder.decode()?;
    let out = decoder.info().ok_or(DecodeError::EmptySource)?;
    let dw = u32::from(out.width);
    let dh = u32::from(out.height);
    match out.pixel_format {
        PixelFormat::RGB24 => {
            let rgb = image::RgbImage::from_raw(dw, dh, pixels).ok_or(DecodeError::EmptySource)?;
            Ok(DynamicImage::ImageRgb8(rgb))
        }
        PixelFormat::L8 => {
            let gray =
                image::GrayImage::from_raw(dw, dh, pixels).ok_or(DecodeError::EmptySource)?;
            Ok(DynamicImage::ImageLuma8(gray))
        }
        PixelFormat::L16 | PixelFormat::CMYK32 => {
            let reader = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
            Ok(DynamicImage::from_decoder(reader.into_decoder()?)?)
        }
    }
}

fn decode_png_limited(path: &Path, need_max_dim: u32) -> Result<DynamicImage> {
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    let decoder = reader.into_decoder()?;
    let (sw, sh) = decoder.dimensions();
    let mut img = DynamicImage::from_decoder(decoder)?;
    let max_src = sw.max(sh).max(1);
    let need = need_max_dim.max(1);
    if max_src > need.saturating_mul(2) {
        let scale = need as f64 / max_src as f64;
        let nw = ((sw as f64 * scale).round() as u32).max(1);
        let nh = ((sh as f64 * scale).round() as u32).max(1);
        img = resize_bilinear(&img, nw, nh);
    }
    Ok(img)
}

fn load_rgb_at_most(
    meta: &SourceMeta,
    path: &Path,
    need_max_dim: u32,
    preserve_alpha: bool,
) -> Result<DynamicImage> {
    let img = if meta.is_jpeg {
        let bytes = meta.jpeg_bytes.as_deref().ok_or(DecodeError::EmptySource)?;
        decode_jpeg_bytes_scaled(bytes, need_max_dim)?
    } else {
        match path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("png") => decode_png_limited(path, need_max_dim)?,
            _ => {
                let reader = ImageReader::open(path)?.with_guessed_format()?;
                let decoder = reader.into_decoder()?;
                let (sw, sh) = decoder.dimensions();
                let mut loaded = DynamicImage::from_decoder(decoder)?;
                let max_src = sw.max(sh).max(1);
                if max_src > need_max_dim.saturating_mul(2) {
                    let scale = need_max_dim as f64 / max_src as f64;
                    let nw = ((sw as f64 * scale).round() as u32).max(1);
                    let nh = ((sh as f64 * scale).round() as u32).max(1);
                    loaded = resize_bilinear(&loaded, nw, nh);
                }
                loaded
            }
        }
    };
    let oriented = apply_orientation(img, meta.orientation);
    if preserve_alpha {
        Ok(oriented)
    } else {
        Ok(flatten_alpha(oriented))
    }
}

/// Mask PNGs are stored in oriented source space (see `vision` crate).
fn load_mask_gray(mask_path: &Path, need_max_dim: u32) -> Result<GrayImage> {
    let reader = ImageReader::open(mask_path)?.with_guessed_format()?;
    let decoder = reader.into_decoder()?;
    let (mw, mh) = decoder.dimensions();
    let mut mask = DynamicImage::from_decoder(decoder)?.into_luma8();
    let max_src = mw.max(mh).max(1);
    let need = need_max_dim.max(1);
    if max_src > need.saturating_mul(2) {
        let scale = need as f64 / max_src as f64;
        let nw = ((mw as f64 * scale).round() as u32).max(1);
        let nh = ((mh as f64 * scale).round() as u32).max(1);
        mask = imageops::resize(&mask, nw, nh, imageops::FilterType::Triangle);
    }
    Ok(mask)
}

fn align_mask_to_photo(mask: GrayImage, photo: &DynamicImage) -> GrayImage {
    let (pw, ph) = photo.dimensions();
    let (mw, mh) = mask.dimensions();
    if mw == pw && mh == ph {
        return mask;
    }
    imageops::resize(&mask, pw.max(1), ph.max(1), imageops::FilterType::Triangle)
}

fn apply_source_crop(
    photo: DynamicImage,
    mask: Option<GrayImage>,
    crop: [i32; 4],
    oriented_w: u32,
    oriented_h: u32,
) -> (DynamicImage, Option<GrayImage>) {
    let loaded_w = photo.width();
    let loaded_h = photo.height();
    let sx = loaded_w as f64 / oriented_w.max(1) as f64;
    let sy = loaded_h as f64 / oriented_h.max(1) as f64;
    let mut x = (crop[0] as f64 * sx).floor() as i32;
    let mut y = (crop[1] as f64 * sy).floor() as i32;
    let mut w = (crop[2] as f64 * sx).ceil() as i32;
    let mut h = (crop[3] as f64 * sy).ceil() as i32;
    if x < 0 {
        w += x;
        x = 0;
    }
    if y < 0 {
        h += y;
        y = 0;
    }
    w = w.min(loaded_w as i32 - x);
    h = h.min(loaded_h as i32 - y);
    if w <= 0 || h <= 0 {
        return (photo, mask);
    }
    let xu = x as u32;
    let yu = y as u32;
    let wu = w as u32;
    let hu = h as u32;
    let photo = photo.crop_imm(xu, yu, wu, hu);
    let mask = mask.map(|m| imageops::crop_imm(&m, xu, yu, wu, hu).to_image());
    (photo, mask)
}

fn decode_need_max(src_w: u32, src_h: u32, dest_w: u32, dest_h: u32, options: &DecodeOptions) -> u32 {
    let base = estimate_max_intermediate_dim(
        src_w,
        src_h,
        dest_w,
        dest_h,
        options.source_trim_left_frac,
        options.horizontal_center_band_frac,
        options.cover_height_first,
        options.fit,
    );
    let Some([_, _, cw, ch]) = options.source_crop else {
        return base;
    };
    let crop_long = cw.max(ch).max(1) as f64;
    let src_long = src_w.max(src_h) as f64;
    let boosted = (base as f64 * src_long / crop_long).ceil() as u32;
    boosted.max(base).min(src_w.max(src_h).max(1))
}

fn fit_mask_to_destination(
    mask: GrayImage,
    dest_w: u32,
    dest_h: u32,
    options: &DecodeOptions,
) -> GrayImage {
    let fitted = fit_to_destination(DynamicImage::ImageLuma8(mask), dest_w, dest_h, options);
    fitted.into_luma8()
}

/// Cheap fringe decontamination: unmix a white-ish background from semi-transparent RGB.
fn decontaminate_fringe(rgba: &mut RgbaImage, bg_rgb: [u8; 3]) {
    for pixel in rgba.pixels_mut() {
        let a = pixel[3] as f32 / 255.0;
        if a <= 0.02 || a >= 0.98 {
            continue;
        }
        for c in 0..3 {
            let observed = pixel[c] as f32;
            let bg = bg_rgb[c] as f32;
            let fg = (observed - (1.0 - a) * bg) / a;
            pixel[c] = fg.clamp(0.0, 255.0) as u8;
        }
    }
}

fn apply_mask_and_decontaminate(
    rgba: RgbaImage,
    mask: &GrayImage,
    bg_rgb: [u8; 3],
) -> RgbaImage {
    assert_eq!(rgba.dimensions(), mask.dimensions());
    let mask = isolate_largest_blob(mask);
    let mut out = rgba;
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        pixel[3] = harden_cutout_alpha(mask.get_pixel(x, y)[0]);
    }
    decontaminate_fringe(&mut out, bg_rgb);
    out
}

fn orientation_ok(img: &DynamicImage, options: &DecodeOptions) -> bool {
    if options.require_portrait && img.width() > img.height() {
        return false;
    }
    if options.require_landscape && img.height() > img.width() {
        return false;
    }
    true
}

fn fit_to_destination(
    mut img: DynamicImage,
    dest_w: u32,
    dest_h: u32,
    options: &DecodeOptions,
) -> DynamicImage {
    let pan_x = options.pan_x.clamp(-1.0, 1.0);
    let pan_y = options.pan_y.clamp(-1.0, 1.0);
    img = trim_source_left(img, options.source_trim_left_frac);

    match options.fit {
        core::Fit::Contain => {
            if options.flip_h {
                img = flip_horizontal(img);
            }
            if let Some(frac) = options.horizontal_center_band_frac {
                img = two_h_height_then_center_band(img, dest_h, frac);
            }
            contain_resize_panned(&img, dest_w, dest_h, pan_x, pan_y, options.contain_fill_rgb)
        }
        core::Fit::Cover => {
            if let Some(frac) = options.horizontal_center_band_frac {
                img = two_h_height_then_center_band(img, dest_h, frac);
                img = cover_resize_and_crop_panned(&img, dest_w, dest_h, pan_x, pan_y);
            } else if options.cover_height_first {
                img = cover_height_first_panned(&img, dest_w, dest_h, pan_x, pan_y);
            } else {
                img = cover_resize_and_crop_panned(&img, dest_w, dest_h, pan_x, pan_y);
            }
            if options.flip_h {
                img = flip_horizontal(img);
            }
            img
        }
    }
}

/// Decode and fit a photo to the destination box (CPU only, no full-res retention).
pub fn decode_card_rgba(
    path: &Path,
    dest_w: u32,
    dest_h: u32,
    options: &DecodeOptions,
) -> Result<image::RgbaImage> {
    if !path.is_file() {
        return Err(DecodeError::Path(path.to_path_buf()));
    }
    let dest_w = dest_w.max(1);
    let dest_h = dest_h.max(1);
    let meta = read_source_meta(path)?;
    let (src_w, src_h) = oriented_dimensions(meta.width, meta.height, meta.orientation);
    if src_w == 0 || src_h == 0 {
        return Err(DecodeError::EmptySource);
    }
    let need_max = decode_need_max(src_w, src_h, dest_w, dest_h, options);
    let loaded = load_rgb_at_most(&meta, path, need_max, options.preserve_alpha)?;
    if !orientation_ok(&loaded, options) {
        return Err(DecodeError::OrientationMismatch);
    }
    let mask_aligned = if options.preserve_alpha {
        let mask_path = options
            .mask_path
            .as_ref()
            .ok_or_else(|| DecodeError::Gpu("cutout decode requires mask_path".into()))?;
        if !mask_path.is_file() {
            return Err(DecodeError::Path(mask_path.clone()));
        }
        let mask = load_mask_gray(mask_path, need_max)?;
        Some(align_mask_to_photo(mask, &loaded))
    } else {
        None
    };
    let (loaded, mask_aligned) = if let Some(crop) = options.source_crop {
        apply_source_crop(loaded, mask_aligned, crop, src_w, src_h)
    } else {
        (loaded, mask_aligned)
    };
    let fitted_mask =
        mask_aligned.map(|mask| fit_mask_to_destination(mask, dest_w, dest_h, options));
    let fitted = fit_to_destination(loaded, dest_w, dest_h, options);
    let (out_w, out_h) = fitted.dimensions();
    tracing::info!(
        path = %path.display(),
        dest_w,
        dest_h,
        decoded_w = out_w,
        decoded_h = out_h,
        need_max,
        preserve_alpha = options.preserve_alpha,
        "decode_card_rgba"
    );
    let rgba = rgb_to_rgba(fitted);
    if let Some(mask) = fitted_mask {
        Ok(apply_mask_and_decontaminate(rgba, &mask, options.contain_fill_rgb))
    } else {
        Ok(rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Luma, Rgb};

    #[test]
    fn oriented_dimensions_swap_on_90() {
        assert_eq!(
            oriented_dimensions(4000, 3000, Orientation::Rotate90),
            (3000, 4000)
        );
    }

    #[test]
    fn decode_generated_png_to_small_box() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tiny.png");
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(32, 16, |x, _y| {
            if x < 16 {
                Rgb([255, 0, 0])
            } else {
                Rgb([0, 0, 255])
            }
        });
        img.save(&path).unwrap();
        let options = DecodeOptions::default();
        let out = decode_card_rgba(&path, 8, 8, &options).unwrap();
        assert_eq!(out.dimensions(), (8, 8));
    }

    #[test]
    fn cutout_mask_produces_transparent_pixels() {
        use image::{GrayImage, Luma};

        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("photo.png");
        let mask = dir.path().join("mask.png");
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(32, 32, Rgb([40, 80, 120]));
        img.save(&photo).unwrap();
        let mask_img: GrayImage = ImageBuffer::from_fn(32, 32, |x, y| {
            let cx = 16.0;
            let cy = 16.0;
            let r = 8.0;
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            if dx * dx + dy * dy <= r * r {
                Luma([255])
            } else {
                Luma([0])
            }
        });
        mask_img.save(&mask).unwrap();

        let options = DecodeOptions {
            preserve_alpha: true,
            mask_path: Some(mask),
            ..DecodeOptions::default()
        };
        let out = decode_card_rgba(&photo, 16, 16, &options).unwrap();
        assert_eq!(out.dimensions(), (16, 16));
        let min_alpha = out.pixels().map(|p| p[3]).min().unwrap_or(255);
        assert!(min_alpha < 255, "expected some transparent pixels from mask");
        let max_alpha = out.pixels().map(|p| p[3]).max().unwrap_or(0);
        assert!(max_alpha > 0, "expected some opaque pixels from mask");
    }

    #[test]
    fn soft_mask_fog_becomes_transparent() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("photo.png");
        let mask = dir.path().join("mask.png");
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(16, 16, Rgb([10, 20, 30]));
        img.save(&photo).unwrap();
        let mask_img: GrayImage = ImageBuffer::from_fn(16, 16, |x, y| {
            if x >= 6 && x < 10 && y >= 6 && y < 10 {
                Luma([255])
            } else {
                Luma([90])
            }
        });
        mask_img.save(&mask).unwrap();
        let options = DecodeOptions {
            preserve_alpha: true,
            mask_path: Some(mask),
            ..DecodeOptions::default()
        };
        let out = decode_card_rgba(&photo, 16, 16, &options).unwrap();
        assert_eq!(out.get_pixel(0, 0)[3], 0, "fog must not survive as alpha");
        assert_eq!(out.get_pixel(8, 8)[3], 255);
    }

    #[test]
    fn source_crop_keeps_off_center_subject() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("photo.png");
        let mask = dir.path().join("mask.png");
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(64, 32, |x, _y| {
            if x >= 48 {
                Rgb([220, 30, 30])
            } else {
                Rgb([0, 0, 0])
            }
        });
        img.save(&photo).unwrap();
        let mask_img: GrayImage = ImageBuffer::from_fn(64, 32, |x, _y| {
            if x >= 48 {
                Luma([255])
            } else {
                Luma([0])
            }
        });
        mask_img.save(&mask).unwrap();

        let missed = decode_card_rgba(
            &photo,
            8,
            16,
            &DecodeOptions {
                preserve_alpha: true,
                mask_path: Some(mask.clone()),
                ..DecodeOptions::default()
            },
        )
        .unwrap();
        let missed_red = missed
            .pixels()
            .filter(|p| p[3] > 200 && p[0] > 150)
            .count();
        assert!(
            missed_red < 20,
            "Cover of the full frame must miss an off-center person"
        );

        let cropped = decode_card_rgba(
            &photo,
            8,
            16,
            &DecodeOptions {
                preserve_alpha: true,
                mask_path: Some(mask),
                source_crop: Some([48, 0, 16, 32]),
                ..DecodeOptions::default()
            },
        )
        .unwrap();
        let kept_red = cropped
            .pixels()
            .filter(|p| p[3] > 200 && p[0] > 150)
            .count();
        assert!(
            kept_red >= 80,
            "cropping to the subject bbox must fill the cutout dest with the person, got {kept_red}"
        );
    }
}
