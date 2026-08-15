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
    scan_image_folder, SlotFill, StripTemplate,
};
use eframe::egui;
use render::{
    compose_preview_gpu, parse_color_rgb, prepare_preview_phase1, prepare_preview_phase2,
    preview_ready_without_underfill, preview_required_flatten, run_strip_export, CardEdge,
    Compositor, PreviewGpuReady, PreviewParams, PreviewPhase1, StripExportParams,
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

struct ThumbEntry {
    path: PathBuf,
    texture: Option<egui::TextureHandle>,
}

enum ExportMsg {
    Done(Result<render::ExportResult, String>),
}

enum PreviewMsg {
    Phase1(Result<PreviewPhase1, String>),
    Phase2(Result<PreviewGpuReady, String>),
}

enum PreviewBuildUi {
    Ok(render::PreviewBuildResult),
    Err(String),
}

struct StripView {
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
    thumbs: Vec<ThumbEntry>,
    thumb_paths_dirty: bool,

    preview_texture: Option<egui::TextureHandle>,
    preview_overlay_slots: Vec<core::StripSlotDef>,
    preview_dirty: bool,
    preview_building: bool,
    strip_view: Option<StripView>,
    preview_error: Option<String>,

    compositor: Option<Compositor>,
    status: String,
    hero_only: bool,
    scroll_offset_x: f32,

    drag_thumb: Option<PathBuf>,
    stage_down_slot: Option<usize>,
    stage_press: Option<egui::Pos2>,
    pan_drag_slot: Option<usize>,
    pan_anchor: Option<(f64, f64)>,
    pan_live: Option<(f64, f64)>,
    pan_moved: bool,
    swap_pickup: Option<usize>,
    swap_hover: Option<usize>,

    export_rx: Option<Receiver<ExportMsg>>,
    export_running: bool,
    last_export_dir: Option<PathBuf>,

    preview_rx: Option<Receiver<(u64, PreviewMsg)>>,
    preview_generation: u64,
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
            thumbs: Vec::new(),
            thumb_paths_dirty: true,
            preview_texture: None,
            preview_overlay_slots: Vec::new(),
            preview_dirty: true,
            preview_building: false,
            strip_view: None,
            preview_error: None,
            compositor: None,
            status: "Ready.".into(),
            hero_only: false,
            scroll_offset_x: 0.0,
            drag_thumb: None,
            stage_down_slot: None,
            stage_press: None,
            pan_drag_slot: None,
            pan_anchor: None,
            pan_live: None,
            pan_moved: false,
            swap_pickup: None,
            swap_hover: None,
            export_rx: None,
            export_running: false,
            last_export_dir: None,
            preview_rx: None,
            preview_generation: 0,
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

    fn flagship_slot(&self) -> Option<usize> {
        self.template.layout_flagship_slot_index
    }

    fn effective_num_slots(&self) -> usize {
        self.template
            .strip_effective_num_slots(Some(self.layout_seed))
    }

    fn required_count(&self) -> usize {
        self.template
            .strip_image_slot_count(Some(self.layout_seed))
    }

    fn reload_folder(&mut self) {
        let folder = PathBuf::from(&self.settings.input_folder);
        if !folder.is_dir() {
            self.source_paths.clear();
            self.thumb_paths_dirty = true;
            return;
        }
        match scan_image_folder(&folder) {
            Ok(paths) => {
                self.source_paths = paths;
                self.thumb_paths_dirty = true;
                self.refill_from_folder(true);
            }
            Err(e) => self.status = format!("Folder scan failed: {e}"),
        }
    }

    fn refill_from_folder(&mut self, randomize: bool) {
        if self.source_paths.is_empty() {
            self.slots.reset();
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
        let req = self
            .template
            .strip_effective_fill_required(Some(self.layout_seed));
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
            self.preview_texture = None;
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
        let params = PreviewParams {
            template: self.template.clone(),
            fills: self.fills_for_preview(),
            source_paths: self.source_paths.clone(),
            layout_seed: self.layout_seed,
            card_edge: self.card_edge(),
            border_rgb: self.border_rgb(),
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
                self.preview_texture = Some(ctx.load_texture(
                    "mural_preview",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
                self.preview_overlay_slots = built.overlay_slots;
                self.preview_error = None;
                self.strip_view = Some(StripView {
                    canvas_w: built.canvas_width as f32,
                    canvas_h: built.canvas_height as f32,
                    preview_w: 0.0,
                    preview_h: 0.0,
                    scale: 0.0,
                    offset_x: 0.0,
                    offset_y: 0.0,
                });
            }
            PreviewBuildUi::Err(e) => {
                self.preview_texture = None;
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
                let Some(compositor) = self.compositor.as_ref() else {
                    self.apply_preview_result(ctx, PreviewBuildUi::Err("no compositor".into()));
                    return;
                };
                if !phase1.underfill_enabled {
                    let ready = preview_ready_without_underfill(phase1);
                    match compose_preview_gpu(compositor, &ready) {
                        Ok(built) => self.apply_preview_result(ctx, PreviewBuildUi::Ok(built)),
                        Err(e) => self.apply_preview_result(ctx, PreviewBuildUi::Err(e.to_string())),
                    }
                    return;
                }
                let flat = match preview_required_flatten(compositor, &phase1) {
                    Ok(f) => f,
                    Err(e) => {
                        self.apply_preview_result(ctx, PreviewBuildUi::Err(e.to_string()));
                        return;
                    }
                };
                let (tx, rx2) = mpsc::channel();
                // Replace receiver for phase2.
                self.preview_rx = Some(rx2);
                let gen = self.preview_generation;
                thread::spawn(move || {
                    let result = prepare_preview_phase2(phase1, flat).map_err(|e| e.to_string());
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
        }
    }

    fn load_thumbs(&mut self, ctx: &egui::Context) {
        if self.thumb_paths_dirty {
            self.thumbs.clear();
            for path in self.source_paths.iter().take(120) {
                self.thumbs.push(ThumbEntry {
                    path: path.clone(),
                    texture: None,
                });
            }
            self.thumb_paths_dirty = false;
        }
        // Decode a few thumbnails per frame so the UI stays responsive.
        let mut budget = 6usize;
        for entry in &mut self.thumbs {
            if budget == 0 {
                break;
            }
            if entry.texture.is_none() {
                entry.texture = load_thumb_texture(ctx, &entry.path);
                budget -= 1;
            }
        }
        if self.thumbs.iter().any(|t| t.texture.is_none()) {
            ctx.request_repaint();
        }
    }

    fn hit_test_slot(&self, pos: egui::Pos2, view: &StripView) -> Option<usize> {
        let cx = (pos.x - view.offset_x) / view.scale;
        let cy = (pos.y - view.offset_y) / view.scale;
        let mut order: Vec<usize> = (0..self.preview_overlay_slots.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(self.preview_overlay_slots[i].z_index));
        for i in order {
            let s = &self.preview_overlay_slots[i];
            if cx >= s.x as f32
                && cx < (s.x + s.w) as f32
                && cy >= s.y as f32
                && cy < (s.y + s.h) as f32
            {
                return Some(i);
            }
        }
        None
    }

    fn assign_to_slot(&mut self, slot: usize, path: PathBuf) {
        if self.hero_only {
            if let Some(hi) = self.flagship_slot() {
                if slot != hi {
                    self.status =
                        "Hero-only mode: drop on hero or use thumbnails.".into();
                    return;
                }
            }
        }
        self.slots.assign(slot, path);
        self.preview_dirty = true;
        self.status = format!("Slot {} updated.", slot + 1);
    }

    fn assign_thumb_click(&mut self, path: PathBuf) {
        if self.hero_only {
            if let Some(hi) = self.flagship_slot() {
                self.assign_to_slot(hi, path);
                self.status = "Hero photo updated.".into();
            } else {
                self.status = "No hero slot on this template.".into();
            }
            return;
        }
        let req = self
            .template
            .strip_effective_fill_required(Some(self.layout_seed));
        let n = self.effective_num_slots();
        for i in 0..n {
            if req.get(i) == Some(&true) && self.slots.assignments[i].is_none() {
                self.assign_to_slot(i, path);
                return;
            }
        }
        self.status = "All slots full — right-click a slot to clear.".into();
    }

    fn slot_pan_sensitivity(&self, slot: usize, view: &StripView) -> Option<(f64, f64)> {
        let s = self.preview_overlay_slots.get(slot)?;
        let tws = (s.w as f32 * view.scale).max(1.0);
        let ths = (s.h as f32 * view.scale).max(1.0);
        Some((2.0 / tws as f64, 2.0 / ths as f64))
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
                "Fill all {} strip slots (use Shuffle or drag thumbnails).",
                self.required_count()
            ));
        }
        let n = self.effective_num_slots();
        let fills: Vec<Option<SlotFill>> = self.slots.assignments[..n].to_vec();
        if self.settings.strip_allow_photo_repeats {
            return Ok(());
        }
        let need = self.required_count();
        let req = self
            .template
            .strip_effective_fill_required(Some(self.layout_seed));
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
        let params = StripExportParams {
            template: self.template.clone(),
            fills,
            slot_fills: Some(slot_fills),
            source_paths: self.source_paths.clone(),
            layout_seed: self.layout_seed,
            card_edge: self.card_edge(),
            border_rgb: self.border_rgb(),
            no_layout_retry: false,
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
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
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

    fn draw_thumbs(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Photos");
        if ui.button("Refresh photos").clicked() {
            self.reload_folder();
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            let thumb_count = self.thumbs.len();
            egui::Grid::new("thumb_grid")
                .num_columns(2)
                .spacing([4.0, 4.0])
                .show(ui, |ui| {
                    let mut click_path: Option<PathBuf> = None;
                    for idx in 0..thumb_count {
                        let path = self.thumbs[idx].path.clone();
                        let response = if let Some(tex) = &self.thumbs[idx].texture {
                            let size = egui::vec2(96.0, 72.0);
                            ui.add(
                                egui::Image::new(egui::load::SizedTexture::new(tex.id(), size))
                                    .sense(egui::Sense::click_and_drag()),
                            )
                        } else {
                            ui.add(
                                egui::Button::new(
                                    path.file_name()
                                        .map(|s| s.to_string_lossy().to_string())
                                        .unwrap_or_else(|| "?".into()),
                                )
                                .min_size(egui::vec2(96.0, 72.0)),
                            )
                        };
                        if response.drag_started() {
                            self.drag_thumb = Some(path.clone());
                        }
                        if response.clicked() && self.drag_thumb.is_none() {
                            click_path = Some(path);
                        }
                        if idx % 2 == 1 {
                            ui.end_row();
                        }
                    }
                    if let Some(p) = click_path {
                        self.assign_thumb_click(p);
                    }
                });
        });
        if let Some(drag_path) = self.drag_thumb.clone() {
            if ctx.input(|i| !i.pointer.primary_down()) {
                if let Some(pointer) = ctx.input(|i| i.pointer.hover_pos()) {
                    if let Some(ref view) = self.strip_view {
                        if let Some(slot) = self.hit_test_slot(pointer, view) {
                            if self.hero_only {
                                if let Some(hi) = self.flagship_slot() {
                                    self.assign_to_slot(hi, drag_path.clone());
                                }
                            } else {
                                self.assign_to_slot(slot, drag_path.clone());
                            }
                        }
                    }
                }
                self.drag_thumb = None;
            }
        }
    }

    fn draw_preview(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Mural preview");
        ui.label("Drag thumbs to slots; Shift+wheel scrolls horizontally; double-click flip; Ctrl+drag swap; right-click clear.");

        let avail = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());

        if ctx.input(|i| i.modifiers.shift) {
            let scroll = ctx.input(|i| i.smooth_scroll_delta);
            if scroll.y.abs() > 0.1 {
                self.scroll_offset_x = (self.scroll_offset_x - scroll.y).max(0.0);
            }
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(20, 20, 20));

        if !self.preview_ready() {
            let need = self.required_count();
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("Fill all {need} photo slots to preview."),
                egui::FontId::proportional(16.0),
                egui::Color32::GRAY,
            );
            return;
        }

        if let Some(err) = &self.preview_error {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                err,
                egui::FontId::proportional(14.0),
                egui::Color32::LIGHT_RED,
            );
            return;
        }

        if self.preview_building {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Building preview…",
                egui::FontId::proportional(16.0),
                egui::Color32::GRAY,
            );
            return;
        }

        let Some(tex) = &self.preview_texture else {
            return;
        };
        let tex_size = tex.size_vec2();
        let scale0 = (rect.height() * 0.92) / tex_size.y;
        let pw = (tex_size.x * scale0).max(1.0);
        let ph = (tex_size.y * scale0).max(1.0);
        let ix = rect.min.x + (rect.width() - pw) * 0.5;
        let iy = rect.min.y + (rect.height() - ph) * 0.5;
        let max_scroll = (pw - rect.width()).max(0.0);
        self.scroll_offset_x = self.scroll_offset_x.min(max_scroll);
        let img_rect = egui::Rect::from_min_size(
            egui::pos2(ix - self.scroll_offset_x, iy),
            egui::vec2(pw, ph),
        );
        painter.image(tex.id(), img_rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);

        if let Some(ref mut view) = self.strip_view {
            view.scale = pw / view.canvas_w;
            view.offset_x = img_rect.min.x;
            view.offset_y = img_rect.min.y;
            view.preview_w = pw;
            view.preview_h = ph;
        }

        let view = self.strip_view.clone();
        if let Some(view) = view {
            if let Some(pickup) = self.swap_pickup {
                for (si, slot) in self.preview_overlay_slots.iter().enumerate() {
                    let x0 = view.offset_x + slot.x as f32 * view.scale;
                    let y0 = view.offset_y + slot.y as f32 * view.scale;
                    let x1 = view.offset_x + (slot.x + slot.w) as f32 * view.scale;
                    let y1 = view.offset_y + (slot.y + slot.h) as f32 * view.scale;
                    let r = egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1));
                    if si == pickup {
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
                            egui::Stroke::new(3.0_f32, egui::Color32::from_rgb(255, 170, 51)),
                            egui::StrokeKind::Outside,
                        );
                    }
                }
            }

            let ctrl = ctx.input(|i| i.modifiers.ctrl);
            if response.secondary_clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    if let Some(slot) = self.hit_test_slot(pos, &view) {
                        self.slots.clear_slot(slot);
                        self.preview_dirty = true;
                        self.status = format!("Slot {} cleared.", slot + 1);
                    }
                }
            }

            if response.double_clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    if let Some(slot) = self.hit_test_slot(pos, &view) {
                        if self.slots.assignments[slot].is_some() {
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
                    if ctrl {
                        if let Some(slot) = self.hit_test_slot(pos, &view) {
                            if self.slots.assignments[slot].is_some() {
                                self.swap_pickup = Some(slot);
                                self.status = format!("Slot {} picked — drop on another slot.", slot + 1);
                            }
                        }
                    } else if let Some(slot) = self.hit_test_slot(pos, &view) {
                        if self.slots.assignments[slot].is_some() {
                            self.stage_down_slot = Some(slot);
                            self.stage_press = Some(pos);
                            self.pan_moved = false;
                        }
                    }
                }
            }

            if response.dragged() {
                if let Some(src) = self.swap_pickup {
                    if let Some(pos) = response.interact_pointer_pos() {
                        self.swap_hover = self.hit_test_slot(pos, &view);
                        self.preview_dirty = true;
                    }
                    let _ = src;
                } else if let (Some(slot), Some(press)) = (self.stage_down_slot, self.stage_press) {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let dx = pos.x - press.x;
                        let dy = pos.y - press.y;
                        if !self.pan_moved && (dx * dx + dy * dy) < 36.0 {
                            return;
                        }
                        if !self.pan_moved {
                            self.pan_moved = true;
                            self.pan_drag_slot = Some(slot);
                            if let Some(fill) = &self.slots.assignments[slot] {
                                self.pan_anchor = Some((fill.pan_x, fill.pan_y));
                            }
                        }
                        if let (Some((ax, ay)), Some((sx, sy))) =
                            (self.pan_anchor, self.slot_pan_sensitivity(slot, &view))
                        {
                            let tcx = pos.x - press.x;
                            let tcy = pos.y - press.y;
                            let px = (ax - tcx as f64 * sx).clamp(-1.0, 1.0);
                            let py = (ay - tcy as f64 * sy).clamp(-1.0, 1.0);
                            self.pan_live = Some((px, py));
                            self.preview_dirty = true;
                        }
                    }
                }
            }

            if response.drag_stopped() {
                if let Some(src) = self.swap_pickup {
                    if let Some(pos) = response.interact_pointer_pos() {
                        if let Some(tgt) = self.hit_test_slot(pos, &view) {
                            if tgt != src {
                                self.slots.swap_slots(src, tgt);
                                self.status = format!("Swapped slots {} and {}.", src + 1, tgt + 1);
                            }
                        }
                    }
                    self.swap_pickup = None;
                    self.swap_hover = None;
                    self.preview_dirty = true;
                } else if self.pan_moved {
                    if let (Some(slot), Some((px, py))) = (self.pan_drag_slot, self.pan_live) {
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
                self.pan_moved = false;
                self.stage_down_slot = None;
                self.stage_press = None;
            }
        }
    }

    fn draw_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Strip options");

        ui.label("Template");
        let mut tpl_idx = self.template_id_index();
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
                            self.thumb_paths_dirty = true;
                            self.preview_dirty = true;
                        }
                    }
                }
            });

        if ui.button("Shuffle strip photos").clicked() {
            self.refill_from_folder(true);
        }
        if ui.button("New layout seed (same photos)").clicked() {
            self.layout_seed = random_layout_seed();
            self.preview_dirty = true;
            self.status = format!("New layout seed: {}.", self.layout_seed);
        }

        let mut hero = self.hero_only;
        if ui
            .checkbox(&mut hero, "Replace hero only (thumb → hero)")
            .changed()
        {
            self.hero_only = hero;
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
            let req = self
                .template
                .strip_effective_fill_required(Some(self.layout_seed));
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
                "Run writes 1 wide master + 10 slice JPEGs (1080×1350); {need} slots."
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

impl Clone for StripView {
    fn clone(&self) -> Self {
        Self {
            scale: self.scale,
            offset_x: self.offset_x,
            offset_y: self.offset_y,
            preview_w: self.preview_w,
            preview_h: self.preview_h,
            canvas_w: self.canvas_w,
            canvas_h: self.canvas_h,
        }
    }
}

fn load_thumb_texture(ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
    let img = image::open(path).ok()?;
    let thumb = img.thumbnail(96, 72);
    let rgba = thumb.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let pixels: Vec<egui::Color32> = rgba
        .pixels()
        .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
        .collect();
    Some(ctx.load_texture(
        format!("thumb_{}", path.display()),
        egui::ColorImage { size, pixels },
        egui::TextureOptions::LINEAR,
    ))
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
        self.load_thumbs(ctx);
        self.poll_preview(ctx);
        self.rebuild_preview(ctx);

        if self.preview_building || (self.preview_dirty && !self.preview_building) {
            ctx.request_repaint();
        }

        ctx.input_mut(|i| {
            if i.key_pressed(egui::Key::Z) && i.modifiers.ctrl && !i.modifiers.shift {
                if self.slots.undo() {
                    self.preview_dirty = true;
                    self.status = "Undo.".into();
                }
            }
            if (i.key_pressed(egui::Key::Y) && i.modifiers.ctrl)
                || (i.key_pressed(egui::Key::Z) && i.modifiers.ctrl && i.modifiers.shift)
            {
                if self.slots.redo() {
                    self.preview_dirty = true;
                    self.status = "Redo.".into();
                }
            }
        });

        egui::TopBottomPanel::top("top").show(ctx, |ui| self.draw_top_bar(ui));

        egui::SidePanel::left("thumbs")
            .resizable(true)
            .default_width(220.0)
            .show(ctx, |ui| self.draw_thumbs(ui, ctx));

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
