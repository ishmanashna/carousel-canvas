use crate::error::{CoreError, Result};
use crate::template::StripTemplate;

pub const TEMPLATE_IDS: &[&str] = &[
    "strip_mural_v2",
    "strip_polaroid_table_v1",
    "strip_seamless_mosaic_v1",
    "strip_seamless_v1",
    "strip_mural_v1",
    "strip_10col",
];

pub fn get_template_by_id(template_id: &str) -> Result<StripTemplate> {
    match template_id {
        "strip_10col" => Ok(crate::template::template_strip_10col()),
        "strip_mural_v1" => Ok(crate::template::template_strip_mural_v1()),
        "strip_mural_v2" => Ok(crate::template::template_strip_mural_v2()),
        "strip_seamless_v1" => Ok(crate::template::template_strip_seamless_v1()),
        "strip_seamless_mosaic_v1" => Ok(crate::template::template_strip_seamless_mosaic_v1()),
        "strip_polaroid_table_v1" => Ok(crate::template::template_strip_polaroid_table_v1()),
        other => Err(CoreError::UnknownTemplate(other.to_string())),
    }
}

pub fn default_template() -> StripTemplate {
    crate::template::default_template()
}

pub fn max_strip_slots() -> usize {
    TEMPLATE_IDS
        .iter()
        .map(|id| {
            let tpl = get_template_by_id(id).expect("registered template");
            tpl.strip_image_slot_count(None)
        })
        .max()
        .unwrap_or(0)
}
