use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};
use crate::python_rng::PythonRandom;
use crate::slot::StripSlotDef;
use crate::template::StripTemplate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspectClass {
    Landscape,
    Portrait,
    Square,
}

fn aspect_from_oriented_dims(w: u32, h: u32) -> AspectClass {
    if w > h {
        AspectClass::Landscape
    } else if h > w {
        AspectClass::Portrait
    } else {
        AspectClass::Square
    }
}

fn oriented_dimensions(width: u32, height: u32, orientation: image::metadata::Orientation) -> (u32, u32) {
    use image::metadata::Orientation::*;
    match orientation {
        NoTransforms | Rotate180 | FlipHorizontal | FlipVertical => (width, height),
        Rotate90 | Rotate270 | Rotate90FlipH | Rotate270FlipH => (height, width),
    }
}

/// Oriented pixel size (EXIF), matching Python `get_image_aspect_hint`.
/// Falls back to filename heuristics only when the file cannot be read (tests / missing paths).
pub fn aspect_hint_from_path(path: &Path) -> AspectClass {
    if let Ok(hint) = aspect_hint_from_pixels(path) {
        return hint;
    }
    aspect_hint_from_filename(path)
}

fn aspect_hint_from_pixels(path: &Path) -> std::result::Result<AspectClass, ()> {
    use image::{ImageDecoder, ImageReader};
    let reader = ImageReader::open(path).map_err(|_| ())?;
    let reader = reader.with_guessed_format().map_err(|_| ())?;
    let mut decoder = reader.into_decoder().map_err(|_| ())?;
    let (w, h) = decoder.dimensions();
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let (w, h) = oriented_dimensions(w, h, orientation);
    Ok(aspect_from_oriented_dims(w, h))
}

fn aspect_hint_from_filename(path: &Path) -> AspectClass {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.contains("landscape") || name.contains("_land_") || name.starts_with('l') {
        return AspectClass::Landscape;
    }
    if name.contains("portrait") || name.contains("_port_") || name.starts_with('p') {
        return AspectClass::Portrait;
    }
    AspectClass::Square
}

fn dedupe_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for p in paths {
        let key = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
        if seen.insert(key) {
            out.push(p.clone());
        }
    }
    out
}

pub fn expand_paths_cyclic(paths: &[PathBuf], n: usize, rng: &mut PythonRandom) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        return Err(CoreError::NeedAtLeastOneImage);
    }
    let mut base = paths.to_vec();
    shuffle_paths(&mut base, rng);
    Ok((0..n).map(|i| base[i % base.len()].clone()).collect())
}

fn shuffle_paths(paths: &mut [PathBuf], rng: &mut PythonRandom) {
    for i in (1..paths.len()).rev() {
        let j = rng.randbelow((i + 1) as u32) as usize;
        paths.swap(i, j);
    }
}

pub fn assignment_pool(paths: &[PathBuf], n: usize, rng: &mut PythonRandom) -> Result<Vec<PathBuf>> {
    let mut uniq = dedupe_paths(paths);
    shuffle_paths(&mut uniq, rng);
    if uniq.len() >= n {
        return Ok(uniq.into_iter().take(n).collect());
    }
    expand_paths_cyclic(&uniq, n, rng)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotPreference {
    Portrait,
    Landscape,
    Any,
}

fn slot_preference(slot: &StripSlotDef) -> SlotPreference {
    if slot.prefer_portrait {
        return SlotPreference::Portrait;
    }
    if slot.prefer_landscape {
        return SlotPreference::Landscape;
    }
    let ar = slot.w as f64 / (slot.h.max(1) as f64);
    if ar >= 1.2 {
        SlotPreference::Landscape
    } else if ar <= 0.9 {
        SlotPreference::Portrait
    } else {
        SlotPreference::Any
    }
}

fn match_score(pref: SlotPreference, cls: AspectClass) -> i32 {
    match (pref, cls) {
        (SlotPreference::Any, _) => 0,
        (SlotPreference::Landscape, AspectClass::Landscape) => 2,
        (SlotPreference::Landscape, AspectClass::Square) => 1,
        (SlotPreference::Landscape, AspectClass::Portrait) => 0,
        (SlotPreference::Portrait, AspectClass::Portrait) => 2,
        (SlotPreference::Portrait, AspectClass::Square) => 1,
        (SlotPreference::Portrait, AspectClass::Landscape) => 0,
    }
}

fn fill_order_indices(slots: &[StripSlotDef]) -> Vec<usize> {
    let mut keyed: Vec<(f64, f64, usize)> = Vec::new();
    for (i, s) in slots.iter().enumerate() {
        let pref = slot_preference(s);
        let ar = s.w as f64 / (s.h.max(1) as f64);
        let key = if s.prefer_portrait {
            (-1.0, 0.0)
        } else if s.prefer_landscape {
            (-0.5, i as f64)
        } else {
            match pref {
                SlotPreference::Landscape => (0.0, -ar),
                SlotPreference::Portrait => (1.0, ar),
                SlotPreference::Any => (2.0, 0.0),
            }
        };
        keyed.push((key.0, key.1, i));
    }
    keyed.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    keyed.into_iter().map(|(_, _, i)| i).collect()
}

fn pick_best_match(
    remaining: &[PathBuf],
    classes: &std::collections::HashMap<PathBuf, AspectClass>,
    pref: SlotPreference,
    rng: &mut PythonRandom,
) -> PathBuf {
    let best = remaining
        .iter()
        .map(|p| match_score(pref, classes.get(p).copied().unwrap_or(AspectClass::Square)))
        .max()
        .unwrap_or(0);
    let tier: Vec<&PathBuf> = remaining
        .iter()
        .filter(|p| {
            match_score(pref, classes.get(*p).copied().unwrap_or(AspectClass::Square)) == best
        })
        .collect();
    let idx = rng.randbelow(tier.len() as u32) as usize;
    tier[idx].clone()
}

fn pick_for_slot(
    remaining: &mut Vec<PathBuf>,
    classes: &std::collections::HashMap<PathBuf, AspectClass>,
    slot: &StripSlotDef,
    rng: &mut PythonRandom,
) -> PathBuf {
    let pref = slot_preference(slot);
    if slot.prefer_portrait {
        let portraits: Vec<PathBuf> = remaining
            .iter()
            .filter(|p| classes.get(*p) == Some(&AspectClass::Portrait))
            .cloned()
            .collect();
        if !portraits.is_empty() {
            let idx = rng.randbelow(portraits.len() as u32) as usize;
            let chosen = portraits[idx].clone();
            remaining.retain(|p| p != &chosen);
            return chosen;
        }
    } else if slot.prefer_landscape {
        let landscapes: Vec<PathBuf> = remaining
            .iter()
            .filter(|p| classes.get(*p) == Some(&AspectClass::Landscape))
            .cloned()
            .collect();
        if !landscapes.is_empty() {
            let idx = rng.randbelow(landscapes.len() as u32) as usize;
            let chosen = landscapes[idx].clone();
            remaining.retain(|p| p != &chosen);
            return chosen;
        }
    }
    let chosen = pick_best_match(remaining, classes, pref, rng);
    remaining.retain(|p| p != &chosen);
    chosen
}

pub fn pick_underfill_paths(
    paths: &[PathBuf],
    main_slot_paths: &[Option<PathBuf>],
    n: usize,
    rng: &mut PythonRandom,
) -> Vec<PathBuf> {
    if n == 0 || paths.is_empty() {
        return Vec::new();
    }
    let used: std::collections::HashSet<PathBuf> = main_slot_paths
        .iter()
        .filter_map(|p| p.as_ref())
        .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()))
        .collect();
    let uniq = dedupe_paths(paths);
    let mut avail: Vec<PathBuf> = uniq
        .iter()
        .filter(|p| {
            let key = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
            !used.contains(&key)
        })
        .cloned()
        .collect();
    shuffle_paths(&mut avail, rng);
    if avail.len() >= n {
        return avail.into_iter().take(n).collect();
    }
    let mut out = avail;
    let mut rest: Vec<PathBuf> = uniq
        .iter()
        .filter(|p| {
            let key = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
            used.contains(&key)
        })
        .cloned()
        .collect();
    shuffle_paths(&mut rest, rng);
    for p in rest {
        if out.len() >= n {
            break;
        }
        out.push(p);
    }
    if out.len() >= n {
        return out.into_iter().take(n).collect();
    }
    expand_paths_cyclic(&uniq, n, rng).unwrap_or_default()
}

#[cfg(test)]
mod aspect_tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn aspect_hint_reads_pixel_dims() {
        let dir = tempfile::tempdir().unwrap();
        let land = dir.path().join("IMG_0001.jpg");
        let port = dir.path().join("DSC0002.jpg");
        RgbImage::from_pixel(400, 200, Rgb([10, 20, 30]))
            .save(&land)
            .unwrap();
        RgbImage::from_pixel(200, 400, Rgb([10, 20, 30]))
            .save(&port)
            .unwrap();
        assert_eq!(aspect_hint_from_path(&land), AspectClass::Landscape);
        assert_eq!(aspect_hint_from_path(&port), AspectClass::Portrait);
    }
}

pub fn pick_smart_fills(
    paths: &[PathBuf],
    template: &StripTemplate,
    layout_seed: Option<i64>,
    assign_seed: u32,
) -> Result<Vec<Option<PathBuf>>> {
    if paths.is_empty() {
        return Err(CoreError::NeedAtLeastOneImage);
    }
    let mut rng = PythonRandom::new(assign_seed);
    let req = template.strip_effective_fill_required(layout_seed);
    let need = req.iter().filter(|r| **r).count();
    let slots = template.resolved_slots(layout_seed);
    if slots.len() != req.len() {
        return Err(CoreError::SlotLengthMismatch {
            expected: req.len(),
            got: slots.len(),
        });
    }
    let classes: std::collections::HashMap<PathBuf, AspectClass> = paths
        .iter()
        .map(|p| (p.clone(), aspect_hint_from_path(p)))
        .collect();
    let mut remaining = assignment_pool(paths, need, &mut rng)?;
    let order: Vec<usize> = fill_order_indices(&slots)
        .into_iter()
        .filter(|i| req[*i])
        .collect();
    let mut out: Vec<Option<PathBuf>> = vec![None; req.len()];
    for idx in order {
        let chosen = pick_for_slot(&mut remaining, &classes, &slots[idx], &mut rng);
        out[idx] = Some(chosen);
    }

    if template.id != "strip_seamless_mosaic_v1" {
        let base_req = template.effective_slot_fill_required();
        let mut optional: Vec<usize> = base_req
            .iter()
            .enumerate()
            .filter_map(|(i, &r)| if !r { Some(i) } else { None })
            .collect();
        optional.sort_by_key(|i| slots[*i].z_index);
        for idx in optional {
            if remaining.is_empty() {
                break;
            }
            let chosen = pick_for_slot(&mut remaining, &classes, &slots[idx], &mut rng);
            out[idx] = Some(chosen);
        }
    }
    Ok(out)
}

/// Random slot assignment (no landscape/portrait matching).
pub fn pick_dumb_fills(
    paths: &[PathBuf],
    template: &StripTemplate,
    layout_seed: Option<i64>,
    assign_seed: u32,
) -> Result<Vec<Option<PathBuf>>> {
    if paths.is_empty() {
        return Err(CoreError::NeedAtLeastOneImage);
    }
    let mut rng = PythonRandom::new(assign_seed);
    let req_full = template.strip_effective_fill_required(layout_seed);
    let need = req_full.iter().filter(|r| **r).count();

    if template.id == "strip_seamless_mosaic_v1" {
        let mut pool = expand_paths_cyclic(paths, need, &mut rng)?;
        shuffle_paths(&mut pool, &mut rng);
        return Ok(pool.into_iter().take(need).map(Some).collect());
    }

    let req_base = template.effective_slot_fill_required();
    let mut pool = expand_paths_cyclic(paths, need, &mut rng)?;
    shuffle_paths(&mut pool, &mut rng);
    let mut out: Vec<Option<PathBuf>> = vec![None; req_full.len()];
    let mut pick_i = 0usize;
    for si in 0..template.num_slots() {
        if !req_base[si] {
            continue;
        }
        out[si] = Some(pool[pick_i].clone());
        pick_i += 1;
    }
    let mut optional: Vec<usize> = req_base
        .iter()
        .enumerate()
        .filter_map(|(i, &r)| if !r { Some(i) } else { None })
        .collect();
    let slots = template.resolved_slots(layout_seed);
    optional.sort_by_key(|i| slots[*i].z_index);
    for si in optional {
        if pick_i >= pool.len() {
            break;
        }
        out[si] = Some(pool[pick_i].clone());
        pick_i += 1;
    }
    Ok(out)
}

pub fn validate_strip_unique_sources(
    template: &StripTemplate,
    source_paths: &[PathBuf],
    allow_repeats: bool,
    layout_seed: Option<i64>,
) -> Result<()> {
    let need = template.strip_image_slot_count(layout_seed);
    crate::io::validate_unique_sources(need, source_paths, allow_repeats, template.id)
}
