use std::path::PathBuf;

#[derive(Debug)]
pub enum DecodeError {
    Io(std::io::Error),
    Image(image::ImageError),
    Jpeg(jpeg_decoder::Error),
    OrientationMismatch,
    EmptySource,
    Path(PathBuf),
    Encode(String),
    Gpu(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Image(e) => write!(f, "image error: {e}"),
            Self::Jpeg(e) => write!(f, "jpeg error: {e}"),
            Self::OrientationMismatch => write!(f, "photo orientation does not match slot preference"),
            Self::EmptySource => write!(f, "decoded image has zero size"),
            Self::Path(p) => write!(f, "missing image: {}", p.display()),
            Self::Encode(msg) => write!(f, "encode error: {msg}"),
            Self::Gpu(msg) => write!(f, "gpu error: {msg}"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<std::io::Error> for DecodeError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<image::ImageError> for DecodeError {
    fn from(value: image::ImageError) -> Self {
        Self::Image(value)
    }
}

impl From<jpeg_decoder::Error> for DecodeError {
    fn from(value: jpeg_decoder::Error) -> Self {
        Self::Jpeg(value)
    }
}

pub type Result<T> = std::result::Result<T, DecodeError>;
