use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    UnknownTemplate(String),
    NeedAtLeastOneImage,
    NotEnoughUniquePhotos {
        template_id: String,
        need: usize,
        have: usize,
    },
    SlotLengthMismatch {
        expected: usize,
        got: usize,
    },
    Io(String),
    InvalidInput(String),
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTemplate(id) => write!(f, "Unknown strip template: {id:?}"),
            Self::NeedAtLeastOneImage => write!(f, "Need at least one image path"),
            Self::NotEnoughUniquePhotos {
                template_id,
                need,
                have,
            } => write!(
                f,
                "Strip template {template_id:?} needs {need} different photos (one per image slot). \
                 You only have {have} distinct file(s). Add more images or use --strip-allow-repeats."
            ),
            Self::SlotLengthMismatch { expected, got } => {
                write!(f, "resolved_slots length {got} must match strip effective slots {expected}")
            }
            Self::Io(msg) => write!(f, "{msg}"),
            Self::InvalidInput(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for CoreError {}

pub type Result<T> = std::result::Result<T, CoreError>;

pub fn count_unique_paths(paths: &[PathBuf]) -> usize {
    let mut seen = std::collections::HashSet::new();
    for p in paths {
        if let Ok(canon) = std::fs::canonicalize(p) {
            seen.insert(canon);
        } else {
            seen.insert(p.clone());
        }
    }
    seen.len()
}
