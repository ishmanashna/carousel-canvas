//! Pin a specific file to the strip template flagship (hero) slot.

use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};
use crate::io::IMAGE_EXTENSIONS;
use crate::python_rng::PythonRandom;
use crate::template::{LayoutPlacer, StripTemplate};

/// Resolve `spec` to an image path: existing file, file under `folder`, or unique
/// substring match among top-level images in `folder`.
pub fn resolve_strip_hero_argument(folder: &Path, spec: &str) -> Result<PathBuf> {
    let s = spec.trim();
    if s.is_empty() {
        return Err(CoreError::InvalidInput(
            "Hero image argument is empty.".into(),
        ));
    }

    let p = PathBuf::from(s);
    if p.is_file() && is_image_ext(&p) {
        return Ok(std::fs::canonicalize(&p).unwrap_or(p));
    }

    let rel = folder.join(s);
    if rel.is_file() && is_image_ext(&rel) {
        return Ok(std::fs::canonicalize(&rel).unwrap_or(rel));
    }

    let frag = s.to_ascii_lowercase();
    let mut matches: Vec<PathBuf> = Vec::new();
    let entries = std::fs::read_dir(folder).map_err(|e| {
        CoreError::InvalidInput(format!("Cannot read folder {}: {e}", folder.display()))
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| CoreError::Io(e.to_string()))?;
        let path = entry.path();
        if path.is_file()
            && is_image_ext(&path)
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_ascii_lowercase().contains(&frag))
                .unwrap_or(false)
        {
            matches.push(path);
        }
    }
    matches.sort_by_key(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
    });

    if matches.is_empty() {
        return Err(CoreError::InvalidInput(format!(
            "No image in {} matches {s:?} (tried path, {}, and filename substring).",
            folder.display(),
            rel.display()
        )));
    }
    if matches.len() > 1 {
        let preview: Vec<String> = matches
            .iter()
            .take(8)
            .filter_map(|m| m.file_name().and_then(|n| n.to_str().map(str::to_string)))
            .collect();
        let more = if matches.len() > 8 {
            format!(" (+{} more)", matches.len() - 8)
        } else {
            String::new()
        };
        return Err(CoreError::InvalidInput(format!(
            "Hero spec {s:?} is ambiguous ({} files): {}{}. \
             Use a longer substring or a full file path.",
            matches.len(),
            preview.join(", "),
            more
        )));
    }
    Ok(std::fs::canonicalize(&matches[0]).unwrap_or_else(|_| matches[0].clone()))
}

fn is_image_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let lower = format!(".{}", e.to_ascii_lowercase());
            IMAGE_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

/// Set flagship slot to `hero_path`; optionally dedupe other slots (when repeats disallowed).
pub fn pin_strip_hero_fill(
    fills: &mut [Option<PathBuf>],
    template: &StripTemplate,
    hero_path: &Path,
    layout_seed: Option<i64>,
    pool_paths: &[PathBuf],
    allow_repeats: bool,
    rng: &mut PythonRandom,
) -> Result<()> {
    pin_strip_hero_fill_at(fills, template, None, hero_path, layout_seed, pool_paths, allow_repeats, rng)
}

/// Pin hero to flagship slot; `flagship_slot_index` overrides template default (snapshot lock).
pub fn pin_strip_hero_fill_at(
    fills: &mut [Option<PathBuf>],
    template: &StripTemplate,
    flagship_slot_index: Option<usize>,
    hero_path: &Path,
    layout_seed: Option<i64>,
    pool_paths: &[PathBuf],
    allow_repeats: bool,
    rng: &mut PythonRandom,
) -> Result<()> {
    let hi = flagship_slot_index.or(template.layout_flagship_slot_index).ok_or_else(|| {
        CoreError::InvalidInput(format!(
            "Template {:?} has no flagship (hero) slot -- omit --strip-hero-image.",
            template.id
        ))
    })?;

    let req = template.strip_effective_fill_required(layout_seed);
    let req = if template.layout_placer == LayoutPlacer::OutOfFrame
        || (req.is_empty() && !fills.is_empty())
    {
        vec![true; fills.len()]
    } else {
        req
    };
    if hi >= fills.len() || hi >= req.len() {
        return Err(CoreError::InvalidInput(format!(
            "Invalid flagship slot index {hi} for current fill list."
        )));
    }
    if !req[hi] {
        return Err(CoreError::InvalidInput(format!(
            "Flagship slot {hi} is not an image-required slot for this template/seed."
        )));
    }

    let hero_r = std::fs::canonicalize(hero_path).unwrap_or_else(|_| hero_path.to_path_buf());
    fills[hi] = Some(hero_path.to_path_buf());

    if allow_repeats {
        return Ok(());
    }

    loop {
        let mut dup_j: Option<usize> = None;
        for (j, f) in fills.iter().enumerate() {
            if j == hi || j >= req.len() || !req[j] {
                continue;
            }
            let Some(fp) = f else { continue };
            let key = std::fs::canonicalize(fp).unwrap_or_else(|_| fp.clone());
            if key == hero_r {
                dup_j = Some(j);
                break;
            }
        }
        let Some(dup_j) = dup_j else { break };

        let mut others = std::collections::HashSet::new();
        for (k, fk) in fills.iter().enumerate() {
            if k == dup_j || k >= req.len() || !req[k] {
                continue;
            }
            if let Some(p) = fk {
                others.insert(std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()));
            }
        }
        let spare: Vec<PathBuf> = pool_paths
            .iter()
            .filter(|p| {
                let key = std::fs::canonicalize(p).unwrap_or_else(|_| (*p).clone());
                !others.contains(&key)
            })
            .cloned()
            .collect();
        if spare.is_empty() {
            break;
        }
        let idx = rng.randbelow(spare.len() as u32) as usize;
        fills[dup_j] = Some(spare[idx].clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_dummy_jpg(dir: &Path, name: &str) {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        // Minimal JPEG header bytes (not a valid image, but passes extension check).
        f.write_all(&[0xFF, 0xD8, 0xFF, 0xD9]).unwrap();
    }

    #[test]
    fn substring_unique() {
        let dir = tempfile::tempdir().unwrap();
        write_dummy_jpg(dir.path(), "MIII4789-Enhanced-NR.jpg");
        write_dummy_jpg(dir.path(), "other.jpg");
        let p = resolve_strip_hero_argument(dir.path(), "MIII4789-Enhanced-NR").unwrap();
        assert_eq!(p.file_name().unwrap(), "MIII4789-Enhanced-NR.jpg");
    }

    #[test]
    fn ambiguous_raises() {
        let dir = tempfile::tempdir().unwrap();
        write_dummy_jpg(dir.path(), "foo-MIII-x.jpg");
        write_dummy_jpg(dir.path(), "bar-MIII-y.jpg");
        let err = resolve_strip_hero_argument(dir.path(), "MIII").unwrap_err();
        assert!(err.to_string().to_lowercase().contains("ambiguous"));
    }

    #[test]
    fn pins_flagship_slot_mural_v2() {
        let tpl = crate::get_template_by_id("strip_mural_v2").unwrap();
        let hi = tpl.layout_flagship_slot_index.unwrap();
        let req = tpl.strip_effective_fill_required(Some(0));
        let n = req.len();
        let mut fills: Vec<Option<PathBuf>> = (0..n).map(|i| Some(PathBuf::from(format!("/tmp/fake{i}.jpg")))).collect();
        let hero = PathBuf::from("/tmp/HERO_ONLY.jpg");
        let pool: Vec<PathBuf> = (0..n).map(|i| PathBuf::from(format!("/tmp/fake{i}.jpg"))).collect();
        let mut rng = PythonRandom::new(1);
        pin_strip_hero_fill(
            &mut fills,
            &tpl,
            &hero,
            Some(0),
            &pool,
            true,
            &mut rng,
        )
        .unwrap();
        assert_eq!(fills[hi].as_ref().unwrap(), &hero);
    }

    #[test]
    fn pins_flagship_slot_out_of_frame() {
        let tpl = crate::get_template_by_id("strip_out_of_frame_v1").unwrap();
        assert!(tpl.strip_effective_fill_required(Some(0)).is_empty());
        let mut fills: Vec<Option<PathBuf>> = (0..4)
            .map(|i| Some(PathBuf::from(format!("/tmp/oof{i}.jpg"))))
            .collect();
        let hero = PathBuf::from("/tmp/HERO_OOF.jpg");
        let pool: Vec<PathBuf> = (0..4)
            .map(|i| PathBuf::from(format!("/tmp/oof{i}.jpg")))
            .collect();
        let mut rng = PythonRandom::new(1);
        pin_strip_hero_fill_at(
            &mut fills,
            &tpl,
            Some(2),
            &hero,
            Some(0),
            &pool,
            true,
            &mut rng,
        )
        .unwrap();
        assert_eq!(fills[2].as_ref().unwrap(), &hero);
    }
}
