use std::{
    fs::File,
    path::{Path, PathBuf},
};

use crate::error::{EutherError, Result};

#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub path: PathBuf,
    pub size_bytes: u64,
}

pub fn inspect_image(path: &Path) -> Result<ImageInfo> {
    if !path.exists() {
        return Err(EutherError::ImageNotFound(path.to_path_buf()));
    }

    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);

    if !matches!(extension.as_deref(), Some("iso" | "img")) {
        return Err(EutherError::UnsupportedImage(path.to_path_buf()));
    }

    let file = File::open(path)?;
    let size_bytes = file.metadata()?.len();

    Ok(ImageInfo {
        path: path.to_path_buf(),
        size_bytes,
    })
}
