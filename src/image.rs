use std::{
    fs::{self, File},
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChecksumStatus {
    Missing,
    Match { expected: String },
    Mismatch { expected: String },
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

pub fn sha256_file_with_progress<F>(path: &Path, mut on_progress: F) -> Result<String>
where
    F: FnMut(u64),
{
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut done = 0_u64;

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        done += read as u64;
        on_progress(done);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

pub fn checksum_sidecar_status(path: &Path, actual_sha256: &str) -> Result<ChecksumStatus> {
    let sidecar = PathBuf::from(format!("{}.sha256", path.display()));
    if !sidecar.exists() {
        return Ok(ChecksumStatus::Missing);
    }

    let data = fs::read_to_string(sidecar)?;
    let Some(expected) = parse_sha256_sidecar(&data) else {
        return Ok(ChecksumStatus::Missing);
    };

    if expected.eq_ignore_ascii_case(actual_sha256) {
        Ok(ChecksumStatus::Match { expected })
    } else {
        Ok(ChecksumStatus::Mismatch { expected })
    }
}

fn parse_sha256_sidecar(data: &str) -> Option<String> {
    data.split_whitespace()
        .find(|part| part.len() == 64 && part.chars().all(|char| char.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_checksum_sidecar_hash() {
        let hash = "a".repeat(64);
        let data = format!("{hash}  image.iso\n");

        assert_eq!(parse_sha256_sidecar(&data), Some(hash));
    }

    #[test]
    fn ignores_invalid_checksum_sidecar() {
        assert_eq!(parse_sha256_sidecar("not a checksum"), None);
    }
}
