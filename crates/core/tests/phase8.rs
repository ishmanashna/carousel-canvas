//! Phase 8: CLI validation and uniqueness rules.

use std::path::PathBuf;

use core::{get_template_by_id, scan_image_folder, validate_strip_unique_sources};

#[test]
fn mural_v2_needs_35_unique_without_repeats() {
    let tpl = get_template_by_id("strip_mural_v2").unwrap();
    let paths: Vec<PathBuf> = (0..8).map(|i| PathBuf::from(format!("/fake/photo{i}.jpg"))).collect();
    let err = validate_strip_unique_sources(&tpl, &paths, false, Some(7)).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("35"), "expected slot count 35 in error: {msg}");
    assert!(msg.contains("strip_mural_v2"), "expected template id in error: {msg}");
}

#[test]
fn strip_10col_accepts_ten_unique() {
    let tpl = get_template_by_id("strip_10col").unwrap();
    let paths: Vec<PathBuf> = (0..10).map(|i| PathBuf::from(format!("/fake/photo{i}.jpg"))).collect();
    validate_strip_unique_sources(&tpl, &paths, false, Some(0)).unwrap();
}

#[test]
fn test_images_folder_has_enough_for_mural() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let test_images = root.join("TEST IMAGES");
    if !test_images.is_dir() {
        return;
    }
    let paths = scan_image_folder(&test_images).unwrap();
    let tpl = get_template_by_id("strip_mural_v2").unwrap();
    let need = tpl.strip_image_slot_count(Some(7));
    assert!(
        paths.len() >= need || paths.is_empty(),
        "TEST IMAGES should have at least {need} files for mural v2 seed 7"
    );
}
