pub mod assign;
pub mod error;
pub mod hero_pin;
pub mod io;
pub mod layout_jitter;
pub mod layout_polaroid;
pub mod layout_retry;
pub mod mosaic;
pub mod polaroid_card;
pub mod python_rng;
pub mod registry;
pub mod scene;
pub mod slot;
pub mod slot_fill;
pub mod template;
pub mod token_fill;
pub mod underfill;

pub use assign::{
    aspect_hint_from_path, pick_dumb_fills, pick_smart_fills, pick_underfill_paths,
    validate_strip_unique_sources, AspectClass,
};
pub use hero_pin::{pin_strip_hero_fill, resolve_strip_hero_argument};
pub use error::{CoreError, Result, count_unique_paths};
pub use io::{is_image_path, scan_image_folder, validate_unique_sources, IMAGE_EXTENSIONS};
pub use layout_retry::{
    pick_best_layout_seed_with_token_retry, resolve_slots_for_layout,
    strip_layout_token_retry_enabled, underfill_rng_from_layout_seed, underfill_rng_seed,
    MURAL_V2_LAYOUT_RETRY_ATTEMPTS, TOKEN_COMPOSE_SCALE,
};
pub use layout_polaroid::resolve_organic_polaroid_slots;
pub use mosaic::{build_seamless_mosaic_v1_slots, mosaic_canvas_extent, mosaic_v1_max_slot_count};
pub use polaroid_card::{
    paper_base_rgb, polaroid_corner_radius, polaroid_inner_dims, polaroid_margins,
    polaroid_slot_seed, POLAROID_INNER_FILL,
};
pub use python_rng::PythonRandom;
pub use registry::{default_template, get_template_by_id, max_strip_slots, TEMPLATE_IDS};
pub use scene::{build_scene, build_scene_with_underfill, compute_bleed_px, Rect, Scene, SceneCard};
pub use slot_fill::{
    build_scene_from_fills, build_scene_from_fills_with_underfill, fills_to_paths, scale_rect,
    scale_underfill_box, SlotFill,
};
pub use slot::{Fit, StripSlotDef};
pub use template::{LayoutPlacer, StripTemplate};
pub use token_fill::token_rgb_for_path;
pub use underfill::{
    carousel_tail_band_x_range, plan_and_assign_mural_underfill, plan_background_underfill_boxes,
    plan_mural_underfill_boxes, PlannedUnderfill, UnderfillBox,
};
