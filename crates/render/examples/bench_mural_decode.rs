//! Bench: decode all mural v2 slot textures from TEST IMAGES.

use std::path::PathBuf;
use std::time::Instant;

use core::{build_scene, get_template_by_id, pick_smart_fills, scan_image_folder};
use render::{decode_scene_cards, DecodeCache, render_version};

fn main() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf();
    let test_images = repo_root.join("TEST IMAGES");
    if !test_images.is_dir() {
        eprintln!("TEST IMAGES folder not found at {}", test_images.display());
        std::process::exit(1);
    }

    let paths = scan_image_folder(&test_images).expect("scan");
    if paths.is_empty() {
        eprintln!("no images in TEST IMAGES");
        std::process::exit(1);
    }

    let tpl = get_template_by_id("strip_mural_v2").expect("template");
    let fills = pick_smart_fills(&paths, &tpl, Some(7), 42).expect("assign");
    let scene = build_scene(&tpl, &fills, Some(7));
    assert_eq!(scene.cards.len(), 35);

    println!("render {}", render_version());
    println!("decoding {} mural v2 cards from {}", scene.cards.len(), test_images.display());

    let cache = DecodeCache::new();
    let t0 = Instant::now();
    let decoded = decode_scene_cards(&scene, &cache).expect("decode cold");
    let cold = t0.elapsed();
    let t1 = Instant::now();
    let _warm = decode_scene_cards(&scene, &cache).expect("decode warm");
    let warm = t1.elapsed();
    let pixels: u64 = decoded
        .iter()
        .map(|c| {
            let (w, h) = c.image.dimensions();
            u64::from(w) * u64::from(h)
        })
        .sum();

    println!(
        "decoded {} cards in {:.3}s cold ({:.1} ms/card), {:.3}s warm, cache entries {}",
        decoded.len(),
        cold.as_secs_f64(),
        cold.as_secs_f64() * 1000.0 / decoded.len() as f64,
        warm.as_secs_f64(),
        cache.len()
    );
    println!("total output pixels: {pixels}");
}
