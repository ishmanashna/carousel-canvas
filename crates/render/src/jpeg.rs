use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder, RgbImage};

use crate::error::{DecodeError, Result};

pub const CAROUSEL_SLICE_MAX_BYTES: usize = 8 * 1024 * 1024;

pub fn encode_slice_jpeg(rgb: &RgbImage, max_bytes: usize) -> Result<Vec<u8>> {
    encode_jpeg_with_search(
        rgb,
        max_bytes,
        JpegSearchOptions {
            quality_first: 98,
            quality_min: 86,
            quality_max: 98,
            use_hq_ladder: true,
        },
    )
}

pub fn encode_jpeg_uncapped(rgb: &RgbImage) -> Result<Vec<u8>> {
    encode_jpeg_with_search(
        rgb,
        usize::MAX,
        JpegSearchOptions {
            quality_first: 95,
            quality_min: 65,
            quality_max: 95,
            use_hq_ladder: false,
        },
    )
}

#[derive(Debug, Clone, Copy)]
struct JpegSearchOptions {
    quality_first: u8,
    quality_min: u8,
    quality_max: u8,
    use_hq_ladder: bool,
}

fn encode_jpeg_with_search(
    rgb: &RgbImage,
    max_bytes: usize,
    opts: JpegSearchOptions,
) -> Result<Vec<u8>> {
    let q_uncapped = if opts.use_hq_ladder {
        opts.quality_first.min(opts.quality_max)
    } else {
        opts.quality_first
    };

    let first = encode_jpeg_quality(rgb, q_uncapped)?;
    if first.len() <= max_bytes {
        return Ok(first);
    }

    if opts.use_hq_ladder {
        let mut lo = opts.quality_min;
        let mut hi = q_uncapped.saturating_sub(1);
        let mut best: Option<Vec<u8>> = None;
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let buf = encode_jpeg_quality(rgb, mid)?;
            if buf.len() <= max_bytes {
                best = Some(buf);
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
        return Ok(best.unwrap_or_else(|| encode_jpeg_quality(rgb, opts.quality_min).unwrap()));
    }

    let mut lo = 65u8;
    let mut hi = 90u8;
    let mut best: Option<Vec<u8>> = None;
    while lo <= hi {
        let mid = (lo + hi) / 2;
        let buf = encode_jpeg_quality(rgb, mid)?;
        if buf.len() <= max_bytes {
            best = Some(buf);
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    Ok(best.unwrap_or_else(|| encode_jpeg_quality(rgb, 65).unwrap()))
}

fn encode_jpeg_quality(rgb: &RgbImage, quality: u8) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let encoder = JpegEncoder::new_with_quality(Cursor::new(&mut buf), quality);
    encoder
        .write_image(rgb.as_raw(), rgb.width(), rgb.height(), ExtendedColorType::Rgb8)
        .map_err(|e| DecodeError::Encode(e.to_string()))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn slice_jpeg_respects_8mb_cap() {
        let img = RgbImage::from_fn(1080, 1350, |x, y| {
            Rgb([
                ((x * 17) % 256) as u8,
                ((y * 31) % 256) as u8,
                (((x + y) * 7) % 256) as u8,
            ])
        });
        let bytes = encode_slice_jpeg(&img, CAROUSEL_SLICE_MAX_BYTES).unwrap();
        assert!(bytes.len() <= CAROUSEL_SLICE_MAX_BYTES);
    }
}
