//! Visual dump: source thumb, mask, cutout on magenta. Not a unit test.
//!
//! cargo run -p vision --release --example dump_subjects

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use core::{harden_cutout_alpha, isolate_largest_blob};
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba};
use vision::analyze_folder;

fn checker_cutout(rgba: &image::RgbaImage) -> image::RgbaImage {
    let (w, h) = rgba.dimensions();
    let mut out = ImageBuffer::from_fn(w, h, |x, y| {
        let c = if ((x / 16) + (y / 16)) % 2 == 0 {
            [255u8, 0, 255, 255]
        } else {
            [40u8, 40, 40, 255]
        };
        Rgba(c)
    });
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = p[3] as f32 / 255.0;
        if a <= 0.0 {
            continue;
        }
        let d = out.get_pixel_mut(x, y);
        for i in 0..3 {
            d[i] = ((p[i] as f32 * a) + (d[i] as f32 * (1.0 - a))) as u8;
        }
        d[3] = 255;
    }
    out
}

fn thumb(img: DynamicImage, long: u32) -> DynamicImage {
    let (w, h) = img.dimensions();
    let scale = long as f32 / w.max(h).max(1) as f32;
    let nw = ((w as f32 * scale).round() as u32).max(1);
    let nh = ((h as f32 * scale).round() as u32).max(1);
    img.resize(nw, nh, image::imageops::FilterType::Triangle)
}

fn main() {
    let root = PathBuf::from("TEST IMAGES");
    let out = PathBuf::from("output/subject_debug");
    fs::create_dir_all(&out).unwrap();

    let names = [
        "MIII1129-Enhanced-NR-Edit.jpg",
        "MIII1150-Enhanced-NR-Edit.jpg",
        "MIII1183-Enhanced-NR-Edit.jpg",
        "MIII1213-Enhanced-NR-Edit.jpg",
        "MIII1253-Enhanced-NR-Edit.jpg",
        "MIII1282-Enhanced-NR-Edit.jpg",
        "MIII1320-Enhanced-NR-Edit.jpg",
        "MIII1367-Enhanced-NR-Edit.jpg",
    ];
    let paths: Vec<PathBuf> = names.iter().map(|n| root.join(n)).collect();
    let cancel = AtomicBool::new(false);
    let analyses = analyze_folder(&paths, &cancel, |d, t| {
        eprintln!("analyze {d}/{t}");
    })
    .expect("analyze_folder");

    let mut report = String::new();
    for (i, a) in analyses.iter().enumerate() {
        let stem = format!("{i:02}_{:?}", a.role);
        let src = image::open(&a.path).expect("open src");
        thumb(src.clone(), 640)
            .save(out.join(format!("{stem}_src.jpg")))
            .unwrap();
        let mask = isolate_largest_blob(&image::open(&a.mask_png).unwrap().into_luma8());
        DynamicImage::ImageLuma8(mask.clone())
            .resize(640, 640, image::imageops::FilterType::Triangle)
            .save(out.join(format!("{stem}_mask.png")))
            .unwrap();

        let (mw, mh) = mask.dimensions();
        let mut rgba = src.resize_exact(mw, mh, image::imageops::FilterType::Triangle).to_rgba8();
        for (x, y, p) in rgba.enumerate_pixels_mut() {
            p[3] = harden_cutout_alpha(mask.get_pixel(x, y)[0]);
        }
        checker_cutout(&rgba)
            .save(out.join(format!("{stem}_cutout.png")))
            .unwrap();

        report.push_str(&format!(
            "{}  role={:?} complete={} bbox={:?} mask={}\n",
            a.path.file_name().unwrap().to_string_lossy(),
            a.role,
            a.complete_subject,
            a.subject_bbox,
            a.mask_png.display()
        ));
    }
    fs::write(out.join("roles.txt"), report).unwrap();
    eprintln!("wrote {}", out.display());
}
