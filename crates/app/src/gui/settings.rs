use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const SETTINGS_NAME: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub input_folder: String,
    #[serde(default = "default_output")]
    pub output_folder: String,
    #[serde(default = "default_true")]
    pub strip_smart_shuffle: bool,
    #[serde(default)]
    pub strip_allow_photo_repeats: bool,
    #[serde(default = "default_template")]
    pub strip_template: String,
    #[serde(default = "default_card_edge")]
    pub strip_card_edge: String,
    #[serde(default = "default_color")]
    pub color: String,
}

fn default_output() -> String {
    "output".into()
}
fn default_true() -> bool {
    true
}
fn default_template() -> String {
    "strip_mural_v2".into()
}
fn default_card_edge() -> String {
    "borderless".into()
}
fn default_color() -> String {
    "white".into()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            input_folder: String::new(),
            output_folder: default_output(),
            strip_smart_shuffle: true,
            strip_allow_photo_repeats: false,
            strip_template: default_template(),
            strip_card_edge: default_card_edge(),
            color: default_color(),
        }
    }
}

pub fn app_data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| dirs_fallback())
        .unwrap_or_else(|| PathBuf::from("."));
    let d = base.join("CarouselCanvas");
    let _ = fs::create_dir_all(&d);
    d
}

fn dirs_fallback() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

pub fn settings_path() -> PathBuf {
    app_data_dir().join(SETTINGS_NAME)
}

pub fn load_settings() -> AppSettings {
    let path = settings_path();
    if !path.is_file() {
        return AppSettings::default();
    }
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => AppSettings::default(),
    }
}

pub fn save_settings(settings: &AppSettings) {
    let path = settings_path();
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        let _ = fs::write(path, text);
    }
}

pub fn reveal_folder(folder: &Path) {
    if !folder.is_dir() {
        let _ = fs::create_dir_all(folder);
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer")
            .arg(folder.as_os_str())
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(folder)
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_json() {
        let s = AppSettings {
            input_folder: "C:\\photos".into(),
            output_folder: "out".into(),
            strip_smart_shuffle: false,
            strip_allow_photo_repeats: true,
            strip_template: "strip_10col".into(),
            strip_card_edge: "wedges".into(),
            color: "beige".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.strip_template, "strip_10col");
        assert!(back.strip_allow_photo_repeats);
    }
}
