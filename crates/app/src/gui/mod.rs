mod settings;
mod slot_state;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use core::{
    fills_to_paths, get_template_by_id, max_strip_slots, pick_dumb_fills, pick_smart_fills,
    scan_image_folder, LayoutSnapshot, SlotFill, StripTemplate,
};
use eframe::egui;
use render::{
    compose_preview_gpu, parse_color_rgb, prepare_preview_from_snapshot, prepare_preview_phase1,
    prepare_preview_phase2, preview_ready_without_underfill, run_strip_export, CardEdge,
    Compositor, PreviewBuildResult, PreviewGpuReady, PreviewParams, PreviewPhase1,
    StripExportParams,
};

use settings::{load_settings, reveal_folder, save_settings, AppSettings};
use slot_state::SlotState;

pub const TEMPLATE_OPTIONS: &[(&str, &str)] = &[
    (
        "strip_mural_v2",
        "Mural v2 - 35 slots + underfill, hero 1st slice",
    ),
    ("strip_polaroid_table_v1", "Polaroid table - wood + 20 photos"),
    (
        "strip_seamless_mosaic_v1",
        "Seamless mosaic - seeded V/H layout",
    ),
    ("strip_seamless_v1", "Seamless v1 - flat overlap (9)"),
    ("strip_mural_v1", "Mural v1 - legacy (8)"),
    ("strip_10col", "10 columns - 1 photo / slide"),
];

const COLOR_PRESETS: &[(&str, &str)] = &[
    ("White", "white"),
    ("Black", "black"),
    ("Beige", "beige"),
    ("Ivory", "ivory"),
    ("Gray", "gray"),
    ("Light gray", "lightgray"),
    ("Dark gray", "darkgray"),
    ("Wheat", "wheat"),
    ("Tan", "tan"),
    ("Navy", "navy"),
    ("Maroon", "maroon"),
];

const CARD_EDGE_LABELS: &[(&str, &str)] = &[
    ("Borderless (transparent wedges)", "borderless"),
    ("Beige wedges (classic)", "wedges"),
    ("Border (uses color below)", "border"),
];

const VIEW_ZOOM_MIN: f32 = 0.25;
const VIEW_ZOOM_MAX: f32 = 8.0;
const SLICE_FOOTER_H: f32 = 18.0;
const DRAG_THRESHOLD_SQ: f32 = 36.0;

fn clamp_scroll_offset(offset: egui::Vec2, content: egui::Vec2, viewport: egui::Vec2) -> egui::Vec2 {
    let max_x = (content.x - viewport.x).max(0.0);
    let max_y = (content.y - viewport.y).max(0.0);
    egui::vec2(offset.x.clamp(0.0, max_x), offset.y.clamp(0.0, max_y))
}

fn mural_content_layout(pw: f32, ph: f32, viewport: egui::Rect) -> (f32, f32, f32, f32) {
    let total_h = ph + SLICE_FOOTER_H;
    let content_w = pw.max(viewport.width());
    let content_h = total_h.max(viewport.height());
    let pad_x = (content_w - pw) * 0.5;
    let pad_y = (content_h - total_h) * 0.5;
    (content_w, content_h, pad_x, pad_y)
}

fn draw_corner_marks(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let n = 10.0_f32.min(rect.width() * 0.25).min(rect.height() * 0.25);
    let stroke = egui::Stroke::new(1.5_f32, color);
    let tl = rect.left_top();
    let tr = rect.right_top();
    let bl = rect.left_bottom();
    let br = rect.right_bottom();
    painter.line_segment([tl, tl + egui::vec2(n, 0.0)], stroke);
    painter.line_segment([tl, tl + egui::vec2(0.0, n)], stroke);
    painter.line_segment([tr, tr + egui::vec2(-n, 0.0)], stroke);
    painter.line_segment([tr, tr + egui::vec2(0.0, n)], stroke);
    painter.line_segment([bl, bl + egui::vec2(n, 0.0)], stroke);
    painter.line_segment([bl, bl + egui::vec2(0.0, -n)], stroke);
    painter.line_segment([br, br + egui::vec2(-n, 0.0)], stroke);
    painter.line_segment([br, br + egui::vec2(0.0, -n)], stroke);
}

fn random_layout_seed() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_nanos() & 0x7FFF_FFFF) as i64)
        .unwrap_or(42)
}

fn assign_seed() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_nanos() & 0xFFFF_FFFF) as u32)
        .unwrap_or(42)
}

enum ExportMsg {
    Done(Result<render::ExportResult, String>),
}

enum PreviewMsg {
    Phase1(Result<PreviewPhase1, String>),
    Phase2(Result<PreviewGpuReady, String>),
    Snapshot(Result<PreviewGpuReady, String>),
}

enum PreviewBuildUi {
    Ok(render::PreviewBuildResult),
    Err(String),
}

enum LayerAdjust {
    Forward,
    Backward,
    ToFront,
    ToBack,
}

#[derive(Clone)]
struct StripView {
    /// Screen pixels per full template canvas pixel.
    scale: f32,
    offset_x: f32,
    offset_y: f32,
    preview_w: f32,
    preview_h: f32,
    canvas_w: f32,
    canvas_h: f32,
}

pub struct CarouselApp {
    settings: AppSettings,
    template: StripTemplate,
    layout_seed: i64,
    slots: SlotState,
    source_paths: Vec<PathBuf>,

    preview_texture: Option<egui::TextureHandle>,
    preview_overlay_slots: Vec<core::StripSlotDef>,
    preview_dirty: bool,
    preview_building: bool,
    strip_view: Option<StripView>,
    preview_error: Option<String>,

    compositor: Option<Compositor>,
    status: String,

    /// 1.0 = fit entire mural in the preview viewport.
    view_zoom: f32,
    scroll_offset: egui::Vec2,
    selected_slot: Option<usize>,

    stage_press: Option<egui::Pos2>,
    press_slot: Option<usize>,
    view_pan_active: bool,
    view_pan_scroll_anchor: egui::Vec2,
    drag_moved: bool,
    pan_drag_slot: Option<usize>,
    pan_anchor: Option<(f64, f64)>,
    pan_live: Option<(f64, f64)>,
    swap_pickup: Option<usize>,
    swap_hover: Option<usize>,
    move_drag_slot: Option<usize>,
    move_anchor_canvas: Option<(i32, i32)>,
    move_live_canvas: Option<(i32, i32)>,

    export_rx: Option<Receiver<ExportMsg>>,
    export_running: bool,
    last_export_dir: Option<PathBuf>,

    preview_rx: Option<Receiver<(u64, PreviewMsg)>>,
    preview_generation: u64,

    layout_locked: bool,
    layout_snapshot: Option<LayoutSnapshot>,
    show_unlock_confirm: bool,
    pending_unlock_browse: bool,
}

impl CarouselApp {
    fn new() -> Self {
        let settings = load_settings();
        let template = get_template_by_id(&settings.strip_template)
            .unwrap_or_else(|_| get_template_by_id("strip_mural_v2").unwrap());
        let mut app = Self {
            settings,
            template,
            layout_seed: random_layout_seed(),
            slots: SlotState::new(),
            source_paths: Vec::new(),
            preview_texture: None,
            preview_overlay_slots: Vec::new(),
            preview_dirty: true,
            preview_building: false,
            strip_view: None,
            preview_error: None,
            compositor: None,
            status: "Ready.".into(),
            view_zoom: 1.0,
            scroll_offset: egui::Vec2::ZERO,
            selected_slot: None,
            stage_press: None,
            press_slot: None,
            view_pan_active: false,
            view_pan_scroll_anchor: egui::Vec2::ZERO,
            drag_moved: false,
            pan_drag_slot: None,
            pan_anchor: None,
            pan_live: None,
            swap_pickup: None,
            swap_hover: None,
            move_drag_slot: None,
            move_anchor_canvas: None,
            move_live_canvas: None,
            export_rx: None,
            export_running: false,
            last_export_dir: None,
            preview_rx: None,
            preview_generation: 0,
            layout_locked: false,
            layout_snapshot: None,
            show_unlock_confirm: false,
            pending_unlock_browse: false,
        };
        app.reload_folder();
        app
    }

    fn sync_settings_from_ui(&mut self) {
        save_settings(&self.settings);
    }

    fn template_id_index(&self) -> usize {
        TEMPLATE_OPTIONS
            .iter()
            .position(|(id, _)| *id == self.settings.strip_template)
            .unwrap_or(0)
    }

    fn card_edge_index(&self) -> usize {
        CARD_EDGE_LABELS
            .iter()
            .position(|(_, v)| *v == self.settings.strip_card_edge)
            .unwrap_or(0)
    }

    fn card_edge(&self) -> CardEdge {
        CardEdge::parse(&self.settings.strip_card_edge).unwrap_or(CardEdge::Borderless)
    }

    fn border_rgb(&self) -> Option<[u8; 3]> {
        if self.card_edge() == CardEdge::Border {
            Some(parse_color_rgb(&self.settings.color))
        } else {
            None
        }
    }

    fn include_underfill(&self) -> bool {
        self.card_edge() == CardEdge::Borderless && self.template.background_underfill_count() > 0
    }

    fn flagship_slot(&self) -> Option<usize> {
        self.template.layout_flagship_slot_index
    }

    fn fill_required_bitmap(&self) -> Vec<bool> {
        if let Some(snap) = &self.layout_snapshot {
            snap.fill_required.clone()
        } else {
            self.template
                .strip_effective_fill_required(Some(self.layout_seed))
        }
    }

    fn canvas_dimensions(&self) -> (i32, i32) {
        if let Some(snap) = &self.layout_snapshot {
            (snap.canvas_width, snap.canvas_height)
        } else {
            (self.template.canvas_width, self.template.canvas_height)
        }
    }

    fn unlock_layout(&mut self) {
        self.layout_locked = false;
        self.layout_snapshot = None;
        self.slots.set_pending_geometry(None);
        self.show_unlock_confirm = false;
        self.pending_unlock_browse = false;
        self.status =
            "Layout unlocked — Shuffle or New layout seed to reposition cards.".into();
    }

    fn locked_geometry_snapshot(&self) -> Option<Vec<core::StripSlotDef>> {
        self.layout_snapshot.as_ref().map(|s| s.slots.clone())
    }

    fn sync_undo_geometry(&mut self) {
        self.slots
            .set_pending_geometry(self.locked_geometry_snapshot());
    }

    fn apply_geometry_restore(&mut self, geometry: Option<Vec<core::StripSlotDef>>) {
        if let (Some(snap), Some(geo)) = (&mut self.layout_snapshot, geometry) {
            snap.slots = geo.clone();
            self.preview_overlay_slots = geo;
        }
    }

    fn overlay_slot_xy(&self, si: usize, slot: &core::StripSlotDef) -> (i32, i32) {
        if self.move_drag_slot == Some(si) {
            if let Some((x, y)) = self.move_live_canvas {
                return (x, y);
            }
        }
        (slot.x, slot.y)
    }

    fn clamp_slot_position(&self, x: i32, y: i32, w: i32, h: i32) -> (i32, i32) {
        let (cw, ch) = self.canvas_dimensions();
        let min_vis_w = (w as f64 * 0.25).round() as i32;
        let min_vis_h = (h as f64 * 0.25).round() as i32;
        let nx = x.clamp(-w + min_vis_w, cw - min_vis_w);
        let ny = y.clamp(-h + min_vis_h, ch - min_vis_h);
        (nx, ny)
    }

    fn slot_allows_move(&self, slot: usize) -> bool {
        self.layout_locked
            && self.slot_allows_crop_pan(slot)
            && self.layout_snapshot.is_some()
    }

    fn layer_adjust(&mut self, slot: usize, mode: LayerAdjust) {
        if !self.layout_locked || !self.slot_allows_crop_pan(slot) {
            return;
        }
        if self.layout_snapshot.is_none() {
            return;
        }
        self.sync_undo_geometry();
        self.slots.checkpoint();
        let Some(snap) = self.layout_snapshot.as_mut() else {
            return;
        };
        let mut order: Vec<usize> = (0..snap.slots.len()).collect();
        order.sort_by_key(|i| snap.slots[*i].z_index);
        let pos = order.iter().position(|&i| i == slot);
        let Some(pos) = pos else {
            return;
        };
        match mode {
            LayerAdjust::Forward if pos + 1 < order.len() => {
                let next = order[pos + 1];
                let z = snap.slots[slot].z_index;
                snap.slots[slot].z_index = snap.slots[next].z_index;
                snap.slots[next].z_index = z;
            }
            LayerAdjust::Backward if pos > 0 => {
                let prev = order[pos - 1];
                let z = snap.slots[slot].z_index;
                snap.slots[slot].z_index = snap.slots[prev].z_index;
                snap.slots[prev].z_index = z;
            }
            LayerAdjust::ToFront => {
                let max_z = snap.slots.iter().map(|s| s.z_index).max().unwrap_or(0);
                snap.slots[slot].z_index = max_z + 1;
            }
            LayerAdjust::ToBack => {
                let min_z = snap.slots.iter().map(|s| s.z_index).min().unwrap_or(0);
                snap.slots[slot].z_index = min_z - 1;
            }
            _ => return,
        }
        order.sort_by_key(|i| snap.slots[*i].z_index);
        for (z, i) in order.iter().enumerate() {
            snap.slots[*i].z_index = z as i32;
        }
        self.preview_overlay_slots = snap.slots.clone();
        self.preview_dirty = true;
        self.status = format!("Slot {} layer updated.", slot + 1);
    }

    fn capture_layout_snapshot(&mut self, built: &PreviewBuildResult) {
        self.layout_snapshot = Some(LayoutSnapshot::capture(
            &self.template,
            self.layout_seed,
            &self.settings.strip_card_edge,
            built.underfill_boxes.clone(),
            built.underfill_paths.clone(),
        ));
        self.layout_locked = true;
    }

    fn effective_num_slots(&self) -> usize {
        if let Some(snap) = &self.layout_snapshot {
            snap.slots.len()
        } else {
            self.template
                .strip_effective_num_slots(Some(self.layout_seed))
        }
    }

    fn required_count(&self) -> usize {
        self.fill_required_bitmap()
            .iter()
            .filter(|&&r| r)
            .count()
    }

    fn pick_image_file(&self) -> Option<PathBuf> {
        let mut dlg = rfd::FileDialog::new().add_filter(
            "Images",
            &["jpg", "jpeg", "png", "webp"],
        );
        let folder = PathBuf::from(&self.settings.input_folder);
        if folder.is_dir() {
            dlg = dlg.set_directory(folder);
        }
        dlg.pick_file()
    }

    fn reload_folder(&mut self) {
        if self.layout_locked {
            self.status = "Unlock layout before changing the input folder.".into();
            return;
        }
        let folder = PathBuf::from(&self.settings.input_folder);
        if !folder.is_dir() {
            self.source_paths.clear();
            return;
        }
        match scan_image_folder(&folder) {
            Ok(paths) => {
                self.source_paths = paths;
                self.refill_from_folder(true);
            }
            Err(e) => self.status = format!("Folder scan failed: {e}"),
        }
    }

    fn refill_from_folder(&mut self, randomize: bool) {
        if self.layout_locked {
            self.status = "Unlock layout before shuffling photos.".into();
            return;
        }
        if self.source_paths.is_empty() {
            self.slots.reset();
            self.selected_slot = None;
            self.preview_dirty = true;
            return;
        }
        if randomize {
            self.layout_seed = random_layout_seed();
        }
        let seed = assign_seed();
        let layout_seed = Some(self.layout_seed);
        let path_fills = if self.settings.strip_smart_shuffle {
            pick_smart_fills(
                &self.source_paths,
                &self.template,
                layout_seed,
                seed,
            )
        } else {
            pick_dumb_fills(
                &self.source_paths,
                &self.template,
                layout_seed,
                seed,
            )
        };
        match path_fills {
            Ok(fills) => {
                let cap = max_strip_slots();
                let mut paths: Vec<Option<PathBuf>> = vec![None; cap];
                for (i, f) in fills.into_iter().enumerate() {
                    if i < cap {
                        paths[i] = f;
                    }
                }
                self.slots.set_assignments_from_paths(paths);
                self.slots.clear_undo();
                self.selected_slot = None;
                self.preview_dirty = true;
                self.status = format!("Shuffled {} photos into slots.", self.source_paths.len());
            }
            Err(e) => self.status = format!("Assign failed: {e}"),
        }
    }

    fn fills_for_preview(&self) -> Vec<Option<SlotFill>> {
        let n = self.effective_num_slots();
        let drag = self.pan_drag_slot;
        let live = self.pan_live;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let f = self.slots.assignments.get(i).and_then(|s| s.clone());
            if let Some(mut fill) = f {
                if drag == Some(i) {
                    if let Some((px, py)) = live {
                        fill.pan_x = px;
                        fill.pan_y = py;
                    }
                }
                out.push(Some(fill));
            } else {
                out.push(None);
            }
        }
        out
    }

    fn preview_ready(&self) -> bool {
        let req = self.fill_required_bitmap();
        let n = self.effective_num_slots();
        !(0..n).any(|i| req.get(i) == Some(&true) && self.slots.assignments[i].is_none())
    }

    fn ensure_compositor(&mut self, frame: &eframe::Frame) {
        if self.compositor.is_some() {
            return;
        }
        if let Some(rs) = frame.wgpu_render_state() {
            self.compositor = Some(Compositor::new(rs.device.clone(), rs.queue.clone()));
        }
    }

    fn rebuild_preview(&mut self, ctx: &egui::Context) {
        if !self.preview_dirty || self.preview_building || self.compositor.is_none() {
            return;
        }
        if !self.preview_ready() {
            // Keep last texture if any; clear overlay so gestures don't hit stale slots.
            self.strip_view = None;
            self.preview_error = None;
            self.preview_dirty = false;
            self.preview_rx = None;
            return;
        }
        self.preview_building = true;
        self.preview_dirty = false;
        self.preview_generation = self.preview_generation.wrapping_add(1);
        let gen = self.preview_generation;

        if self.layout_locked {
            let Some(snapshot) = self.layout_snapshot.clone() else {
                self.layout_locked = false;
                self.preview_building = false;
                self.preview_dirty = true;
                return;
            };
            let fills = self.fills_for_preview();
            let border_rgb = self.border_rgb();
            let (tx, rx) = mpsc::channel();
            self.preview_rx = Some(rx);
            thread::spawn(move || {
                let result =
                    prepare_preview_from_snapshot(snapshot, fills, border_rgb).map_err(|e| e.to_string());
                let _ = tx.send((gen, PreviewMsg::Snapshot(result)));
            });
            ctx.request_repaint();
            return;
        }

        let params = PreviewParams {
            template: self.template.clone(),
            fills: self.fills_for_preview(),
            source_paths: self.source_paths.clone(),
            layout_seed: self.layout_seed,
            card_edge: self.card_edge(),
            border_rgb: self.border_rgb(),
            include_underfill: self.include_underfill(),
        };
        let (tx, rx) = mpsc::channel();
        self.preview_rx = Some(rx);
        thread::spawn(move || {
            let result = prepare_preview_phase1(params).map_err(|e| e.to_string());
            let _ = tx.send((gen, PreviewMsg::Phase1(result)));
        });
        ctx.request_repaint();
    }

    fn apply_preview_result(&mut self, ctx: &egui::Context, result: PreviewBuildUi) {
        match result {
            PreviewBuildUi::Ok(built) => {
                let size = [built.rgb.width() as usize, built.rgb.height() as usize];
                let pixels: Vec<egui::Color32> = built
                    .rgb
                    .pixels()
                    .map(|p| egui::Color32::from_rgb(p[0], p[1], p[2]))
                    .collect();
                let image = egui::ColorImage { size, pixels };
                if !self.layout_locked {
                    self.capture_layout_snapshot(&built);
                    self.status = "Layout locked.".into();
                }
                self.preview_texture = Some(ctx.load_texture(
                    "mural_preview",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
                self.preview_overlay_slots = built.overlay_slots;
                self.preview_error = None;
                let (cw, ch) = self.canvas_dimensions();
                self.strip_view = Some(StripView {
                    canvas_w: cw as f32,
                    canvas_h: ch as f32,
                    preview_w: 0.0,
                    preview_h: 0.0,
                    scale: 0.0,
                    offset_x: 0.0,
                    offset_y: 0.0,
                });
            }
            PreviewBuildUi::Err(e) => {
                // Keep prior texture while showing the error in status/overlay.
                self.preview_error = Some(e);
            }
        }
        self.preview_building = false;
        self.preview_rx = None;
        if self.preview_dirty {
            ctx.request_repaint();
        }
    }

    fn poll_preview(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.preview_rx.as_ref() else {
            return;
        };
        let Ok((gen, msg)) = rx.try_recv() else {
            return;
        };
        if gen != self.preview_generation {
            return;
        }
        // Newer edits landed while this job was in flight — drop and rebuild.
        if self.preview_dirty {
            self.preview_building = false;
            self.preview_rx = None;
            ctx.request_repaint();
            return;
        }
        match msg {
            PreviewMsg::Phase1(Err(e)) => {
                self.apply_preview_result(ctx, PreviewBuildUi::Err(e));
            }
            PreviewMsg::Phase1(Ok(phase1)) => {
                if !phase1.underfill_enabled {
                    let Some(compositor) = self.compositor.as_ref() else {
                        self.apply_preview_result(ctx, PreviewBuildUi::Err("no compositor".into()));
                        return;
                    };
                    let ready = preview_ready_without_underfill(phase1);
                    match compose_preview_gpu(compositor, &ready) {
                        Ok(built) => self.apply_preview_result(ctx, PreviewBuildUi::Ok(built)),
                        Err(e) => self.apply_preview_result(ctx, PreviewBuildUi::Err(e.to_string())),
                    }
                    return;
                }
                // Full-res flatten + underfill plan on offscreen worker GPU.
                let (tx, rx2) = mpsc::channel();
                self.preview_rx = Some(rx2);
                let gen = self.preview_generation;
                thread::spawn(move || {
                    let result = prepare_preview_phase2(phase1).map_err(|e| e.to_string());
                    let _ = tx.send((gen, PreviewMsg::Phase2(result)));
                });
                ctx.request_repaint();
            }
            PreviewMsg::Phase2(Err(e)) => {
                self.apply_preview_result(ctx, PreviewBuildUi::Err(e));
            }
            PreviewMsg::Phase2(Ok(ready)) => {
                let Some(compositor) = self.compositor.as_ref() else {
                    self.apply_preview_result(ctx, PreviewBuildUi::Err("no compositor".into()));
                    return;
                };
                match compose_preview_gpu(compositor, &ready) {
                    Ok(built) => self.apply_preview_result(ctx, PreviewBuildUi::Ok(built)),
                    Err(e) => self.apply_preview_result(ctx, PreviewBuildUi::Err(e.to_string())),
                }
            }
            PreviewMsg::Snapshot(Err(e)) => {
                self.apply_preview_result(ctx, PreviewBuildUi::Err(e));
            }
            PreviewMsg::Snapshot(Ok(ready)) => {
                let Some(compositor) = self.compositor.as_ref() else {
                    self.apply_preview_result(ctx, PreviewBuildUi::Err("no compositor".into()));
                    return;
                };
                match compose_preview_gpu(compositor, &ready) {
                    Ok(built) => self.apply_preview_result(ctx, PreviewBuildUi::Ok(built)),
                    Err(e) => self.apply_preview_result(ctx, PreviewBuildUi::Err(e.to_string())),
                }
            }
        }
    }

    fn hit_test_slot(&self, pos: egui::Pos2, view: &StripView) -> Option<usize> {
        let cx = (pos.x - view.offset_x) / view.scale;
        let cy = (pos.y - view.offset_y) / view.scale;
        let mut order: Vec<usize> = (0..self.preview_overlay_slots.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(self.preview_overlay_slots[i].z_index));
        for i in order {
            let s = &self.preview_overlay_slots[i];
            let (sx, sy) = self.overlay_slot_xy(i, s);
            if cx >= sx as f32
                && cx < (sx + s.w) as f32
                && cy >= sy as f32
                && cy < (sy + s.h) as f32
            {
                return Some(i);
            }
        }
        None
    }

    fn assign_to_slot(&mut self, slot: usize, path: PathBuf) {
        self.sync_undo_geometry();
        self.slots.assign(slot, path);
        self.selected_slot = Some(slot);
        self.preview_dirty = true;
        self.status = format!("Slot {} updated.", slot + 1);
    }

    fn pick_hero_photo(&mut self) {
        let Some(hi) = self.flagship_slot() else {
            self.status = "No hero slot on this template.".into();
            return;
        };
        let Some(path) = self.pick_image_file() else {
            return;
        };
        self.assign_to_slot(hi, path);
        self.status = "Hero photo updated.".into();
    }

    fn replace_selected_photo(&mut self) {
        let Some(slot) = self.selected_slot else {
            self.status = "Click a slot first, then Replace selected photo…".into();
            return;
        };
        let Some(path) = self.pick_image_file() else {
            return;
        };
        self.assign_to_slot(slot, path);
    }

    fn slot_allows_crop_pan(&self, slot: usize) -> bool {
        let req = self.fill_required_bitmap();
        req.get(slot) == Some(&true)
            && self
                .slots
                .assignments
                .get(slot)
                .and_then(|s| s.as_ref())
                .is_some()
    }

    fn slot_pan_sensitivity(&self, slot: usize, view: &StripView) -> Option<(f64, f64)> {
        let s = self.preview_overlay_slots.get(slot)?;
        let tws = (s.w as f32 * view.scale).max(1.0);
        let ths = (s.h as f32 * view.scale).max(1.0);
        Some((2.0 / tws as f64, 2.0 / ths as f64))
    }

    fn set_view_zoom(
        &mut self,
        zoom: f32,
        anchor_in_viewport: Option<egui::Pos2>,
        viewport: egui::Rect,
        content_size: egui::Vec2,
    ) {
        let old = self.view_zoom;
        let new = zoom.clamp(VIEW_ZOOM_MIN, VIEW_ZOOM_MAX);
        if (new - old).abs() < f32::EPSILON {
            return;
        }
        let clamp = |offset: egui::Vec2| clamp_scroll_offset(offset, content_size, viewport.size());
        if let Some(pointer) = anchor_in_viewport {
            let local = pointer - viewport.min;
            let content = local + self.scroll_offset;
            let ratio = new / old;
            self.scroll_offset = clamp(content * ratio - local);
        } else {
            let local = viewport.size() * 0.5;
            let content = local + self.scroll_offset;
            let ratio = new / old;
            self.scroll_offset = clamp(content * ratio - local);
        }
        self.view_zoom = new;
    }

    fn validate_export(&self) -> Result<(), String> {
        let folder = Path::new(&self.settings.input_folder);
        if !folder.is_dir() {
            return Err("Input folder does not exist.".into());
        }
        let out = PathBuf::from(&self.settings.output_folder);
        std::fs::create_dir_all(&out).map_err(|e| format!("Cannot create output folder: {e}"))?;
        if self.source_paths.is_empty() {
            return Err("Strip needs at least one photo in the input folder.".into());
        }
        if !self.preview_ready() {
            return Err(format!(
                "Fill all {} strip slots (use Shuffle, Pick hero, or Replace selected).",
                self.required_count()
            ));
        }
        let n = self.effective_num_slots();
        let fills: Vec<Option<SlotFill>> = self.slots.assignments[..n].to_vec();
        if self.settings.strip_allow_photo_repeats {
            return Ok(());
        }
        let need = self.required_count();
        let req = self.fill_required_bitmap();
        let mut seen = HashSet::new();
        for (i, fill) in fills.iter().enumerate() {
            if req.get(i) == Some(&true) {
                if let Some(f) = fill {
                    let key = std::fs::canonicalize(&f.path).unwrap_or_else(|_| f.path.clone());
                    seen.insert(key);
                }
            }
        }
        if seen.len() < need {
            return Err(format!(
                "Need {need} different photos; only {} distinct in slots. Allow repeats or add images.",
                seen.len()
            ));
        }
        Ok(())
    }

    fn start_export(&mut self) {
        if self.export_running {
            return;
        }
        if let Err(e) = self.validate_export() {
            self.status = e;
            return;
        }
        let n = self.effective_num_slots();
        let slot_fills: Vec<Option<SlotFill>> = self.slots.assignments[..n].to_vec();
        let fills = fills_to_paths(&slot_fills);
        let locked_layout = if self.layout_locked {
            self.layout_snapshot.clone()
        } else {
            None
        };
        let layout_seed = locked_layout
            .as_ref()
            .map(|s| s.layout_seed)
            .unwrap_or(self.layout_seed);
        let params = StripExportParams {
            template: self.template.clone(),
            fills,
            slot_fills: Some(slot_fills),
            source_paths: self.source_paths.clone(),
            layout_seed,
            card_edge: self.card_edge(),
            border_rgb: self.border_rgb(),
            no_layout_retry: self.layout_locked,
            locked_layout,
            output_dir: PathBuf::from(&self.settings.output_folder),
        };
        let (tx, rx) = mpsc::channel();
        self.export_rx = Some(rx);
        self.export_running = true;
        self.status = "Exporting…".into();
        thread::spawn(move || {
            let result = run_strip_export(params).map_err(|e| e.to_string());
            let _ = tx.send(ExportMsg::Done(result));
        });
    }

    fn poll_export(&mut self) {
        let msg = self.export_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(ExportMsg::Done(result)) = msg {
            self.export_running = false;
            self.export_rx = None;
            match result {
                Ok(res) => {
                    self.last_export_dir = Some(self.settings.output_folder.clone().into());
                    self.status = format!(
                        "Finished. Wrote {} slices + wide master.",
                        res.slice_paths.len()
                    );
                }
                Err(e) => self.status = format!("Export failed: {e}"),
            }
        }
    }

    fn draw_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Input");
            ui.add(
                egui::TextEdit::singleline(&mut self.settings.input_folder)
                    .desired_width(180.0),
            );
            if ui.button("Browse…").clicked() {
                if self.layout_locked {
                    self.pending_unlock_browse = true;
                    self.show_unlock_confirm = true;
                } else if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    self.settings.input_folder = p.display().to_string();
                    self.sync_settings_from_ui();
                    self.reload_folder();
                }
            }
            ui.separator();
            ui.label("Output");
            ui.add(
                egui::TextEdit::singleline(&mut self.settings.output_folder)
                    .desired_width(140.0),
            );
            if ui.button("Browse…").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    self.settings.output_folder = p.display().to_string();
                    self.sync_settings_from_ui();
                }
            }
        });
    }

    fn draw_preview(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Mural preview");
        if self.layout_locked {
            ui.colored_label(egui::Color32::from_rgb(120, 200, 140), "Layout locked");
        }
        ui.label(
            "One wide mural (10 Instagram slides side-by-side). Export cuts it into 10 portrait \
             JPEGs. Wheel zooms; drag to pan; Alt-drag on a photo to pan crop; click to select. \
             Double-click flips; Ctrl+drag moves (locked) or swaps (Auto); Ctrl+Shift+drag swaps \
             when locked; right-click clears.",
        );

        let viewport = ui.available_rect_before_wrap();
        let tex_size = self
            .preview_texture
            .as_ref()
            .map(|t| t.size_vec2())
            .unwrap_or(egui::vec2(1.0, 1.0));
        let pad = 0.96;
        let fit_scale = if self.preview_texture.is_some() {
            (viewport.width() * pad / tex_size.x).min(viewport.height() * pad / tex_size.y)
        } else {
            1.0
        };
        let display_scale = fit_scale * self.view_zoom;
        let pw = (tex_size.x * display_scale).max(1.0);
        let ph = (tex_size.y * display_scale).max(1.0);
        let (content_w, content_h, _, _) = mural_content_layout(pw, ph, viewport);
        let content_size = egui::vec2(content_w, content_h);

        ui.horizontal(|ui| {
            if ui.button("Zoom −").clicked() {
                let z = self.view_zoom / 1.25;
                self.set_view_zoom(z, None, viewport, content_size);
            }
            if ui.button("Fit").clicked() {
                self.view_zoom = 1.0;
                self.scroll_offset = egui::Vec2::ZERO;
            }
            if ui.button("Zoom +").clicked() {
                let z = self.view_zoom * 1.25;
                self.set_view_zoom(z, None, viewport, content_size);
            }
            ui.label(format!("{:.0}%", self.view_zoom * 100.0));
            ui.separator();
            let has_hero = self.flagship_slot().is_some();
            ui.add_enabled_ui(has_hero, |ui| {
                if ui
                    .button("Pick hero photo…")
                    .on_hover_text("Choose an image file for the flagship/hero slot")
                    .clicked()
                {
                    self.pick_hero_photo();
                }
            });
            let has_sel = self.selected_slot.is_some();
            ui.add_enabled_ui(has_sel, |ui| {
                if ui
                    .button("Replace selected photo…")
                    .on_hover_text("Replace the highlighted slot with a file from disk")
                    .clicked()
                {
                    self.replace_selected_photo();
                }
            });
            if let Some(s) = self.selected_slot {
                ui.label(format!("Selected: slot {}", s + 1));
            }
            if self.preview_building {
                ui.spinner();
                ui.label("Updating…");
            }
        });

        let slices = self.template.slice_count.max(1) as f32;

        // Wheel zooms toward cursor; consume scroll so ScrollArea does not also pan.
        if ui.rect_contains_pointer(viewport) {
            let scroll_y = ctx.input(|i| i.raw_scroll_delta.y);
            if scroll_y.abs() > 0.0 {
                let factor = if scroll_y > 0.0 { 1.1 } else { 1.0 / 1.1 };
                let anchor = ctx.pointer_hover_pos();
                self.set_view_zoom(self.view_zoom * factor, anchor, viewport, content_size);
                ctx.input_mut(|i| {
                    i.raw_scroll_delta = egui::Vec2::ZERO;
                    i.smooth_scroll_delta = egui::Vec2::ZERO;
                });
            }
        }

        let scroll_before = self.scroll_offset;
        let scroll_out = egui::ScrollArea::both()
            .id_salt("mural_preview_scroll")
            .auto_shrink([false, false])
            .drag_to_scroll(false)
            .scroll_offset(self.scroll_offset)
            .show(ui, |ui| {
                if !self.preview_ready() {
                    let need = self.required_count();
                    ui.allocate_ui_with_layout(
                        viewport.size(),
                        egui::Layout::centered_and_justified(egui::Direction::TopDown),
                        |ui| {
                            ui.label(format!(
                                "Fill all {need} photo slots to preview (Shuffle, Pick hero, or Replace selected)."
                            ));
                        },
                    );
                    return;
                }

                if let Some(err) = &self.preview_error {
                    if self.preview_texture.is_none() {
                        ui.colored_label(egui::Color32::LIGHT_RED, err);
                        return;
                    }
                }

                let Some(tex) = &self.preview_texture else {
                    if self.preview_building {
                        ui.allocate_ui_with_layout(
                            viewport.size(),
                            egui::Layout::centered_and_justified(egui::Direction::TopDown),
                            |ui| {
                                ui.label("Building preview…");
                            },
                        );
                    }
                    return;
                };

                let (content_w, content_h, pad_x, pad_y) = mural_content_layout(pw, ph, viewport);
                let (response, painter) = ui.allocate_painter(
                    egui::vec2(content_w, content_h),
                    egui::Sense::click_and_drag(),
                );
                let img_rect = egui::Rect::from_min_size(
                    response.rect.min + egui::vec2(pad_x, pad_y),
                    egui::vec2(pw, ph),
                );
                painter.rect_filled(img_rect, 0.0, egui::Color32::from_rgb(20, 20, 20));
                painter.image(
                    tex.id(),
                    img_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );

                let slice_w = pw / slices;
                for i in 0..=self.template.slice_count {
                    let x = img_rect.min.x + i as f32 * slice_w;
                    painter.line_segment(
                        [egui::pos2(x, img_rect.min.y), egui::pos2(x, img_rect.max.y)],
                        egui::Stroke::new(
                            1.0_f32,
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 70),
                        ),
                    );
                }
                let footer_center_y = img_rect.max.y + SLICE_FOOTER_H * 0.5;
                for i in 0..self.template.slice_count {
                    let label_x = img_rect.min.x + (i as f32 + 0.5) * slice_w;
                    painter.text(
                        egui::pos2(label_x, footer_center_y),
                        egui::Align2::CENTER_CENTER,
                        format!("{}", i + 1),
                        egui::FontId::proportional(11.0),
                        egui::Color32::from_rgb(140, 140, 140),
                    );
                }

                let (canvas_w, canvas_h) = self.canvas_dimensions();
                if let Some(ref mut view) = self.strip_view {
                    view.canvas_w = canvas_w as f32;
                    view.canvas_h = canvas_h as f32;
                    view.scale = pw / view.canvas_w;
                    view.offset_x = img_rect.min.x;
                    view.offset_y = img_rect.min.y;
                    view.preview_w = pw;
                    view.preview_h = ph;
                }

                let view = self.strip_view.clone();
                let Some(view) = view else {
                    return;
                };

                // Selection + swap highlights mapped through full template canvas coords.
                for (si, slot) in self.preview_overlay_slots.iter().enumerate() {
                    let (sx, sy) = self.overlay_slot_xy(si, slot);
                    let x0 = view.offset_x + sx as f32 * view.scale;
                    let y0 = view.offset_y + sy as f32 * view.scale;
                    let x1 = view.offset_x + (sx + slot.w) as f32 * view.scale;
                    let y1 = view.offset_y + (sy + slot.h) as f32 * view.scale;
                    let r = egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1));
                    if self.selected_slot == Some(si) && self.swap_pickup.is_none() {
                        draw_corner_marks(
                            &painter,
                            r,
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 210),
                        );
                    }
                    if self.swap_pickup == Some(si) {
                        painter.rect_stroke(
                            r.expand(2.0),
                            0.0,
                            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(102, 204, 255)),
                            egui::StrokeKind::Outside,
                        );
                    } else if self.swap_hover == Some(si) {
                        painter.rect_stroke(
                            r.expand(2.0),
                            0.0,
                            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(255, 170, 51)),
                            egui::StrokeKind::Outside,
                        );
                    }
                }

                let (ctrl, shift, alt) =
                    ctx.input(|i| (i.modifiers.ctrl, i.modifiers.shift, i.modifiers.alt));

                if response.clicked() && !ctrl {
                    if let Some(pos) = response.interact_pointer_pos() {
                        self.selected_slot = self.hit_test_slot(pos, &view);
                    }
                }

                if response.secondary_clicked() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        if let Some(slot) = self.hit_test_slot(pos, &view) {
                            let req = self.fill_required_bitmap();
                            if self.layout_locked && req.get(slot) == Some(&true) {
                                self.status =
                                    "Cannot clear a required slot while layout is locked.".into();
                            } else {
                                self.sync_undo_geometry();
                                self.slots.clear_slot(slot);
                                if self.selected_slot == Some(slot) {
                                    self.selected_slot = None;
                                }
                                self.preview_dirty = true;
                                self.status = format!("Slot {} cleared.", slot + 1);
                            }
                        }
                    }
                }

                if response.double_clicked() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        if let Some(slot) = self.hit_test_slot(pos, &view) {
                            if self.slots.assignments[slot].is_some() {
                                self.sync_undo_geometry();
                                self.slots.checkpoint();
                                if let Some(fill) = &mut self.slots.assignments[slot] {
                                    fill.flip_h = !fill.flip_h;
                                    self.preview_dirty = true;
                                    self.status = format!(
                                        "Slot {}: flip {}.",
                                        slot + 1,
                                        if fill.flip_h { "on" } else { "off" }
                                    );
                                }
                            }
                        }
                    }
                }

                if response.drag_started() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        self.stage_press = Some(pos);
                        self.press_slot = self.hit_test_slot(pos, &view);
                        self.drag_moved = false;
                        if ctrl {
                            if let Some(slot) = self.press_slot {
                                if self.slots.assignments[slot].is_some() {
                                    if self.layout_locked && !shift && self.slot_allows_move(slot) {
                                        let s = &self.preview_overlay_slots[slot];
                                        self.move_drag_slot = Some(slot);
                                        self.move_anchor_canvas = Some((s.x, s.y));
                                        self.move_live_canvas = Some((s.x, s.y));
                                        self.status =
                                            format!("Slot {} — Ctrl-drag to move.", slot + 1);
                                    } else if !self.layout_locked || shift {
                                        self.swap_pickup = Some(slot);
                                        self.status = format!(
                                            "Slot {} picked — drop on another slot.",
                                            slot + 1
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                if response.dragged() {
                    if let Some(slot) = self.move_drag_slot {
                        if let (Some(press), Some((ax, ay)), Some(pos)) = (
                            self.stage_press,
                            self.move_anchor_canvas,
                            response.interact_pointer_pos(),
                        ) {
                            let dx = ((pos.x - press.x) / view.scale).round() as i32;
                            let dy = ((pos.y - press.y) / view.scale).round() as i32;
                            let s = &self.preview_overlay_slots[slot];
                            let (nx, ny) =
                                self.clamp_slot_position(ax + dx, ay + dy, s.w, s.h);
                            self.move_live_canvas = Some((nx, ny));
                            if nx != ax || ny != ay {
                                self.drag_moved = true;
                            }
                        }
                    } else if let Some(src) = self.swap_pickup {
                        if let Some(pos) = response.interact_pointer_pos() {
                            self.swap_hover = self.hit_test_slot(pos, &view);
                        }
                        let _ = src;
                    } else if let Some(press) = self.stage_press {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let dx = pos.x - press.x;
                            let dy = pos.y - press.y;
                            if !self.drag_moved && (dx * dx + dy * dy) < DRAG_THRESHOLD_SQ {
                                // wait for a real drag
                            } else {
                                if !self.drag_moved {
                                    self.drag_moved = true;
                                    if alt && !ctrl {
                                        if let Some(slot) = self.press_slot {
                                            if self.slot_allows_crop_pan(slot) {
                                                self.pan_drag_slot = Some(slot);
                                                if let Some(fill) = &self.slots.assignments[slot] {
                                                    self.pan_anchor = Some((fill.pan_x, fill.pan_y));
                                                }
                                            } else {
                                                self.view_pan_active = true;
                                                self.view_pan_scroll_anchor = self.scroll_offset;
                                            }
                                        } else {
                                            self.view_pan_active = true;
                                            self.view_pan_scroll_anchor = self.scroll_offset;
                                        }
                                    } else if !ctrl && !alt {
                                        self.view_pan_active = true;
                                        self.view_pan_scroll_anchor = self.scroll_offset;
                                    }
                                }
                                if self.view_pan_active {
                                    let delta = pos - press;
                                    self.scroll_offset = clamp_scroll_offset(
                                        self.view_pan_scroll_anchor - delta,
                                        egui::vec2(content_w, content_h),
                                        viewport.size(),
                                    );
                                } else if let Some(slot) = self.pan_drag_slot {
                                    if let (Some((ax, ay)), Some((sx, sy))) = (
                                        self.pan_anchor,
                                        self.slot_pan_sensitivity(slot, &view),
                                    ) {
                                        let tcx = pos.x - press.x;
                                        let tcy = pos.y - press.y;
                                        let px = (ax - tcx as f64 * sx).clamp(-1.0, 1.0);
                                        let py = (ay - tcy as f64 * sy).clamp(-1.0, 1.0);
                                        // Defer GPU rebuild until mouse-up.
                                        self.pan_live = Some((px, py));
                                    }
                                }
                            }
                        }
                    }
                }

                if response.drag_stopped() {
                    if let Some(slot) = self.move_drag_slot {
                        if self.drag_moved {
                            if let Some((nx, ny)) = self.move_live_canvas {
                                self.sync_undo_geometry();
                                self.slots.checkpoint();
                                if let Some(snap) = &mut self.layout_snapshot {
                                    snap.slots[slot].x = nx;
                                    snap.slots[slot].y = ny;
                                    self.preview_overlay_slots = snap.slots.clone();
                                }
                                self.preview_dirty = true;
                                self.status = format!("Slot {} moved.", slot + 1);
                            }
                        }
                        self.move_drag_slot = None;
                        self.move_anchor_canvas = None;
                        self.move_live_canvas = None;
                    } else if let Some(src) = self.swap_pickup {
                        if let Some(pos) = response.interact_pointer_pos() {
                            if let Some(tgt) = self.hit_test_slot(pos, &view) {
                                if tgt != src {
                                    self.sync_undo_geometry();
                                    self.slots.swap_slots(src, tgt);
                                    self.status =
                                        format!("Swapped slots {} and {}.", src + 1, tgt + 1);
                                }
                            }
                        }
                        self.swap_pickup = None;
                        self.swap_hover = None;
                        self.preview_dirty = true;
                    } else if self.pan_drag_slot.is_some() && self.drag_moved {
                        if let (Some(slot), Some((px, py))) = (self.pan_drag_slot, self.pan_live) {
                            self.sync_undo_geometry();
                            self.slots.checkpoint();
                            if let Some(fill) = &mut self.slots.assignments[slot] {
                                fill.pan_x = px;
                                fill.pan_y = py;
                            }
                            self.preview_dirty = true;
                        }
                    }
                    self.pan_drag_slot = None;
                    self.pan_anchor = None;
                    self.pan_live = None;
                    self.view_pan_active = false;
                    self.drag_moved = false;
                    self.press_slot = None;
                    self.stage_press = None;
                }
            });

        let manual_pan = self.view_pan_active || self.scroll_offset != scroll_before;
        self.scroll_offset = clamp_scroll_offset(
            if manual_pan {
                self.scroll_offset
            } else {
                scroll_out.state.offset
            },
            content_size,
            viewport.size(),
        );
    }

    fn draw_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Strip options");

        if self.layout_locked {
            ui.horizontal(|ui| {
                ui.colored_label(egui::Color32::from_rgb(120, 200, 140), "Layout locked");
                if ui.button("Unlock layout…").clicked() {
                    self.show_unlock_confirm = true;
                }
            });
            ui.label(
                "Card positions and underfill are frozen. Pick hero / Replace change photos only.",
            );
            ui.separator();
        }

        if self.show_unlock_confirm {
            egui::Window::new("Unlock layout?")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(
                        "Unlocking discards the frozen layout. Shuffle or New layout seed will \
                         reposition cards and replan underfill.",
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Unlock").clicked() {
                            let browse = self.pending_unlock_browse;
                            self.unlock_layout();
                            if browse {
                                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                                    self.settings.input_folder = p.display().to_string();
                                    self.sync_settings_from_ui();
                                    self.reload_folder();
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_unlock_confirm = false;
                            self.pending_unlock_browse = false;
                        }
                    });
                });
        }

        ui.label("Template");
        let mut tpl_idx = self.template_id_index();
        ui.add_enabled_ui(!self.layout_locked, |ui| {
            egui::ComboBox::from_id_salt("template")
                .selected_text(TEMPLATE_OPTIONS[tpl_idx].1)
                .show_ui(ui, |ui| {
                    for (i, (_id, label)) in TEMPLATE_OPTIONS.iter().enumerate() {
                        if ui.selectable_value(&mut tpl_idx, i, *label).clicked() {
                            let new_id = TEMPLATE_OPTIONS[i].0;
                            if new_id != self.settings.strip_template {
                                self.settings.strip_template = new_id.to_string();
                                self.template = get_template_by_id(new_id).unwrap();
                                self.sync_settings_from_ui();
                                self.refill_from_folder(true);
                                self.preview_dirty = true;
                            }
                        }
                    }
                });
        });
        if self.layout_locked {
            ui.label("Unlock layout to change template.");
        }

        ui.add_enabled_ui(!self.layout_locked, |ui| {
            if ui
                .button("Shuffle strip photos")
                .on_hover_text("Unlock layout first when locked")
                .clicked()
            {
                self.refill_from_folder(true);
            }
            if ui
                .button("New layout seed (same photos)")
                .on_hover_text("Unlock layout first when locked")
                .clicked()
            {
                self.layout_seed = random_layout_seed();
                self.preview_dirty = true;
                self.status = format!("New layout seed: {}.", self.layout_seed);
            }
        });

        ui.separator();
        let has_hero = self.flagship_slot().is_some();
        ui.add_enabled_ui(has_hero, |ui| {
            if ui.button("Pick hero photo…").clicked() {
                self.pick_hero_photo();
            }
        });
        let has_sel = self.selected_slot.is_some();
        ui.add_enabled_ui(has_sel, |ui| {
            if ui.button("Replace selected photo…").clicked() {
                self.replace_selected_photo();
            }
        });

        if self.layout_locked {
            let sel_movable = self
                .selected_slot
                .map(|s| self.slot_allows_move(s))
                .unwrap_or(false);
            ui.add_enabled_ui(sel_movable, |ui| {
                ui.label("Layer (selected slot)");
                ui.horizontal(|ui| {
                    if ui.button("Send backward").clicked() {
                        if let Some(slot) = self.selected_slot {
                            self.layer_adjust(slot, LayerAdjust::Backward);
                        }
                    }
                    if ui.button("Bring forward").clicked() {
                        if let Some(slot) = self.selected_slot {
                            self.layer_adjust(slot, LayerAdjust::Forward);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("To back").clicked() {
                        if let Some(slot) = self.selected_slot {
                            self.layer_adjust(slot, LayerAdjust::ToBack);
                        }
                    }
                    if ui.button("To front").clicked() {
                        if let Some(slot) = self.selected_slot {
                            self.layer_adjust(slot, LayerAdjust::ToFront);
                        }
                    }
                });
            });
            if sel_movable {
                ui.label("Ctrl-drag moves the card; Ctrl+Shift+drag swaps photos.");
            }
        }

        let mut smart = self.settings.strip_smart_shuffle;
        if ui
            .checkbox(&mut smart, "Smart shuffle (match photo shape to slot)")
            .changed()
        {
            self.settings.strip_smart_shuffle = smart;
            self.sync_settings_from_ui();
        }

        let mut repeats = self.settings.strip_allow_photo_repeats;
        if ui
            .checkbox(
                &mut repeats,
                "Allow repeating photos (fewer files than slots)",
            )
            .changed()
        {
            self.settings.strip_allow_photo_repeats = repeats;
            self.sync_settings_from_ui();
        }

        ui.label("Card edges");
        let mut edge_idx = self.card_edge_index();
        ui.add_enabled_ui(!self.layout_locked, |ui| {
            egui::ComboBox::from_id_salt("card_edge")
                .selected_text(CARD_EDGE_LABELS[edge_idx].0)
                .show_ui(ui, |ui| {
                    for (i, (label, val)) in CARD_EDGE_LABELS.iter().enumerate() {
                        if ui.selectable_value(&mut edge_idx, i, *label).clicked() {
                            self.settings.strip_card_edge = (*val).to_string();
                            self.sync_settings_from_ui();
                            self.preview_dirty = true;
                        }
                    }
                });
        });
        if self.layout_locked {
            ui.label("Unlock layout to change card edge mode.");
        }

        ui.horizontal(|ui| {
            ui.label("Color");
            egui::ComboBox::from_id_salt("color_preset")
                .selected_text(&self.settings.color)
                .show_ui(ui, |ui| {
                    for (label, val) in COLOR_PRESETS {
                        if ui.selectable_label(self.settings.color == *val, *label).clicked() {
                            self.settings.color = (*val).to_string();
                            self.sync_settings_from_ui();
                            self.preview_dirty = true;
                        }
                    }
                });
            if ui.text_edit_singleline(&mut self.settings.color).changed() {
                self.sync_settings_from_ui();
                self.preview_dirty = true;
            }
        });

        ui.separator();
        ui.label(&self.status);

        ui.add_space(12.0);
        ui.heading("Run");
        let need = self.required_count();
        let filled = {
            let req = self.fill_required_bitmap();
            let n = self.effective_num_slots();
            (0..n)
                .filter(|&i| req.get(i) == Some(&true) && self.slots.assignments[i].is_some())
                .count()
        };
        if self.source_paths.is_empty() {
            ui.label("Choose an input folder with photos.");
        } else if filled < need {
            ui.label(format!("Fill all {need} strip slots to run ({filled}/{need})."));
        } else {
            ui.label(format!(
                "Export: 1 wide master (all 10 slides in a row) + 10 portrait JPEGs (1080×1350). {need} slots."
            ));
        }

        ui.add_enabled_ui(!self.export_running, |ui| {
            if ui.button("Export strip").clicked() {
                self.start_export();
            }
        });
        if self.export_running {
            ui.spinner();
            ui.label("Exporting on background thread…");
        }
        let can_open = self.last_export_dir.is_some();
        ui.add_enabled_ui(can_open, |ui| {
            if ui.button("Open output folder").clicked() {
                if let Some(dir) = &self.last_export_dir {
                    reveal_folder(dir);
                }
            }
        });
    }
}

fn dx12_adapter_selector(
    adapters: &[wgpu::Adapter],
    _compatible_surface: Option<&wgpu::Surface<'_>>,
) -> std::result::Result<wgpu::Adapter, String> {
    fn score(adapter: &wgpu::Adapter) -> u8 {
        match adapter.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu => 4,
            wgpu::DeviceType::IntegratedGpu => 3,
            wgpu::DeviceType::VirtualGpu => 2,
            wgpu::DeviceType::Cpu => 1,
            wgpu::DeviceType::Other => 0,
        }
    }
    adapters
        .iter()
        .max_by_key(|a| score(a))
        .cloned()
        .or_else(|| adapters.first().cloned())
        .ok_or_else(|| "no DX12 or WARP adapter".to_string())
}

impl eframe::App for CarouselApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.ensure_compositor(frame);
        self.poll_export();
        self.poll_preview(ctx);
        self.rebuild_preview(ctx);

        if self.preview_building || (self.preview_dirty && !self.preview_building) {
            ctx.request_repaint();
        }

        ctx.input_mut(|i| {
            if i.key_pressed(egui::Key::Z) && i.modifiers.ctrl && !i.modifiers.shift {
                let geo = self.locked_geometry_snapshot();
                if let Some(restore) = self.slots.undo(geo) {
                    self.apply_geometry_restore(restore.slot_geometry);
                    self.preview_dirty = true;
                    self.status = "Undo.".into();
                }
            }
            if (i.key_pressed(egui::Key::Y) && i.modifiers.ctrl)
                || (i.key_pressed(egui::Key::Z) && i.modifiers.ctrl && i.modifiers.shift)
            {
                let geo = self.locked_geometry_snapshot();
                if let Some(restore) = self.slots.redo(geo) {
                    self.apply_geometry_restore(restore.slot_geometry);
                    self.preview_dirty = true;
                    self.status = "Redo.".into();
                }
            }
        });

        egui::TopBottomPanel::top("top").show(ctx, |ui| self.draw_top_bar(ui));

        egui::SidePanel::right("controls")
            .resizable(true)
            .default_width(260.0)
            .show(ctx, |ui| self.draw_controls(ui));

        egui::CentralPanel::default().show(ctx, |ui| self.draw_preview(ui, ctx));
    }

    fn on_exit(&mut self) {
        self.sync_settings_from_ui();
    }
}

pub fn run_gui() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("Carousel Canvas"),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(egui_wgpu::WgpuSetupCreateNew {
                instance_descriptor: wgpu::InstanceDescriptor {
                    backends: wgpu::Backends::DX12,
                    ..Default::default()
                },
                native_adapter_selector: Some(Arc::new(dx12_adapter_selector)),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "Carousel Canvas",
        options,
        Box::new(|_cc| Ok(Box::new(CarouselApp::new()))),
    )
}

#[cfg(test)]
mod tests {
    use core::TEMPLATE_IDS;
    use super::*;

    #[test]
    fn template_ids_match_core() {
        for (id, _) in TEMPLATE_OPTIONS {
            assert!(TEMPLATE_IDS.contains(id));
        }
    }
}
