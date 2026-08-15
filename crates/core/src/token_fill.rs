//! Stable RGB per image path for token composes (layout scoring).

use std::path::Path;

use blake2::{Blake2b512, Digest};

pub fn token_rgb_for_path(path: &Path) -> [u8; 3] {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut hasher = Blake2b512::new();
    hasher.update(resolved.to_string_lossy().as_bytes());
    let full = hasher.finalize();
    let h: [u8; 8] = full[..8].try_into().unwrap();
    [
        32 + (h[0] % 200),
        32 + (h[1] % 200),
        32 + (h[2] % 200),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn token_color_stable() {
        let p = PathBuf::from("album/photo_01.jpg");
        let a = token_rgb_for_path(&p);
        let b = token_rgb_for_path(&p);
        assert_eq!(a, b);
        assert!(a[0] >= 32 && a[0] < 232);
    }
}
