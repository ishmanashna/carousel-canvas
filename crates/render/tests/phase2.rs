use std::path::PathBuf;
use std::time::Instant;

use core::{build_scene, get_template_by_id, pick_smart_fills, scan_image_folder};
use render::{decode_card_rgba, decode_scene_cards, DecodeCache, DecodeOptions};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf()
}

#[test]
fn decode_real_test_image_to_200px_if_present() {
    let test_images = repo_root().join("TEST IMAGES");
    if !test_images.is_dir() {
        eprintln!("skip decode_real_test_image_to_200px_if_present: no TEST IMAGES");
        return;
    }
    let paths = scan_image_folder(&test_images).expect("scan");
    let Some(path) = paths.into_iter().next() else {
        eprintln!("skip decode_real_test_image_to_200px_if_present: empty folder");
        return;
    };
    let out = decode_card_rgba(&path, 200, 200, &DecodeOptions::default()).expect("decode");
    assert_eq!(out.dimensions(), (200, 200));
}

#[test]
fn mural_v2_scene_decode_bench_if_test_images_present() {
    let test_images = repo_root().join("TEST IMAGES");
    if !test_images.is_dir() {
        eprintln!("skip mural_v2_scene_decode_bench_if_test_images_present");
        return;
    }
    let paths = scan_image_folder(&test_images).expect("scan");
    if paths.len() < 35 {
        eprintln!("skip bench: need at least 35 images, have {}", paths.len());
        return;
    }
    let tpl = get_template_by_id("strip_mural_v2").unwrap();
    let fills = pick_smart_fills(&paths, &tpl, Some(7), 42).unwrap();
    let scene = build_scene(&tpl, &fills, Some(7));
    assert_eq!(scene.cards.len(), 35);

    let cache = DecodeCache::new();
    let t0 = Instant::now();
    let decoded = decode_scene_cards(&scene, &cache).unwrap();
    let elapsed = t0.elapsed();
    assert_eq!(decoded.len(), 35);
    eprintln!(
        "mural v2 decode bench: {:.3}s for {} cards",
        elapsed.as_secs_f64(),
        decoded.len()
    );
}
