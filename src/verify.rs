use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use indicatif::{ProgressBar, ProgressStyle};

use crate::{
    cancel::CancelFlag,
    error::{EutherError, Result},
};

pub fn verify_image(
    image_path: &Path,
    device_path: &Path,
    image_size: u64,
    chunk_size_mib: u64,
    show_progress: bool,
) -> Result<()> {
    let progress = progress_bar(image_size, show_progress);

    verify_image_with_progress(
        image_path,
        device_path,
        chunk_size_mib,
        |verified| {
            progress.set_position(verified);
            Ok(())
        },
        &CancelFlag::default(),
    )?;

    progress.finish();
    Ok(())
}

pub fn verify_image_with_progress<F>(
    image_path: &Path,
    device_path: &Path,
    chunk_size_mib: u64,
    mut on_progress: F,
    cancel: &CancelFlag,
) -> Result<()>
where
    F: FnMut(u64) -> Result<()>,
{
    let mut image = File::open(image_path)?;
    let mut device = File::open(device_path)?;
    device.seek(SeekFrom::Start(0))?;

    let chunk_size = (chunk_size_mib * 1024 * 1024) as usize;
    let mut image_buf = vec![0_u8; chunk_size];
    let mut device_buf = vec![0_u8; chunk_size];
    let mut offset = 0_u64;

    loop {
        cancel.check()?;
        let image_read = image.read(&mut image_buf)?;
        if image_read == 0 {
            break;
        }

        device.read_exact(&mut device_buf[..image_read])?;

        if image_buf[..image_read] != device_buf[..image_read] {
            return Err(EutherError::VerificationFailed { offset });
        }

        offset += image_read as u64;
        on_progress(offset)?;
    }

    Ok(())
}

fn progress_bar(total: u64, show: bool) -> ProgressBar {
    if !show {
        return ProgressBar::hidden();
    }

    let bar = ProgressBar::new(total);
    let style = ProgressStyle::with_template(
        "{spinner:.green} [{elapsed_precise}] [{bar:40.magenta/blue}] {bytes}/{total_bytes} ({eta})",
    )
    .unwrap_or_else(|_| ProgressStyle::default_bar())
    .progress_chars("#>-");
    bar.set_style(style);
    bar
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;
    use crate::cancel::CancelFlag;

    #[test]
    fn verifies_matching_target_file() {
        let mut image = NamedTempFile::new().expect("image temp file");
        let mut target = NamedTempFile::new().expect("target temp file");
        image.write_all(b"match").expect("write image");
        target.write_all(b"match").expect("write target");

        verify_image_with_progress(
            image.path(),
            target.path(),
            1,
            |_verified| Ok(()),
            &CancelFlag::default(),
        )
        .expect("verify should succeed");
    }

    #[test]
    fn rejects_mismatched_target_file() {
        let mut image = NamedTempFile::new().expect("image temp file");
        let mut target = NamedTempFile::new().expect("target temp file");
        image.write_all(b"image").expect("write image");
        target.write_all(b"xxxxx").expect("write target");

        let err = verify_image_with_progress(
            image.path(),
            target.path(),
            1,
            |_verified| Ok(()),
            &CancelFlag::default(),
        )
        .expect_err("verify should fail");

        assert!(matches!(
            err,
            crate::error::EutherError::VerificationFailed { .. }
        ));
    }
}
