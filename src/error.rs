use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum EutherError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse TOML config: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("failed to parse lsblk JSON output: {0}")]
    Json(#[from] serde_json::Error),

    #[error("lsblk failed: {0}")]
    Lsblk(String),

    #[error("missing required value: {0}")]
    MissingValue(&'static str),

    #[error("image file does not exist: {0}")]
    ImageNotFound(PathBuf),

    #[error("unsupported image extension for {0}; expected .iso or .img")]
    UnsupportedImage(PathBuf),

    #[error("device not found in lsblk output: {0}")]
    DeviceNotFound(String),

    #[error("safety check failed: {0}")]
    Safety(String),

    #[error("confirmation failed")]
    ConfirmationFailed,

    #[error("verification failed at byte offset {offset}")]
    VerificationFailed { offset: u64 },

    #[error("audio error: {0}")]
    Audio(String),
}

pub type Result<T> = std::result::Result<T, EutherError>;
