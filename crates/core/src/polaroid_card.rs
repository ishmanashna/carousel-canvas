//! Polaroid instant-film geometry and per-card tint (port of `app.strip.polaroid_card`).

use crate::python_rng::PythonRandom;

pub const POLAROID_INNER_FILL: [u8; 3] = [252, 252, 250];

/// Side (L=R), top, bottom — instant-film chin (~26–28% of card height).
pub fn polaroid_margins(sw: i32, sh: i32) -> (i32, i32, i32) {
    let m_side = (sw as f64 * 0.042).round() as i32;
    let m_side = m_side.max(5);
    let m_top = (sh as f64 * 0.038).round() as i32;
    let m_top = m_top.max(5);
    let mut m_bottom = (sh as f64 * 0.27).round() as i32;
    m_bottom = m_bottom.max(18);
    let inner_h = sh - m_top - m_bottom;
    let min_inner = ((sw as f64 * 0.45).round() as i32).max(48);
    if inner_h < min_inner {
        m_bottom = (sh - m_top - min_inner).max(12);
    }
    (m_side, m_top, m_bottom)
}

/// Inner photo window size for a card of outer `(sw, sh)`.
pub fn polaroid_inner_dims(sw: i32, sh: i32) -> (i32, i32) {
    let (m_side, m_top, m_bot) = polaroid_margins(sw, sh);
    (
        (sw - 2 * m_side).max(1),
        (sh - m_top - m_bot).max(1),
    )
}

pub fn polaroid_corner_radius(slot_w: i32, slot_h: i32) -> i32 {
    let m = slot_w.min(slot_h) as f64 * 0.021;
    (m.round() as i32).clamp(5, 15)
}

/// Per-slot seed for grain/tint variation (`composer.py` polaroid branch).
pub fn polaroid_slot_seed(layout_seed: i64, slot_index: usize) -> u32 {
    let ls = layout_seed.rem_euclid(1_i64 << 32) as u32;
    ls.wrapping_mul(1009)
        .wrapping_add((slot_index as u32).wrapping_mul(9176))
        & 0xffff_ffff
}

/// Per-card stock tint — subtle variation.
pub fn paper_base_rgb(seed: u32) -> [u8; 3] {
    let mut rng = PythonRandom::new(
        seed.wrapping_mul(0x9e37_79b1)
            .wrapping_add(0xa11ce)
            & 0xffff_ffff,
    );
    let anchors: [[u8; 3]; 12] = [
        [248, 245, 236],
        [242, 244, 252],
        [252, 248, 236],
        [246, 242, 232],
        [244, 248, 238],
        [250, 244, 246],
        [252, 250, 238],
        [238, 240, 244],
        [255, 248, 228],
        [240, 244, 236],
        [248, 240, 242],
        [244, 246, 250],
    ];
    let idx = rng.randrange(anchors.len() as u32) as usize;
    let [br, bg, bb] = anchors[idx];
    [
        (br as i32 + rng.randint(-18, 18)).clamp(225, 255) as u8,
        (bg as i32 + rng.randint(-20, 20)).clamp(222, 255) as u8,
        (bb as i32 + rng.randint(-22, 22)).clamp(215, 255) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margins_produce_readable_chin() {
        let (side, top, bot) = polaroid_margins(740, 920);
        assert!(side >= 5);
        assert!(top >= 5);
        assert!(bot >= 18);
        let (_, ih) = polaroid_inner_dims(740, 920);
        assert!(bot as f64 / 920.0 > 0.2);
        assert!(ih >= 48);
    }

    #[test]
    fn inner_dims_match_margins() {
        let sw = 740;
        let sh = 920;
        let (ms, mt, mb) = polaroid_margins(sw, sh);
        let (iw, ih) = polaroid_inner_dims(sw, sh);
        assert_eq!(iw, sw - 2 * ms);
        assert_eq!(ih, sh - mt - mb);
    }

    #[test]
    fn slot_seed_stable() {
        assert_eq!(polaroid_slot_seed(42, 3), polaroid_slot_seed(42, 3));
        assert_ne!(polaroid_slot_seed(42, 3), polaroid_slot_seed(42, 4));
    }
}
