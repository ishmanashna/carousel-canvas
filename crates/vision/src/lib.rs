//! Local ONNX segmentation (u2net human) for out-of-frame photo analysis.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use blake2::{Blake2s256, Digest};
use core::{
    complete_subject_from_mask, occupancy_from_mask, role_from_mask, subject_bbox_from_mask,
    PhotoAnalysis,
};
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, GrayImage, ImageBuffer, ImageDecoder, ImageFormat, ImageReader, Luma};
use ort::ep;
use ort::session::Session;
use ort::value::Tensor;

const MODEL_URL: &str =
    "https://github.com/danielgatis/rembg/releases/download/v0.0.0/u2net_human_seg.onnx";
pub const MODEL_FILENAME: &str = "u2net_human_seg.onnx";
const MODEL_STEM: &str = "u2net_human_seg";
const ORT_DLL_URL: &str =
    "https://github.com/microsoft/onnxruntime/releases/download/v1.28.0/onnxruntime-win-x64-1.28.0.zip";
const ORT_DLL_NAME: &str = "onnxruntime.dll";
const INPUT_SIZE: u32 = 320;
const NORM_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const NORM_STD: [f32; 3] = [0.229, 0.224, 0.225];
/// Below this raw peak, treat the mask as empty (do not min-max stretch noise into fog).
const MASK_MIN_PEAK: f32 = 0.20;
const MASK_MIN_RANGE: f32 = 0.15;

pub type Result<T> = std::result::Result<T, VisionError>;

#[derive(Debug)]
pub enum VisionError {
    Io(std::io::Error),
    Image(image::ImageError),
    Ort(String),
    ModelDownload(String),
    MissingInput(String),
}

impl std::fmt::Display for VisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Image(e) => write!(f, "{e}"),
            Self::Ort(e) => write!(f, "{e}"),
            Self::ModelDownload(msg) => write!(f, "model download failed: {msg}"),
            Self::MissingInput(msg) => write!(f, "missing model input: {msg}"),
        }
    }
}

impl std::error::Error for VisionError {}

impl From<std::io::Error> for VisionError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<image::ImageError> for VisionError {
    fn from(e: image::ImageError) -> Self {
        Self::Image(e)
    }
}

impl<E> From<ort::Error<E>> for VisionError {
    fn from(e: ort::Error<E>) -> Self {
        Self::Ort(e.to_string())
    }
}

/// Root data directory (`%LOCALAPPDATA%/CarouselCanvas` or override for tests).
pub fn data_root() -> PathBuf {
    if let Ok(dir) = std::env::var("CAROUSEL_CANVAS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("CarouselCanvas")
}

fn ort_bin_dir() -> PathBuf {
    data_root().join("bin")
}

fn models_dir() -> PathBuf {
    data_root().join("models")
}

fn masks_dir() -> PathBuf {
    data_root().join("masks")
}

/// ONNX weights path (`%LOCALAPPDATA%/CarouselCanvas/models/u2net_human_seg.onnx`).
pub fn model_path() -> PathBuf {
    models_dir().join(MODEL_FILENAME)
}

fn content_hash_hex(bytes: &[u8]) -> String {
    let mut hasher = Blake2s256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn mask_cache_path(bytes: &[u8]) -> PathBuf {
    masks_dir()
        .join(MODEL_STEM)
        .join(format!("{}.png", content_hash_hex(bytes)))
}

/// Download the human-segmentation ONNX model if missing. Returns the on-disk path.
pub fn ensure_model() -> Result<PathBuf> {
    let path = model_path();
    if path.is_file() {
        return Ok(path);
    }
    fs::create_dir_all(models_dir())?;
    eprintln!("vision: downloading {MODEL_FILENAME}…");
    let mut tmp = path.clone();
    tmp.set_extension("onnx.part");
    let status = std::process::Command::new("curl")
        .args([
            "-fL",
            MODEL_URL,
            "-o",
            &tmp.to_string_lossy(),
        ])
        .status()
        .map_err(|e| VisionError::ModelDownload(e.to_string()))?;
    if !status.success() {
        let _ = fs::remove_file(&tmp);
        return Err(VisionError::ModelDownload(format!(
            "curl exited with {status}"
        )));
    }
    fs::rename(&tmp, &path)?;
    eprintln!("vision: model saved to {}", path.display());
    Ok(path)
}

#[cfg(windows)]
fn preload_directml_neighbor(dll_dir: &Path) {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let Some(local) = local else { return };
    let pyke = local.join("ort.pyke.io").join("dfbin").join("x86_64-pc-windows-msvc");
    if !pyke.is_dir() {
        return;
    }
    for entry in fs::read_dir(&pyke).into_iter().flatten().flatten() {
        let candidate = entry.path().join("DirectML.dll");
        if candidate.is_file() {
            let dest = dll_dir.join("DirectML.dll");
            if !dest.is_file() {
                let _ = fs::copy(&candidate, &dest);
            }
            let _ = ort::util::preload_dylib(dest);
            return;
        }
    }
}

#[cfg(not(windows))]
fn preload_directml_neighbor(_dll_dir: &Path) {}

/// Ensure ONNX Runtime dynamic library is on disk and configured for `load-dynamic`.
fn ensure_ort_dylib() -> Result<PathBuf> {
    let dll_dir = ort_bin_dir();
    let dll = dll_dir.join(ORT_DLL_NAME);
    if dll.is_file() {
        if std::env::var_os("ORT_DYLIB_PATH").is_none() {
            std::env::set_var("ORT_DYLIB_PATH", &dll);
        }
        preload_directml_neighbor(&dll_dir);
        return Ok(dll);
    }

    fs::create_dir_all(&dll_dir)?;
    let zip_path = dll_dir.join("onnxruntime-win-x64.zip");
    eprintln!("vision: downloading {ORT_DLL_NAME}…");
    let status = std::process::Command::new("curl")
        .args(["-fL", ORT_DLL_URL, "-o", &zip_path.to_string_lossy()])
        .status()
        .map_err(|e| VisionError::Ort(e.to_string()))?;
    if !status.success() {
        let _ = fs::remove_file(&zip_path);
        return Err(VisionError::Ort(format!("curl onnxruntime zip exited with {status}")));
    }

    let extract_dir = dll_dir.join("extract");
    if extract_dir.exists() {
        fs::remove_dir_all(&extract_dir)?;
    }
    fs::create_dir_all(&extract_dir)?;
    let tar_status = std::process::Command::new("tar")
        .args([
            "-xf",
            &zip_path.to_string_lossy(),
            "-C",
            &extract_dir.to_string_lossy(),
        ])
        .status()
        .map_err(|e| VisionError::Ort(e.to_string()))?;
    if !tar_status.success() {
        return Err(VisionError::Ort(format!("tar extract exited with {tar_status}")));
    }
    let _ = fs::remove_file(&zip_path);

    let mut found: Option<PathBuf> = None;
    for entry in fs::read_dir(&extract_dir).map_err(VisionError::Io)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let candidate = path.join("lib").join(ORT_DLL_NAME);
            if candidate.is_file() {
                found = Some(candidate);
                break;
            }
        }
    }
    let src = found.ok_or_else(|| VisionError::Ort("onnxruntime.dll not in zip".into()))?;
    fs::copy(&src, &dll)?;
    fs::remove_dir_all(&extract_dir)?;
    eprintln!("vision: {ORT_DLL_NAME} saved to {}", dll.display());
    std::env::set_var("ORT_DYLIB_PATH", &dll);
    preload_directml_neighbor(&dll_dir);
    Ok(dll)
}

fn session_builder() -> Result<ort::session::builder::SessionBuilder> {
    Ok(Session::builder()?
        .with_memory_pattern(false)?
        .with_parallel_execution(false)?)
}

/// Create one ONNX session: DirectML (sequential, no mem pattern) then CPU fallback.
pub fn create_session(model_path: &Path) -> Result<Session> {
    ensure_ort_dylib()?;
    let dml = ep::DirectML::default().build();
    match session_builder()?
        .with_execution_providers([dml])?
        .commit_from_file(model_path)
    {
        Ok(session) => {
            eprintln!("vision: execution provider DirectML");
            return Ok(session);
        }
        Err(e) => {
            eprintln!("vision: DirectML unavailable ({e}), trying CPU");
        }
    }

    let cpu = ep::CPU::default().build();
    let session = session_builder()?
        .with_execution_providers([cpu])?
        .commit_from_file(model_path)?;
    eprintln!("vision: execution provider CPU");
    Ok(session)
}

/// Match render decode: read EXIF/PNG orientation, then pixel-apply before inference.
fn apply_orientation(mut img: DynamicImage, orientation: Orientation) -> DynamicImage {
    img.apply_orientation(orientation);
    img
}

fn load_oriented_rgb(path: &Path) -> Result<image::RgbImage> {
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let img = DynamicImage::from_decoder(decoder)?;
    Ok(apply_orientation(img, orientation).into_rgb8())
}

fn preprocess(rgb: &image::RgbImage) -> (Vec<f32>, u32, u32) {
    let (sw, sh) = rgb.dimensions();
    let resized = image::imageops::resize(rgb, INPUT_SIZE, INPUT_SIZE, FilterType::Lanczos3);
    let side = INPUT_SIZE as usize;
    let mut tensor = vec![0f32; 3 * side * side];
    for y in 0..side {
        for x in 0..side {
            let p = resized.get_pixel(x as u32, y as u32);
            for c in 0..3 {
                let v = (p[c] as f32 / 255.0 - NORM_MEAN[c]) / NORM_STD[c];
                tensor[c * side * side + y * side + x] = v;
            }
        }
    }
    (tensor, sw, sh)
}

fn run_segment(session: &mut Session, rgb: &image::RgbImage) -> Result<GrayImage> {
    let (input, sw, sh) = preprocess(rgb);
    let input_name = session
        .inputs()
        .first()
        .map(|i| i.name().to_string())
        .ok_or_else(|| VisionError::MissingInput("no inputs".into()))?;
    let input_tensor = Tensor::from_array(([1_i64, 3, INPUT_SIZE as i64, INPUT_SIZE as i64], input))?;
    let outputs = session.run(ort::inputs![input_name.as_str() => input_tensor])?;
    let (_shape, data) = outputs[0].try_extract_tensor::<f32>()?;
    let plane = (INPUT_SIZE * INPUT_SIZE) as usize;
    let plane_data: &[f32] = if data.len() >= plane {
        &data[..plane]
    } else {
        data
    };
    let mut min_v = f32::INFINITY;
    let mut max_v = f32::NEG_INFINITY;
    for v in plane_data {
        min_v = min_v.min(*v);
        max_v = max_v.max(*v);
    }
    let mut small = ImageBuffer::new(INPUT_SIZE, INPUT_SIZE);
    if max_v < MASK_MIN_PEAK || (max_v - min_v) < MASK_MIN_RANGE {
        let mask = image::imageops::resize(&small, sw, sh, FilterType::Lanczos3);
        return Ok(mask);
    }
    let range = (max_v - min_v).max(1e-6);
    for y in 0..INPUT_SIZE {
        for x in 0..INPUT_SIZE {
            let idx = (y * INPUT_SIZE + x) as usize;
            let norm = (plane_data[idx] - min_v) / range;
            small.put_pixel(x, y, Luma([(norm * 255.0).round().clamp(0.0, 255.0) as u8]));
        }
    }
    let mask = image::imageops::resize(&small, sw, sh, FilterType::Lanczos3);
    Ok(mask)
}

fn build_analysis(path: &Path, mask_path: PathBuf, mask: &GrayImage) -> PhotoAnalysis {
    let complete = complete_subject_from_mask(mask);
    let role = role_from_mask(mask);
    PhotoAnalysis {
        path: path.to_path_buf(),
        role,
        occupancy: occupancy_from_mask(mask),
        subject_bbox: subject_bbox_from_mask(mask),
        mask_png: mask_path,
        complete_subject: complete,
    }
}

/// Load analysis from a cached mask PNG (no ONNX).
pub fn analysis_from_cached_mask(
    source_path: &Path,
    _source_bytes: &[u8],
    mask_path: &Path,
) -> Result<PhotoAnalysis> {
    let mask = image::open(mask_path)?.into_luma8();
    Ok(build_analysis(source_path, mask_path.to_path_buf(), &mask))
}

fn try_cache_hit(path: &Path, bytes: &[u8]) -> Option<Result<PhotoAnalysis>> {
    let cache = mask_cache_path(bytes);
    if !cache.is_file() {
        return None;
    }
    match analysis_from_cached_mask(path, bytes, &cache) {
        Ok(analysis) => Some(Ok(analysis)),
        Err(e) => {
            eprintln!(
                "vision: dropping corrupt mask {} ({e})",
                cache.display()
            );
            let _ = fs::remove_file(&cache);
            None
        }
    }
}

fn save_mask(cache_path: &Path, mask: &GrayImage) -> Result<()> {
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = match cache_path.file_name() {
        Some(name) => cache_path.with_file_name(format!("{}.part", name.to_string_lossy())),
        None => cache_path.with_extension("part"),
    };
    mask.save_with_format(&tmp, ImageFormat::Png)?;
    fs::rename(&tmp, cache_path)?;
    Ok(())
}

fn path_needs_inference(path: &Path) -> Result<bool> {
    let bytes = fs::read(path)?;
    let cache = mask_cache_path(&bytes);
    if !cache.is_file() {
        return Ok(true);
    }
    match image::open(&cache) {
        Ok(_) => Ok(false),
        Err(_) => {
            let _ = fs::remove_file(&cache);
            Ok(true)
        }
    }
}

/// Analyze one photo. Cache hit reads the mask PNG and skips ONNX.
pub fn analyze_path(path: &Path, session: &mut Session) -> Result<PhotoAnalysis> {
    analyze_path_impl(path, Some(session))
}

fn analyze_path_impl(path: &Path, session: Option<&mut Session>) -> Result<PhotoAnalysis> {
    let bytes = fs::read(path)?;
    if let Some(hit) = try_cache_hit(path, &bytes) {
        return hit;
    }

    let session = session.ok_or_else(|| {
        VisionError::MissingInput("ONNX session required when mask cache misses".into())
    })?;
    let rgb = load_oriented_rgb(path)?;
    let mask = run_segment(session, &rgb)?;
    let cache = mask_cache_path(&bytes);
    save_mask(&cache, &mask)?;
    Ok(build_analysis(path, cache, &mask))
}

/// Analyze many paths on one session. Honors `cancel` between files.
pub fn analyze_folder(
    paths: &[PathBuf],
    cancel: &AtomicBool,
    progress: impl Fn(usize, usize),
) -> Result<Vec<PhotoAnalysis>> {
    if cancel.load(Ordering::Relaxed) || paths.is_empty() {
        return Ok(Vec::new());
    }

    let total = paths.len();
    let any_inference = paths
        .iter()
        .try_fold(false, |acc, path| path_needs_inference(path).map(|need| acc || need))?;

    let mut session = if any_inference {
        let model = ensure_model()?;
        Some(create_session(&model)?)
    } else {
        None
    };

    let mut out = Vec::with_capacity(total);
    for (i, path) in paths.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        progress(i + 1, total);
        out.push(analyze_path_impl(path, session.as_mut())?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::PhotoRole;
    use image::{GenericImageView, Rgb};
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static DATA_DIR_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn write_test_png(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let img: GrayImage = ImageBuffer::from_pixel(32, 32, Luma([0u8]));
        img.save(&path).unwrap();
        path
    }

    #[test]
    fn cache_hit_returns_analysis_without_onnx() {
        let _lock = DATA_DIR_ENV_LOCK.lock().unwrap();
        let tmp = TempDir::new().unwrap();
        std::env::set_var("CAROUSEL_CANVAS_DATA_DIR", tmp.path());
        let photo_dir = tempfile::tempdir().unwrap();
        let photo = write_test_png(photo_dir.path(), "photo.png");
        let bytes = fs::read(&photo).unwrap();
        let cache = mask_cache_path(&bytes);
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        let mask = ImageBuffer::from_pixel(32, 32, Luma([200u8]));
        mask.save(&cache).unwrap();

        let analysis = analysis_from_cached_mask(&photo, &bytes, &cache).unwrap();
        assert_eq!(analysis.path, photo);
        assert_eq!(analysis.mask_png, cache);
        assert_eq!(analysis.role, PhotoRole::Paper);

        let hit = try_cache_hit(&photo, &bytes).unwrap().unwrap();
        assert_eq!(hit.path, photo);

        std::env::remove_var("CAROUSEL_CANVAS_DATA_DIR");
    }

    #[test]
    fn mask_cache_path_includes_model_stem() {
        let p = mask_cache_path(b"abc");
        let text = p.to_string_lossy();
        assert!(
            text.contains("u2net_human_seg") || text.replace('\\', "/").contains("u2net_human_seg"),
            "cache must not reuse isnet masks: {text}"
        );
    }

    #[test]
    fn content_hash_stable() {
        let a = content_hash_hex(b"hello");
        let b = content_hash_hex(b"hello");
        assert_eq!(a, b);
        assert_ne!(a, content_hash_hex(b"world"));
    }

    #[test]
    fn integration_analyze_path_skips_if_no_model() {
        let model = model_path();
        if !model.is_file() {
            eprintln!("vision integration: skipping (no ONNX at {})", model.display());
            return;
        }
        let session = create_session(&model).expect("session");
        let mut session = session;
        let photo_dir = tempfile::tempdir().unwrap();
        let photo = write_test_png(photo_dir.path(), "integration.png");
        let first = analyze_path(&photo, &mut session).expect("first analyze");
        let second = analyze_path(&photo, &mut session).expect("cache hit");
        assert_eq!(first.role, second.role);
        assert!(second.mask_png.is_file());
    }

    #[test]
    fn analyze_folder_respects_cancel() {
        let paths = vec![
            PathBuf::from("nonexistent1.png"),
            PathBuf::from("nonexistent2.png"),
        ];
        let cancel = AtomicBool::new(true);
        let result = analyze_folder(&paths, &cancel, |_, _| {});
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn analyze_folder_empty_skips_onnx() {
        let result = analyze_folder(&[], &AtomicBool::new(false), |_, _| {});
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn analyze_folder_all_cache_hits_skips_onnx() {
        let _lock = DATA_DIR_ENV_LOCK.lock().unwrap();
        let tmp = TempDir::new().unwrap();
        std::env::set_var("CAROUSEL_CANVAS_DATA_DIR", tmp.path());
        let photo_dir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for name in ["a.png", "b.png"] {
            let photo = write_test_png(photo_dir.path(), name);
            let bytes = fs::read(&photo).unwrap();
            let cache = mask_cache_path(&bytes);
            fs::create_dir_all(cache.parent().unwrap()).unwrap();
            ImageBuffer::from_pixel(32, 32, Luma([180u8]))
                .save(&cache)
                .unwrap();
            paths.push(photo);
        }
        let analyses = analyze_folder(&paths, &AtomicBool::new(false), |_, _| {}).unwrap();
        assert_eq!(analyses.len(), 2);
        assert!(!model_path().exists());
        std::env::remove_var("CAROUSEL_CANVAS_DATA_DIR");
    }

    #[test]
    fn corrupt_mask_cache_is_dropped() {
        let _lock = DATA_DIR_ENV_LOCK.lock().unwrap();
        let tmp = TempDir::new().unwrap();
        std::env::set_var("CAROUSEL_CANVAS_DATA_DIR", tmp.path());
        let photo_dir = tempfile::tempdir().unwrap();
        let photo = write_test_png(photo_dir.path(), "photo.png");
        let bytes = fs::read(&photo).unwrap();
        let cache = mask_cache_path(&bytes);
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&cache, b"not a png").unwrap();

        let hit = try_cache_hit(&photo, &bytes);
        assert!(hit.is_none());
        assert!(!cache.is_file());

        std::env::remove_var("CAROUSEL_CANVAS_DATA_DIR");
    }

    #[test]
    fn apply_orientation_swaps_dimensions_on_rotate90() {
        let img: image::RgbImage =
            ImageBuffer::from_fn(4, 2, |x, _y| if x == 0 { Rgb([255, 0, 0]) } else { Rgb([0, 0, 255]) });
        let oriented = apply_orientation(DynamicImage::ImageRgb8(img), Orientation::Rotate90);
        assert_eq!(oriented.dimensions(), (2, 4));
    }
}
