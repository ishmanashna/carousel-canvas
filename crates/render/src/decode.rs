use std::fs::File;
use std::io::Read;
use std::path::Path;

use image::metadata::Orientation;
use image::{DynamicImage, GenericImageView, ImageDecoder, ImageReader};
use jpeg_decoder::{Decoder, PixelFormat};

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

fn load_rgb_at_most(meta: &SourceMeta, path: &Path, need_max_dim: u32) -> Result<DynamicImage> {
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
    Ok(flatten_alpha(apply_orientation(img, meta.orientation)))
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
    let need_max = estimate_max_intermediate_dim(
        src_w,
        src_h,
        dest_w,
        dest_h,
        options.source_trim_left_frac,
        options.horizontal_center_band_frac,
        options.cover_height_first,
        options.fit,
    );
    let loaded = load_rgb_at_most(&meta, path, need_max)?;
    if !orientation_ok(&loaded, options) {
        return Err(DecodeError::OrientationMismatch);
    }
    let fitted = fit_to_destination(loaded, dest_w, dest_h, options);
    let (out_w, out_h) = fitted.dimensions();
    tracing::info!(
        path = %path.display(),
        dest_w,
        dest_h,
        decoded_w = out_w,
        decoded_h = out_h,
        need_max,
        "decode_card_rgba"
    );
    Ok(rgb_to_rgba(fitted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

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
}
