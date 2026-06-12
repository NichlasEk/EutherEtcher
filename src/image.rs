use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

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

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}
