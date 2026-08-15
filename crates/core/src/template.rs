use crate::mosaic::build_seamless_mosaic_v1_slots;
use crate::slot::StripSlotDef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutPlacer {
    Jitter,
    OrganicPolaroid,
}

#[derive(Debug, Clone)]
pub struct StripTemplate {
    pub id: &'static str,
    pub canvas_width: i32,
    pub canvas_height: i32,
    pub slice_width: i32,
    pub slice_height: i32,
    pub slice_count: usize,
    pub overlap_px: i32,
    pub background: &'static str,
    pub slots: Vec<StripSlotDef>,
    pub slot_fill_required: Option<Vec<bool>>,
    pub layout_jitter_px: i32,
    pub layout_cover_slot_index: Option<usize>,
    pub layout_cover_max_jitter: i32,
    pub layout_flagship_slot_index: Option<usize>,
    pub layout_flagship_max_jitter: i32,
    pub layout_flagship_rim_slot_indices: Vec<usize>,
    pub background_underfill_layers: i32,
    pub background_underfill_boost_layers: i32,
    pub background_underfill_repeat_layers: i32,
    pub background_tail_underfill_layers: i32,
    pub background_tail_slice_count: i32,
    pub gap_fill_max_layers: i32,
    pub gap_fill_beige_tolerance: i32,
    pub gap_fill_stop_ratio: f64,
    pub procedural_background: Option<&'static str>,
    pub layout_placer: LayoutPlacer,
}

impl StripTemplate {
    pub fn num_slots(&self) -> usize {
        self.slots.len()
    }

    pub fn effective_slot_fill_required(&self) -> Vec<bool> {
        if let Some(ref req) = self.slot_fill_required {
            assert_eq!(
                req.len(),
                self.slots.len(),
                "slot_fill_required length must match slots"
            );
            return req.clone();
        }
        vec![true; self.slots.len()]
    }

    pub fn strip_effective_fill_required(&self, layout_seed: Option<i64>) -> Vec<bool> {
        if self.id == "strip_seamless_mosaic_v1" {
            let eff = layout_seed.unwrap_or(0);
            let n = build_seamless_mosaic_v1_slots(eff).len();
            return vec![true; n];
        }
        self.effective_slot_fill_required()
    }

    pub fn strip_effective_num_slots(&self, layout_seed: Option<i64>) -> usize {
        self.strip_effective_fill_required(layout_seed).len()
    }

    pub fn strip_image_slot_count(&self, layout_seed: Option<i64>) -> usize {
        if self.id == "strip_seamless_mosaic_v1" && layout_seed.is_none() {
            return crate::mosaic::mosaic_v1_max_slot_count();
        }
        self.strip_effective_fill_required(layout_seed)
            .iter()
            .filter(|r| **r)
            .count()
    }

    pub fn background_underfill_count(&self) -> i32 {
        self.background_underfill_layers.max(0)
            + self.background_underfill_boost_layers.max(0)
            + self.background_underfill_repeat_layers.max(0)
            + self.background_tail_underfill_layers.max(0)
    }

    pub fn resolved_slots(&self, layout_seed: Option<i64>) -> Vec<StripSlotDef> {
        let eff = layout_seed.unwrap_or(0);
        if self.id == "strip_seamless_mosaic_v1" {
            return build_seamless_mosaic_v1_slots(eff);
        }
        if self.layout_placer == LayoutPlacer::OrganicPolaroid {
            return crate::layout_polaroid::resolve_organic_polaroid_slots(
                &self.slots,
                self.canvas_width,
                self.canvas_height,
                self.slice_width,
                self.slice_count,
                eff,
            );
        }
        if self.layout_jitter_px <= 0 {
            return self.slots.clone();
        }
        crate::layout_jitter::jitter_strip_slots(
            &self.slots,
            self.layout_jitter_px,
            eff,
            self.canvas_width,
            self.canvas_height,
            self.layout_cover_slot_index,
            self.layout_cover_max_jitter,
            self.layout_flagship_slot_index,
            self.layout_flagship_max_jitter,
            &self.layout_flagship_rim_slot_indices,
            70.0,
            48,
            200,
            220,
            520,
        )
    }
}

fn mural_v2_main_board() -> Vec<StripSlotDef> {
    // Generated once from Python random.Random(881_122) — see template_data._gen_mural_v2_main_board.
    const SLOTS: &[(i32, i32, i32, i32, f64, i32)] = &[
        (-1, -48, 1045, 851, -1.855059, 0),
        (-116, 75, 1042, 838, -1.679472, 1),
        (-120, 607, 1317, 582, 0.824346, 2),
        (-120, 710, 1089, 749, 0.263235, 3),
        (1218, -70, 919, 718, -1.237031, 4),
        (866, 228, 1564, 639, 0.336248, 5),
        (1007, 341, 1046, 849, 0.730441, 6),
        (739, 614, 1893, 876, 0.777423, 7),
        (2097, -70, 945, 641, -1.972955, 8),
        (1733, 120, 1858, 755, 1.451581, 9),
        (1478, 577, 1981, 626, 1.742474, 10),
        (1837, 765, 1606, 648, -0.911167, 11),
        (3106, -70, 1015, 788, 1.951266, 12),
        (2728, 230, 1848, 562, 2.268497, 13),
        (2858, 289, 1644, 861, -2.311503, 14),
        (3109, 821, 1132, 613, 1.163323, 15),
        (4153, 29, 1484, 629, 0.715045, 16),
        (4065, 286, 1933, 605, 1.789640, 17),
        (4102, 553, 1478, 651, -1.769574, 18),
        (4385, 685, 1221, 640, -0.295081, 19),
        (5145, -70, 1833, 791, 0.797578, 20),
        (5464, 221, 1336, 643, 1.123320, 21),
        (5548, 559, 860, 574, 2.012122, 22),
        (5392, 852, 1398, 638, -0.581624, 23),
        (6118, -70, 1747, 808, -1.439917, 24),
        (6240, 140, 1941, 691, 0.881199, 25),
        (6356, 314, 1283, 829, 0.312081, 26),
        (6237, 623, 1827, 836, -1.350498, 27),
        (7331, -70, 1242, 671, 0.549125, 28),
        (7195, 317, 1973, 573, 0.793912, 29),
        (7358, 383, 1834, 721, -2.052721, 30),
        (7371, 657, 1199, 826, 0.782930, 31),
    ];
    SLOTS
        .iter()
        .map(|(x, y, w, h, rot, z)| {
            StripSlotDef::new(*x, *y, *w, *h)
                .with_rotation(*rot)
                .with_z(*z)
        })
        .collect()
}

fn mural_v2_slots() -> Vec<StripSlotDef> {
    let mut slots = mural_v2_main_board();
    slots.push(
        StripSlotDef::new(0, 0, 500, 400)
            .with_rotation(2.05)
            .with_z(85),
    );
    slots.push(
        StripSlotDef::new(0, 0, 480, 380)
            .with_rotation(-1.95)
            .with_z(86),
    );
    slots.push(
        StripSlotDef::new(52, 78, 976, 1194)
            .with_rotation(-1.85)
            .with_z(90)
            .with_prefer_portrait(),
    );
    slots
}

fn strip_10col_slots() -> Vec<StripSlotDef> {
    (0..10)
        .map(|i| {
            StripSlotDef::new(i * 1080, 0, 1080, 1350).with_z(i as i32)
        })
        .collect()
}

fn mural_v1_slots() -> Vec<StripSlotDef> {
    vec![
        StripSlotDef::new(0, 480, 10800, 870).with_z(0),
        StripSlotDef::new(-60, 40, 1280, 1180)
            .with_rotation(-2.2)
            .with_z(1),
        StripSlotDef::new(1180, 90, 1020, 1040)
            .with_rotation(3.0)
            .with_z(2),
        StripSlotDef::new(2480, 30, 1180, 1180)
            .with_rotation(-3.5)
            .with_z(3),
        StripSlotDef::new(3780, 140, 980, 1020)
            .with_rotation(2.5)
            .with_z(4),
        StripSlotDef::new(4980, 20, 1220, 1220)
            .with_rotation(-4.0)
            .with_z(5),
        StripSlotDef::new(6320, 80, 1060, 1080)
            .with_rotation(3.2)
            .with_z(6),
        StripSlotDef::new(7720, 50, 1240, 1200)
            .with_rotation(-2.8)
            .with_z(7),
    ]
}

fn seamless_v1_slots() -> Vec<StripSlotDef> {
    vec![
        StripSlotDef::new(0, 410, 10800, 940).with_z(0),
        StripSlotDef::new(-35, 72, 1320, 1010)
            .with_rotation(-0.52)
            .with_z(1),
        StripSlotDef::new(920, 38, 1720, 1040)
            .with_rotation(0.42)
            .with_z(2),
        StripSlotDef::new(2480, 88, 1460, 990)
            .with_rotation(-0.38)
            .with_z(3),
        StripSlotDef::new(3780, 22, 1980, 1088)
            .with_rotation(0.48)
            .with_z(4),
        StripSlotDef::new(5420, 62, 1620, 1035)
            .with_rotation(-0.45)
            .with_z(5),
        StripSlotDef::new(6780, 32, 1860, 1075)
            .with_rotation(0.4)
            .with_z(6),
        StripSlotDef::new(8280, 78, 1520, 1005)
            .with_rotation(-0.42)
            .with_z(7),
        StripSlotDef::new(9060, 28, 1740, 1032)
            .with_rotation(0.36)
            .with_z(8),
    ]
}

fn polaroid_table_slots() -> Vec<StripSlotDef> {
    const CARD_W: i32 = 740;
    const CARD_H: i32 = 920;
    let z_order: [i32; 24] = [
        48, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25,
    ];
    z_order
        .iter()
        .enumerate()
        .map(|(i, &z)| {
            let mut s = StripSlotDef::new(0, 0, CARD_W, CARD_H)
                .with_z(z)
                .with_polaroid();
            if i == 0 {
                s = s.with_prefer_portrait();
            }
            s
        })
        .collect()
}

fn base_geometry() -> (i32, i32, i32, i32, usize, i32) {
    (10800, 1350, 1080, 1350, 10, 0)
}

pub fn template_strip_10col() -> StripTemplate {
    let (cw, ch, sw, sh, sc, ov) = base_geometry();
    StripTemplate {
        id: "strip_10col",
        canvas_width: cw,
        canvas_height: ch,
        slice_width: sw,
        slice_height: sh,
        slice_count: sc,
        overlap_px: ov,
        background: "white",
        slots: strip_10col_slots(),
        slot_fill_required: None,
        layout_jitter_px: 0,
        layout_cover_slot_index: None,
        layout_cover_max_jitter: 14,
        layout_flagship_slot_index: None,
        layout_flagship_max_jitter: 18,
        layout_flagship_rim_slot_indices: vec![],
        background_underfill_layers: 0,
        background_underfill_boost_layers: 0,
        background_underfill_repeat_layers: 0,
        background_tail_underfill_layers: 0,
        background_tail_slice_count: 2,
        gap_fill_max_layers: 0,
        gap_fill_beige_tolerance: 48,
        gap_fill_stop_ratio: 0.004,
        procedural_background: None,
        layout_placer: LayoutPlacer::Jitter,
    }
}

pub fn template_strip_mural_v1() -> StripTemplate {
    let (cw, ch, sw, sh, sc, ov) = base_geometry();
    StripTemplate {
        id: "strip_mural_v1",
        canvas_width: cw,
        canvas_height: ch,
        slice_width: sw,
        slice_height: sh,
        slice_count: sc,
        overlap_px: ov,
        background: "#ece8e3",
        slots: mural_v1_slots(),
        slot_fill_required: None,
        layout_jitter_px: 0,
        layout_cover_slot_index: None,
        layout_cover_max_jitter: 14,
        layout_flagship_slot_index: None,
        layout_flagship_max_jitter: 18,
        layout_flagship_rim_slot_indices: vec![],
        background_underfill_layers: 0,
        background_underfill_boost_layers: 0,
        background_underfill_repeat_layers: 0,
        background_tail_underfill_layers: 0,
        background_tail_slice_count: 2,
        gap_fill_max_layers: 0,
        gap_fill_beige_tolerance: 48,
        gap_fill_stop_ratio: 0.004,
        procedural_background: None,
        layout_placer: LayoutPlacer::Jitter,
    }
}

pub fn template_strip_mural_v2() -> StripTemplate {
    let (cw, ch, sw, sh, sc, ov) = base_geometry();
    StripTemplate {
        id: "strip_mural_v2",
        canvas_width: cw,
        canvas_height: ch,
        slice_width: sw,
        slice_height: sh,
        slice_count: sc,
        overlap_px: ov,
        background: "#ece8e3",
        slots: mural_v2_slots(),
        slot_fill_required: None,
        layout_jitter_px: 56,
        layout_cover_slot_index: None,
        layout_cover_max_jitter: 14,
        layout_flagship_slot_index: Some(34),
        layout_flagship_max_jitter: 12,
        layout_flagship_rim_slot_indices: vec![32, 33],
        background_underfill_layers: 10,
        background_underfill_boost_layers: 5,
        background_underfill_repeat_layers: 9,
        background_tail_underfill_layers: 18,
        background_tail_slice_count: 1,
        gap_fill_max_layers: 0,
        gap_fill_beige_tolerance: 48,
        gap_fill_stop_ratio: 0.004,
        procedural_background: None,
        layout_placer: LayoutPlacer::Jitter,
    }
}

pub fn template_strip_seamless_v1() -> StripTemplate {
    let (cw, ch, sw, sh, sc, ov) = base_geometry();
    StripTemplate {
        id: "strip_seamless_v1",
        canvas_width: cw,
        canvas_height: ch,
        slice_width: sw,
        slice_height: sh,
        slice_count: sc,
        overlap_px: ov,
        background: "#101010",
        slots: seamless_v1_slots(),
        slot_fill_required: None,
        layout_jitter_px: 0,
        layout_cover_slot_index: None,
        layout_cover_max_jitter: 14,
        layout_flagship_slot_index: None,
        layout_flagship_max_jitter: 18,
        layout_flagship_rim_slot_indices: vec![],
        background_underfill_layers: 0,
        background_underfill_boost_layers: 0,
        background_underfill_repeat_layers: 0,
        background_tail_underfill_layers: 0,
        background_tail_slice_count: 2,
        gap_fill_max_layers: 0,
        gap_fill_beige_tolerance: 48,
        gap_fill_stop_ratio: 0.004,
        procedural_background: None,
        layout_placer: LayoutPlacer::Jitter,
    }
}

pub fn template_strip_seamless_mosaic_v1() -> StripTemplate {
    let (cw, ch, sw, sh, sc, ov) = base_geometry();
    StripTemplate {
        id: "strip_seamless_mosaic_v1",
        canvas_width: cw,
        canvas_height: ch,
        slice_width: sw,
        slice_height: sh,
        slice_count: sc,
        overlap_px: ov,
        background: "#121212",
        slots: build_seamless_mosaic_v1_slots(0),
        slot_fill_required: None,
        layout_jitter_px: 0,
        layout_cover_slot_index: None,
        layout_cover_max_jitter: 14,
        layout_flagship_slot_index: None,
        layout_flagship_max_jitter: 18,
        layout_flagship_rim_slot_indices: vec![],
        background_underfill_layers: 0,
        background_underfill_boost_layers: 0,
        background_underfill_repeat_layers: 0,
        background_tail_underfill_layers: 0,
        background_tail_slice_count: 2,
        gap_fill_max_layers: 0,
        gap_fill_beige_tolerance: 48,
        gap_fill_stop_ratio: 0.004,
        procedural_background: None,
        layout_placer: LayoutPlacer::Jitter,
    }
}

pub fn template_strip_polaroid_table_v1() -> StripTemplate {
    let (cw, ch, sw, sh, sc, ov) = base_geometry();
    let mut fill = vec![true; 20];
    fill.extend(std::iter::repeat_n(false, 4));
    StripTemplate {
        id: "strip_polaroid_table_v1",
        canvas_width: cw,
        canvas_height: ch,
        slice_width: sw,
        slice_height: sh,
        slice_count: sc,
        overlap_px: ov,
        background: "#5a3d26",
        slots: polaroid_table_slots(),
        slot_fill_required: Some(fill),
        layout_jitter_px: 0,
        layout_cover_slot_index: None,
        layout_cover_max_jitter: 14,
        layout_flagship_slot_index: None,
        layout_flagship_max_jitter: 18,
        layout_flagship_rim_slot_indices: vec![],
        background_underfill_layers: 0,
        background_underfill_boost_layers: 0,
        background_underfill_repeat_layers: 0,
        background_tail_underfill_layers: 0,
        background_tail_slice_count: 2,
        gap_fill_max_layers: 0,
        gap_fill_beige_tolerance: 48,
        gap_fill_stop_ratio: 0.004,
        procedural_background: Some("wood_polaroid_table"),
        layout_placer: LayoutPlacer::OrganicPolaroid,
    }
}

pub fn default_template() -> StripTemplate {
    template_strip_mural_v2()
}
