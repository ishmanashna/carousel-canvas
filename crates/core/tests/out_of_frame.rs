//! Out-of-frame placer integration tests (synthetic analyses, no ONNX).

use std::path::PathBuf;

use core::analysis::{OccupancyMap, PhotoAnalysis, PhotoRole};
use core::layout_out_of_frame::{
    build_canvas_occupancy_for_slots, bottom_band_mean, paper_occupancy_hits_slice_lines,
    pose_passes_figure_rules, resolve_out_of_frame_slots,
};
use core::slot::StripSlotDef;

const CANVAS_W: i32 = 10800;
const CANVAS_H: i32 = 1350;
const SLICE_W: i32 = 1080;

fn uniform_occupancy(w: u32, h: u32, val: u8) -> OccupancyMap {
    OccupancyMap {
        width: w,
        height: h,
        occupied: vec![val; (w * h) as usize],
    }
}

fn rect_occupancy(w: u32, h: u32, x0: u32, y0: u32, rw: u32, rh: u32, val: u8) -> OccupancyMap {
    let mut occupied = vec![0u8; (w * h) as usize];
    for y in y0..y0 + rh {
        for x in x0..x0 + rw {
            if x < w && y < h {
                occupied[(y * w + x) as usize] = val;
            }
        }
    }
    OccupancyMap {
        width: w,
        height: h,
        occupied,
    }
}

fn fake_analysis(
    name: &str,
    role: PhotoRole,
    occ: OccupancyMap,
    bbox: [i32; 4],
) -> PhotoAnalysis {
    PhotoAnalysis {
        path: PathBuf::from(name),
        role,
        occupancy: occ,
        subject_bbox: bbox,
        mask_png: PathBuf::from(format!("{name}.mask.png")),
        complete_subject: role == PhotoRole::Figure,
    }
}

fn paper(name: &str, occ: OccupancyMap) -> PhotoAnalysis {
    fake_analysis(name, PhotoRole::Paper, occ, [0, 0, 800, 1000])
}

fn figure(name: &str, occ: OccupancyMap, bbox: [i32; 4]) -> PhotoAnalysis {
    fake_analysis(name, PhotoRole::Figure, occ, bbox)
}

fn paper_slots(slots: &[StripSlotDef]) -> Vec<&StripSlotDef> {
    slots.iter().filter(|s| !s.cutout).collect()
}

fn figure_slots(slots: &[StripSlotDef]) -> Vec<&StripSlotDef> {
    slots.iter().filter(|s| s.cutout).collect()
}

fn paper_occ_with_peak(peak_center: bool) -> OccupancyMap {
    if peak_center {
        rect_occupancy(40, 50, 12, 10, 16, 30, 220)
    } else {
        rect_occupancy(40, 50, 2, 2, 36, 46, 200)
    }
}

fn fixture_gallery() -> Vec<PhotoAnalysis> {
    let paper_occ = paper_occ_with_peak;
    let mut out = Vec::new();
    for i in 0..10 {
        let peak = i == 0;
        out.push(paper(
            &format!("paper_{i}.jpg"),
            paper_occ(peak),
        ));
    }
    for i in 0..8 {
        out.push(figure(
            &format!("figure_{i}.jpg"),
            uniform_occupancy(20, 40, 200),
            [20, 5, 80, 180],
        ));
    }
    out
}

fn paper_covers_point(papers: &[&StripSlotDef], px: i32, py: i32) -> bool {
    papers.iter().any(|p| px >= p.x && px < p.x + p.w && py >= p.y && py < p.y + p.h)
}

#[test]
fn papers_cover_canvas_no_cream_gutters() {
    let slots = resolve_out_of_frame_slots(&fixture_gallery(), CANVAS_W, CANVAS_H, SLICE_W, 7);
    let papers = paper_slots(&slots);
    assert!(!papers.is_empty());
    assert!(
        papers[0].x <= 0,
        "first paper must overscan the left edge, got x={}",
        papers[0].x
    );
    let last = papers.last().unwrap();
    assert!(
        last.x + last.w >= CANVAS_W,
        "last paper must overscan the right edge, got right={}",
        last.x + last.w
    );
    for p in &papers {
        assert!(
            p.rotation_deg.abs() < 0.05,
            "paper rotation {} leaves cream corners",
            p.rotation_deg
        );
        assert!(
            p.y <= 0 && p.y + p.h >= CANVAS_H,
            "paper must cover full canvas height, y={} h={}",
            p.y,
            p.h
        );
    }
    for x in (0..CANVAS_W).step_by(120) {
        for y in [0, CANVAS_H / 2, CANVAS_H - 1] {
            assert!(
                paper_covers_point(&papers, x, y),
                "canvas gap at ({x},{y}) — cream would show"
            );
        }
    }
}

#[test]
fn papers_overlap_on_x() {
    let slots = resolve_out_of_frame_slots(&fixture_gallery(), CANVAS_W, CANVAS_H, SLICE_W, 42);
    let papers = paper_slots(&slots);
    assert!(papers.len() >= 2, "expected multiple papers");
    for pair in papers.windows(2) {
        let prev = pair[0];
        let next = pair[1];
        assert!(
            prev.x + prev.w > next.x,
            "papers must overlap on x: prev right {} <= next x {}",
            prev.x + prev.w,
            next.x
        );
    }
}

#[test]
fn figure_not_centered_on_paper_peak() {
    let gallery = fixture_gallery();
    let slots = resolve_out_of_frame_slots(&gallery, CANVAS_W, CANVAS_H, SLICE_W, 7);
    let papers = paper_slots(&slots);
    let figures = figure_slots(&slots);
    assert!(!figures.is_empty(), "expected at least one figure");

    let paper_maps: Vec<(StripSlotDef, OccupancyMap)> = papers
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let occ = paper_occ_with_peak(i == 0);
            ((*s).clone(), occ)
        })
        .collect();
    let refs: Vec<(StripSlotDef, &OccupancyMap)> =
        paper_maps.iter().map(|(s, o)| (s.clone(), o)).collect();
    let occ = build_canvas_occupancy_for_slots(CANVAS_W, CANVAS_H, &refs);
    let (peak_cx, peak_cy) = occ.occupancy_peak_center();

    for fig in figures {
        let fcx = fig.x + fig.w / 2;
        let fcy = fig.y + fig.h / 2;
        let dx = (fcx - peak_cx).abs();
        let dy = (fcy - peak_cy).abs();
        assert!(
            dx > fig.w / 4 || dy > fig.h / 4,
            "figure center ({fcx},{fcy}) too close to mapped occupancy peak ({peak_cx},{peak_cy})"
        );
    }
}

#[test]
fn figure_bottom_band_in_empty_paper() {
    let slots = resolve_out_of_frame_slots(&fixture_gallery(), CANVAS_W, CANVAS_H, SLICE_W, 99);
    let papers = paper_slots(&slots);
    let figures = figure_slots(&slots);
    assert!(!figures.is_empty());

    let occ_maps: Vec<(StripSlotDef, OccupancyMap)> = papers
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let occ = if i == 0 {
                rect_occupancy(40, 50, 12, 10, 16, 30, 220)
            } else {
                uniform_occupancy(40, 50, 200)
            };
            ((*s).clone(), occ)
        })
        .collect();
    let refs: Vec<(StripSlotDef, &OccupancyMap)> =
        occ_maps.iter().map(|(s, o)| (s.clone(), o)).collect();
    let occ = build_canvas_occupancy_for_slots(CANVAS_W, CANVAS_H, &refs);

    for fig in figures {
        let band_y = fig.y + (fig.h as f64 * 0.82).floor() as i32;
        let band_h = fig.y + fig.h - band_y;
        let inside = papers.iter().any(|p| {
            fig.x >= p.x
                && band_y >= p.y
                && fig.x + fig.w <= p.x + p.w
                && band_y + band_h <= p.y + p.h
        });
        assert!(inside, "figure bottom band must sit inside a paper rect");
        assert!(
            bottom_band_mean(fig, &occ) < 0.08,
            "bottom band mean occupancy {:.3} too high",
            bottom_band_mean(fig, &occ)
        );
    }
}

#[test]
fn no_inner_band_slice_split() {
    let slots = resolve_out_of_frame_slots(&fixture_gallery(), CANVAS_W, CANVAS_H, SLICE_W, 123);
    let figures = figure_slots(&slots);
    let slice_count = CANVAS_W / SLICE_W;
    for fig in figures {
        let ix0 = fig.x + (fig.w as f64 * 0.20).round() as i32;
        let ix1 = fig.x + fig.w - (fig.w as f64 * 0.20).round() as i32;
        for k in 1..slice_count {
            let sx = k * SLICE_W;
            assert!(
                sx < ix0 || sx > ix1,
                "slice line x={sx} cuts figure inner band"
            );
        }
    }
}

#[test]
fn second_figure_avoids_first_bbox() {
    let slots = resolve_out_of_frame_slots(&fixture_gallery(), CANVAS_W, CANVAS_H, SLICE_W, 55);
    let figures = figure_slots(&slots);
    if figures.len() < 2 {
        return;
    }
    let a = figures[0];
    let b = figures[1];
    let ix0 = a.x.max(b.x);
    let iy0 = a.y.max(b.y);
    let ix1 = (a.x + a.w).min(b.x + b.w);
    let iy1 = (a.y + a.h).min(b.y + b.h);
    let inter = if ix1 > ix0 && iy1 > iy0 {
        (ix1 - ix0) as f64 * (iy1 - iy0) as f64
    } else {
        0.0
    };
    let area_a = (a.w * a.h) as f64;
    let area_b = (b.w * b.h) as f64;
    assert!(inter / area_a <= 0.10 && inter / area_b <= 0.10);
}

#[test]
fn high_occupancy_feet_pose_rejected() {
    let paper_slot = StripSlotDef::new(100, 50, 1400, 1200);
    let face_band = rect_occupancy(40, 50, 8, 35, 24, 12, 255);
    let occ = build_canvas_occupancy_for_slots(
        CANVAS_W,
        CANVAS_H,
        &[(paper_slot.clone(), &face_band)],
    );
    let papers = vec![paper_slot];
    let placed: Vec<(i32, i32, i32, i32)> = vec![];

    let fw = 400;
    let fh = 900;
    let x = 600;
    let y = 400;
    assert!(
        !pose_passes_figure_rules(
            x,
            y,
            fw,
            fh,
            &papers,
            &occ,
            SLICE_W,
            CANVAS_W / SLICE_W,
            &placed,
        ),
        "pose hiding feet on high-occupancy face band must be rejected"
    );
}

#[test]
fn paper_occupancy_peak_on_slice_line_rejected() {
    // Dest 1500..2900 straddles x=2160 (k=2). Cover scale = max(1400/40, 1250/50) = 35,
    // so canvas x=2160 maps to occupancy x ≈ 18 in the inner 70% height band.
    let slot = StripSlotDef::new(1500, 50, 1400, 1250);
    let occ = rect_occupancy(40, 50, 18, 12, 3, 20, 255);
    assert!(
        paper_occupancy_hits_slice_lines(&slot, &occ, SLICE_W, CANVAS_W / SLICE_W),
        "occupancy peak on x=2160 in the face/torso band must be rejected"
    );
}

#[test]
fn slice1_is_covered_by_paper() {
    let slots = resolve_out_of_frame_slots(&fixture_gallery(), CANVAS_W, CANVAS_H, SLICE_W, 7);
    let papers = paper_slots(&slots);
    assert!(!papers.is_empty());
    for x in (0..SLICE_W).step_by(80) {
        for y in [0, CANVAS_H / 2, CANVAS_H - 1] {
            assert!(
                paper_covers_point(&papers, x, y),
                "slice 1 cream gap at ({x},{y})"
            );
        }
    }
}

#[test]
fn figure_slots_are_cutouts_with_mask() {
    let slots = resolve_out_of_frame_slots(&fixture_gallery(), CANVAS_W, CANVAS_H, SLICE_W, 3);
    for fig in figure_slots(&slots) {
        assert!(fig.cutout);
        assert!(fig.mask_path.is_some());
        assert!(fig.z_index >= 100);
        assert!(
            fig.source_crop.is_some(),
            "figure decode must crop to the subject, not Cover the whole photo"
        );
    }
}
