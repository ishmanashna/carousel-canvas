use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result, count_unique_paths};

pub const IMAGE_EXTENSIONS: &[&str] = &[".jpg", ".jpeg", ".png"];

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let lower = e.to_ascii_lowercase();
            IMAGE_EXTENSIONS.iter().any(|ext| *ext == format!(".{lower}"))
        })
        .unwrap_or(false)
}

/// Scan a folder for jpg/jpeg/png files (sorted).
pub fn scan_image_folder(folder: &Path) -> Result<Vec<PathBuf>> {
    if !folder.is_dir() {
        return Ok(vec![]);
    }
    let mut paths = Vec::new();
    let entries = std::fs::read_dir(folder).map_err(|e| CoreError::Io(e.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|e| CoreError::Io(e.to_string()))?;
        let path = entry.path();
        if path.is_file() && is_image_path(&path) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

pub fn validate_unique_sources(
    need: usize,
    source_paths: &[PathBuf],
    allow_repeats: bool,
    template_id: &str,
) -> Result<()> {
    if allow_repeats {
        return Ok(());
    }
    let have = count_unique_paths(source_paths);
    if have < need {
        return Err(CoreError::NotEnoughUniquePhotos {
            template_id: template_id.to_string(),
            need,
            have,
        });
    }
    Ok(())
}
