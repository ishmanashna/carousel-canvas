//! Photo role analysis from segmentation masks (ONNX-free).

use std::path::PathBuf;

use image::{GrayImage, ImageBuffer, Luma};

/// Downsampled occupancy from a mask (~80 px on the long side).
#[derive(Debug, Clone, PartialEq)]
pub struct OccupancyMap {
    pub width: u32,
    pub height: u32,
    pub occupied: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhotoRole {
    Paper,
    Figure,
    Skip,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PhotoAnalysis {
    pub path: PathBuf,
    pub role: PhotoRole,
    pub occupancy: OccupancyMap,
    pub subject_bbox: [i32; 4],
    pub mask_png: PathBuf,
    pub complete_subject: bool,
}

const MASK_THRESHOLD: u8 = 128;
/// Stamp this many bbox-heights of occupancy below the subject so a face-only
/// mask still blocks the torso (figures must not sit on chests).
const OCCUPANCY_BODY_HEIGHT_MULT: f64 = 2.5;
const DILATE_RADIUS: i32 = 2;
const FIGURE_AREA_MIN_FRAC: f64 = 0.06;
const FIGURE_AREA_MAX_FRAC: f64 = 0.85;
/// Fragment starting too far down (horn-only, arm-only). A whole person holding an
/// instrument still starts near the top of the frame.
const FIGURE_MAX_MIN_Y_FRAC: f64 = 0.30;
/// Too short to be a whole person (or person+instrument). Fragments fail this.
const FIGURE_MIN_BBOX_HEIGHT_FRAC: f64 = 0.38;
/// Soft mask below this is treated as background (kills ISNet/u2net fog).
const CUTOUT_ALPHA_FLOOR: u8 = 150;
const CUTOUT_ALPHA_FULL: u8 = 210;
/// Second blob at or above this fraction of image area counts as "two large blobs".
const LARGE_BLOB_MIN_FRAC: f64 = 0.08;
/// Second blob above this fraction of the largest blob breaks single-dominance.
const SECOND_BLOB_DOMINANCE_FRAC: f64 = 0.25;
const PANORAMIC_ASPECT_RATIO: f64 = 3.5;

struct Blob {
    area: usize,
    min_x: u32,
    min_y: u32,
    max_x: u32,
    max_y: u32,
    seed: usize,
}

fn threshold_mask(mask: &GrayImage) -> Vec<bool> {
    mask.pixels()
        .map(|p| p.0[0] >= MASK_THRESHOLD)
        .collect()
}

fn dilate_binary(fg: &[bool], width: u32, height: u32, radius: i32) -> Vec<bool> {
    let w = width as i32;
    let h = height as i32;
    let mut out = vec![false; fg.len()];
    for y in 0..h {
        for x in 0..w {
            if !fg[(y * w + x) as usize] {
                continue;
            }
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let nx = x + dx;
                    let ny = y + dy;
                    if nx >= 0 && ny >= 0 && nx < w && ny < h {
                        out[(ny * w + nx) as usize] = true;
                    }
                }
            }
        }
    }
    out
}

fn find_blobs(fg: &[bool], width: u32, height: u32) -> Vec<Blob> {
    let w = width as usize;
    let h = height as usize;
    let mut visited = vec![false; fg.len()];
    let mut blobs = Vec::new();

    for start_y in 0..h {
        for start_x in 0..w {
            let start = start_y * w + start_x;
            if !fg[start] || visited[start] {
                continue;
            }
            let mut stack = vec![start];
            let mut area = 0usize;
            let mut min_x = width;
            let mut min_y = height;
            let mut max_x = 0u32;
            let mut max_y = 0u32;
            while let Some(idx) = stack.pop() {
                if visited[idx] {
                    continue;
                }
                visited[idx] = true;
                area += 1;
                let x = (idx % w) as u32;
                let y = (idx / w) as u32;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
                if x > 0 {
                    let ni = idx - 1;
                    if fg[ni] && !visited[ni] {
                        stack.push(ni);
                    }
                }
                if x + 1 < width {
                    let ni = idx + 1;
                    if fg[ni] && !visited[ni] {
                        stack.push(ni);
                    }
                }
                if y > 0 {
                    let ni = idx - w;
                    if fg[ni] && !visited[ni] {
                        stack.push(ni);
                    }
                }
                if y + 1 < height {
                    let ni = idx + w;
                    if fg[ni] && !visited[ni] {
                        stack.push(ni);
                    }
                }
            }
            blobs.push(Blob {
                area,
                min_x,
                min_y,
                max_x,
                max_y,
                seed: start,
            });
        }
    }
    blobs.sort_by_key(|b| b.area);
    blobs.reverse();
    blobs
}

fn processed_blobs(mask: &GrayImage) -> Vec<Blob> {
    let (w, h) = mask.dimensions();
    let fg = threshold_mask(mask);
    let dilated = dilate_binary(&fg, w, h, DILATE_RADIUS);
    find_blobs(&dilated, w, h)
}

fn component_from_seed(fg: &[bool], width: u32, height: u32, seed: usize) -> Vec<bool> {
    let w = width as usize;
    let mut keep = vec![false; fg.len()];
    if seed >= fg.len() || !fg[seed] {
        return keep;
    }
    let mut stack = vec![seed];
    while let Some(idx) = stack.pop() {
        if keep[idx] {
            continue;
        }
        keep[idx] = true;
        let x = (idx % w) as u32;
        let y = (idx / w) as u32;
        if x > 0 {
            let ni = idx - 1;
            if fg[ni] && !keep[ni] {
                stack.push(ni);
            }
        }
        if x + 1 < width {
            let ni = idx + 1;
            if fg[ni] && !keep[ni] {
                stack.push(ni);
            }
        }
        if y > 0 {
            let ni = idx - w;
            if fg[ni] && !keep[ni] {
                stack.push(ni);
            }
        }
        if y + 1 < height {
            let ni = idx + w;
            if fg[ni] && !keep[ni] {
                stack.push(ni);
            }
        }
    }
    keep
}

fn touches_left(blob: &Blob) -> bool {
    blob.min_x == 0
}

fn touches_right(blob: &Blob, width: u32) -> bool {
    blob.max_x + 1 >= width
}

fn is_side_or_top_fragment(blob: &Blob, width: u32) -> bool {
    blob.min_y == 0 || touches_left(blob) || touches_right(blob, width)
}

/// Fill-frame portrait whose hair already hits the top of the file is still a whole person.
fn top_touch_is_chopped_fragment(blob: &Blob, height: u32) -> bool {
    if blob.min_y != 0 {
        return false;
    }
    let bh = blob.max_y - blob.min_y + 1;
    let bw = blob.max_x - blob.min_x + 1;
    let tall_frac = bh as f64 / height.max(1) as f64;
    if tall_frac >= 0.70 && bh > bw {
        return false;
    }
    true
}

/// Largest dilated blob is a single person. Extra people clipped at the frame edge do not count.
/// A tight portrait that already meets the top of the photo is allowed (person + instrument).
pub fn complete_subject_from_mask(mask: &GrayImage) -> bool {
    let (width, height) = mask.dimensions();
    let blobs = processed_blobs(mask);
    if blobs.is_empty() {
        return false;
    }
    let largest = &blobs[0];
    if blobs.len() > 1 {
        let second = &blobs[1];
        let second_frac = second.area as f64 / largest.area as f64;
        if second_frac >= SECOND_BLOB_DOMINANCE_FRAC && !is_side_or_top_fragment(second, width) {
            return false;
        }
    }
    if top_touch_is_chopped_fragment(largest, height) {
        return false;
    }
    if touches_left(largest) && touches_right(largest, width) {
        return false;
    }
    true
}

/// Keep only the largest foreground component so leftover hands/ghosts are not cut out.
pub fn isolate_largest_blob(mask: &GrayImage) -> GrayImage {
    let (width, height) = mask.dimensions();
    let fg = threshold_mask(mask);
    let dilated = dilate_binary(&fg, width, height, DILATE_RADIUS);
    let blobs = find_blobs(&dilated, width, height);
    if blobs.is_empty() {
        return ImageBuffer::from_pixel(width, height, Luma([0]));
    }
    let keep = component_from_seed(&dilated, width, height, blobs[0].seed);
    let mut out = mask.clone();
    for (i, pixel) in out.pixels_mut().enumerate() {
        if !keep[i] {
            pixel.0[0] = 0;
        }
    }
    out
}

/// Map a raw saliency value to cutout alpha. Mid-gray fog becomes fully transparent.
pub fn harden_cutout_alpha(mask_value: u8) -> u8 {
    if mask_value < CUTOUT_ALPHA_FLOOR {
        0
    } else if mask_value >= CUTOUT_ALPHA_FULL {
        255
    } else {
        let span = (CUTOUT_ALPHA_FULL - CUTOUT_ALPHA_FLOOR) as u16;
        let t = (mask_value - CUTOUT_ALPHA_FLOOR) as u16;
        ((t * 255) / span) as u8
    }
}

fn count_large_blobs(blobs: &[Blob], image_area: usize, width: u32) -> usize {
    let min_area = (image_area as f64 * LARGE_BLOB_MIN_FRAC).ceil() as usize;
    blobs
        .iter()
        .filter(|b| b.area >= min_area && !is_side_or_top_fragment(b, width))
        .count()
}

/// Classify role from a mask. `Skip` is reserved for decode failures elsewhere.
pub fn role_from_mask(mask: &GrayImage) -> PhotoRole {
    let (width, height) = mask.dimensions();
    let image_area = (width * height) as usize;
    let blobs = processed_blobs(mask);
    if blobs.is_empty() {
        return PhotoRole::Paper;
    }
    let largest = &blobs[0];
    let area_frac = largest.area as f64 / image_area as f64;

    if count_large_blobs(&blobs, image_area, width) >= 2 {
        return PhotoRole::Paper;
    }
    if area_frac < FIGURE_AREA_MIN_FRAC || area_frac > FIGURE_AREA_MAX_FRAC {
        return PhotoRole::Paper;
    }
    if !complete_subject_from_mask(mask) {
        return PhotoRole::Paper;
    }

    let min_y_frac = largest.min_y as f64 / height.max(1) as f64;
    if min_y_frac > FIGURE_MAX_MIN_Y_FRAC {
        return PhotoRole::Paper;
    }
    let bw = (largest.max_x - largest.min_x + 1) as f64;
    let bh = (largest.max_y - largest.min_y + 1).max(1) as f64;
    if bh / (height.max(1) as f64) < FIGURE_MIN_BBOX_HEIGHT_FRAC {
        return PhotoRole::Paper;
    }
    if bw / bh >= PANORAMIC_ASPECT_RATIO {
        return PhotoRole::Paper;
    }

    PhotoRole::Figure
}

/// Build a coarse occupancy map from a mask (long side ≈ 80 px).
pub fn occupancy_from_mask(mask: &GrayImage) -> OccupancyMap {
    let (sw, sh) = mask.dimensions();
    let long = sw.max(sh).max(1);
    let scale = 80.0 / long as f64;
    let dw = ((sw as f64 * scale).round() as u32).max(1);
    let dh = ((sh as f64 * scale).round() as u32).max(1);
    let mut occupied = vec![0u8; (dw * dh) as usize];
    for y in 0..dh {
        for x in 0..dw {
            let sx = ((x as f64 / dw as f64) * sw as f64) as u32;
            let sy = ((y as f64 / dh as f64) * sh as f64) as u32;
            let sx = sx.min(sw - 1);
            let sy = sy.min(sh - 1);
            occupied[(y * dw + x) as usize] = mask.get_pixel(sx, sy).0[0];
        }
    }
    stamp_subject_occupancy(&mut occupied, dw, dh, sw, sh, subject_bbox_from_mask(mask));
    OccupancyMap {
        width: dw,
        height: dh,
        occupied,
    }
}

fn stamp_subject_occupancy(
    occupied: &mut [u8],
    dw: u32,
    dh: u32,
    sw: u32,
    sh: u32,
    bbox: [i32; 4],
) {
    let [x, y, w, h] = bbox;
    if w <= 0 || h <= 0 {
        return;
    }
    let body_h = ((h as f64) * OCCUPANCY_BODY_HEIGHT_MULT).round() as i32;
    let max_h = (sh as i32 - y.max(0)).max(h);
    let fill_h = body_h.max(h).min(max_h);
    fill_occupancy_rect(occupied, dw, dh, sw, sh, x, y, w, fill_h);
}

fn fill_occupancy_rect(
    occupied: &mut [u8],
    dw: u32,
    dh: u32,
    sw: u32,
    sh: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) {
    if w <= 0 || h <= 0 || dw == 0 || dh == 0 {
        return;
    }
    let sw = sw.max(1) as f64;
    let sh = sh.max(1) as f64;
    let x0 = ((x.max(0) as f64 / sw) * dw as f64).floor() as i32;
    let y0 = ((y.max(0) as f64 / sh) * dh as f64).floor() as i32;
    let x1 = ((((x + w).max(0) as f64 / sw) * dw as f64).ceil() as i32).min(dw as i32);
    let y1 = ((((y + h).max(0) as f64 / sh) * dh as f64).ceil() as i32).min(dh as i32);
    for oy in y0.max(0)..y1.max(0) {
        for ox in x0.max(0)..x1.max(0) {
            let idx = (oy as u32 * dw + ox as u32) as usize;
            if idx < occupied.len() {
                occupied[idx] = occupied[idx].max(255);
            }
        }
    }
}

/// Pad a subject bbox in oriented source pixels so cutout Cover keeps a little context.
pub fn padded_subject_crop(bbox: [i32; 4]) -> Option<[i32; 4]> {
    let [x, y, w, h] = bbox;
    if w <= 0 || h <= 0 {
        return None;
    }
    let px = ((w as f64) * 0.08).ceil() as i32;
    let py = ((h as f64) * 0.08).ceil() as i32;
    Some([x - px, y - py, w + 2 * px, h + 2 * py])
}

/// Bounding box `[x, y, w, h]` of foreground pixels (threshold 128).
pub fn subject_bbox_from_mask(mask: &GrayImage) -> [i32; 4] {
    let (width, height) = mask.dimensions();
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any = false;
    for y in 0..height {
        for x in 0..width {
            if mask.get_pixel(x, y).0[0] >= MASK_THRESHOLD {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if !any {
        return [0, 0, 0, 0];
    }
    [
        min_x as i32,
        min_y as i32,
        (max_x - min_x + 1) as i32,
        (max_y - min_y + 1) as i32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Luma};

    fn solid_rect(w: u32, h: u32, x0: u32, y0: u32, rw: u32, rh: u32, val: u8) -> GrayImage {
        let mut img = ImageBuffer::from_pixel(w, h, Luma([0u8]));
        for y in y0..y0 + rh {
            for x in x0..x0 + rw {
                if x < w && y < h {
                    img.put_pixel(x, y, Luma([val]));
                }
            }
        }
        img
    }

    #[test]
    fn center_blob_is_figure() {
        let mask = solid_rect(100, 100, 35, 25, 30, 50, 200);
        assert!(complete_subject_from_mask(&mask));
        assert_eq!(role_from_mask(&mask), PhotoRole::Figure);
    }

    #[test]
    fn top_touch_is_not_figure() {
        let mask = solid_rect(100, 100, 35, 0, 30, 50, 200);
        assert!(!complete_subject_from_mask(&mask));
        assert_eq!(role_from_mask(&mask), PhotoRole::Paper);
    }

    #[test]
    fn feet_cropped_person_can_be_figure() {
        // Head near the top, blob reaches the bottom (feet cropped). Not a mid-frame instrument.
        let mask = solid_rect(100, 100, 38, 8, 24, 92, 200);
        assert!(complete_subject_from_mask(&mask));
        let blobs = processed_blobs(&mask);
        assert!(blobs[0].max_y + 1 >= 100);
        assert_eq!(role_from_mask(&mask), PhotoRole::Figure);
    }

    #[test]
    fn fill_frame_portrait_touching_top_is_figure() {
        // Hair already at the top of the file; person (+ instrument) is still whole.
        let mask = solid_rect(100, 100, 32, 0, 36, 94, 200);
        assert_eq!(role_from_mask(&mask), PhotoRole::Figure);
    }

    #[test]
    fn instrument_blob_low_in_frame_is_paper() {
        // Instrument or limb alone, no person. Person+instrument together is a figure.
        let mask = solid_rect(100, 100, 20, 55, 55, 30, 200);
        assert_eq!(role_from_mask(&mask), PhotoRole::Paper);
    }

    #[test]
    fn person_holding_instrument_is_figure() {
        // Body from near the top plus a connected horn to the side — one subject.
        let mut mask = solid_rect(100, 100, 40, 8, 22, 85, 200);
        for y in 38..62 {
            for x in 22..70 {
                mask.put_pixel(x, y, Luma([200]));
            }
        }
        assert_eq!(role_from_mask(&mask), PhotoRole::Figure);
    }

    #[test]
    fn isolate_keeps_largest_blob_only() {
        let mut mask = solid_rect(80, 80, 10, 10, 40, 50, 200);
        for y in 5..15 {
            for x in 60..70 {
                mask.put_pixel(x, y, Luma([200]));
            }
        }
        let isolated = isolate_largest_blob(&mask);
        assert!(isolated.get_pixel(30, 30)[0] >= 200);
        assert_eq!(isolated.get_pixel(65, 10)[0], 0);
    }

    #[test]
    fn harden_cutout_kills_fog_keeps_core() {
        assert_eq!(harden_cutout_alpha(0), 0);
        assert_eq!(harden_cutout_alpha(80), 0);
        assert_eq!(harden_cutout_alpha(140), 0);
        assert_eq!(harden_cutout_alpha(255), 255);
        assert!(harden_cutout_alpha(210) >= 200);
    }

    fn occupancy_at(occ: &OccupancyMap, src_x: u32, src_y: u32, src_w: u32, src_h: u32) -> u8 {
        let ox = ((src_x as f64 / src_w as f64) * occ.width as f64) as u32;
        let oy = ((src_y as f64 / src_h as f64) * occ.height as f64) as u32;
        let ox = ox.min(occ.width - 1);
        let oy = oy.min(occ.height - 1);
        occ.occupied[(oy * occ.width + ox) as usize]
    }

    #[test]
    fn occupancy_fills_subject_bbox_and_body_below_head() {
        // Face-only mask: chest is empty in the mask but still a person.
        let mask = solid_rect(100, 100, 40, 10, 20, 20, 200);
        let occ = occupancy_from_mask(&mask);
        assert!(
            occupancy_at(&occ, 50, 18, 100, 100) > 128,
            "head bbox must be occupied"
        );
        assert!(
            occupancy_at(&occ, 50, 45, 100, 100) > 128,
            "torso under a face crop must be occupied so figures do not sit on chests"
        );
        assert!(
            occupancy_at(&occ, 8, 50, 100, 100) < 40,
            "empty stage beside the person must stay free"
        );
    }

    #[test]
    fn two_blobs_are_paper() {
        let mut mask = solid_rect(100, 100, 10, 10, 30, 30, 200);
        for y in 55..85 {
            for x in 60..90 {
                mask.put_pixel(x, y, Luma([200u8]));
            }
        }
        assert_eq!(role_from_mask(&mask), PhotoRole::Paper);
    }
}
