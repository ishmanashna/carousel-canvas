use std::collections::{HashMap, VecDeque};

use std::path::{Path, PathBuf};

use std::sync::{Arc, Mutex};



use image::RgbaImage;



use crate::DecodeOptions;



/// Soft cap for cached RGBA payloads (~256 MiB of pixel bytes).

pub const DECODE_CACHE_DEFAULT_MAX_BYTES: usize = 256 * 1024 * 1024;



#[derive(Debug, Clone, PartialEq, Eq, Hash)]

struct CacheKey {

    path: PathBuf,

    dest_w: u32,

    dest_h: u32,

    pan_x_milli: i32,

    pan_y_milli: i32,

    flip_h: bool,

    trim_milli: Option<i32>,

    band_milli: Option<i32>,

    cover_height_first: bool,

    fit_tag: u8,

}



impl CacheKey {

    fn from_options(path: &Path, dest_w: u32, dest_h: u32, options: &DecodeOptions) -> Self {

        Self {

            path: path.to_path_buf(),

            dest_w,

            dest_h,

            pan_x_milli: (options.pan_x.clamp(-1.0, 1.0) * 1000.0).round() as i32,

            pan_y_milli: (options.pan_y.clamp(-1.0, 1.0) * 1000.0).round() as i32,

            flip_h: options.flip_h,

            trim_milli: options

                .source_trim_left_frac

                .map(|v| (v.clamp(0.0, 0.49) * 10_000.0).round() as i32),

            band_milli: options

                .horizontal_center_band_frac

                .map(|v| (v.clamp(0.05, 1.0) * 10_000.0).round() as i32),

            cover_height_first: options.cover_height_first,

            fit_tag: match options.fit {

                core::Fit::Cover => 0,

                core::Fit::Contain => 1,

            },

        }

    }

}



fn image_bytes(img: &RgbaImage) -> usize {

    img.width() as usize * img.height() as usize * 4

}



#[derive(Debug)]
struct CacheInner {

    map: HashMap<CacheKey, Arc<RgbaImage>>,

    order: VecDeque<CacheKey>,

    bytes: usize,

    max_bytes: usize,

}



impl CacheInner {

    fn touch(&mut self, key: &CacheKey) {

        if let Some(pos) = self.order.iter().position(|k| k == key) {

            self.order.remove(pos);

        }

        self.order.push_back(key.clone());

    }



    fn evict_one(&mut self) {

        while let Some(old) = self.order.pop_front() {

            if let Some(img) = self.map.remove(&old) {

                self.bytes = self.bytes.saturating_sub(image_bytes(&img));

                return;

            }

        }

    }



    fn evict_until_fit(&mut self, incoming: usize) {

        while self.bytes + incoming > self.max_bytes && !self.map.is_empty() {

            self.evict_one();

        }

    }

}



/// Thread-safe decode cache keyed by path, destination size, pan, trim, band, flip, and fit.

/// Evicts least-recently-used entries when the byte budget is exceeded.

#[derive(Debug)]

pub struct DecodeCache {

    inner: Mutex<CacheInner>,

}



impl Default for DecodeCache {

    fn default() -> Self {

        Self::with_max_bytes(DECODE_CACHE_DEFAULT_MAX_BYTES)

    }

}



impl DecodeCache {

    pub fn new() -> Self {

        Self::default()

    }



    pub fn with_max_bytes(max_bytes: usize) -> Self {

        Self {

            inner: Mutex::new(CacheInner {

                map: HashMap::new(),

                order: VecDeque::new(),

                bytes: 0,

                max_bytes: max_bytes.max(1),

            }),

        }

    }



    pub fn get(

        &self,

        path: &Path,

        dest_w: u32,

        dest_h: u32,

        options: &DecodeOptions,

    ) -> Option<Arc<RgbaImage>> {

        let key = CacheKey::from_options(path, dest_w, dest_h, options);

        let mut guard = self.inner.lock().ok()?;

        let hit = guard.map.get(&key).cloned()?;

        guard.touch(&key);

        Some(hit)

    }



    pub fn insert(

        &self,

        path: &Path,

        dest_w: u32,

        dest_h: u32,

        options: &DecodeOptions,

        image: RgbaImage,

    ) {

        let key = CacheKey::from_options(path, dest_w, dest_h, options);

        let incoming = image_bytes(&image);

        if let Ok(mut guard) = self.inner.lock() {

            if let Some(prev) = guard.map.remove(&key) {

                guard.bytes = guard.bytes.saturating_sub(image_bytes(&prev));

                if let Some(pos) = guard.order.iter().position(|k| k == &key) {

                    guard.order.remove(pos);

                }

            }

            // Oversized single entry: keep only that entry (still useful for re-get).

            if incoming > guard.max_bytes {

                guard.map.clear();

                guard.order.clear();

                guard.bytes = 0;

            } else {

                guard.evict_until_fit(incoming);

            }

            guard.bytes = guard.bytes.saturating_add(incoming);

            guard.map.insert(key.clone(), Arc::new(image));

            guard.order.push_back(key);

        }

    }



    pub fn len(&self) -> usize {

        self.inner.lock().map(|g| g.map.len()).unwrap_or(0)

    }



    pub fn current_bytes(&self) -> usize {

        self.inner.lock().map(|g| g.bytes).unwrap_or(0)

    }



    pub fn clear(&self) {

        if let Ok(mut guard) = self.inner.lock() {

            guard.map.clear();

            guard.order.clear();

            guard.bytes = 0;

        }

    }

}



#[cfg(test)]

mod tests {

    use super::*;

    use core::Fit;

    use image::{Rgba, RgbaImage};



    fn opts() -> DecodeOptions {

        DecodeOptions {

            fit: Fit::Cover,

            pan_x: 0.0,

            pan_y: 0.0,

            flip_h: false,

            source_trim_left_frac: None,

            horizontal_center_band_frac: None,

            cover_height_first: false,

            contain_fill_rgb: [0, 0, 0],

            require_portrait: false,

            require_landscape: false,

        }

    }



    #[test]

    fn evicts_lru_when_over_budget() {

        let cache = DecodeCache::with_max_bytes(100 * 100 * 4 * 2 + 8);

        let o = opts();

        let a = RgbaImage::from_pixel(100, 100, Rgba([1, 0, 0, 255]));

        let b = RgbaImage::from_pixel(100, 100, Rgba([0, 1, 0, 255]));

        let c = RgbaImage::from_pixel(100, 100, Rgba([0, 0, 1, 255]));

        cache.insert(Path::new("a.jpg"), 100, 100, &o, a);

        cache.insert(Path::new("b.jpg"), 100, 100, &o, b);

        assert_eq!(cache.len(), 2);

        let _ = cache.get(Path::new("a.jpg"), 100, 100, &o);

        cache.insert(Path::new("c.jpg"), 100, 100, &o, c);

        assert!(cache.get(Path::new("a.jpg"), 100, 100, &o).is_some());

        assert!(cache.get(Path::new("b.jpg"), 100, 100, &o).is_none());

        assert!(cache.get(Path::new("c.jpg"), 100, 100, &o).is_some());

    }

}


