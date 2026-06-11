use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use indicatif::{ProgressBar, ProgressStyle};

use crate::error::{EutherError, Result};

pub fn verify_image(
    image_path: &Path,
    device_path: &Path,
    image_size: u64,
    chunk_size_mib: u64,
    show_progress: bool,
) -> Result<()> {
    let mut image = File::open(image_path)?;
    let mut device = File::open(device_path)?;
    device.seek(SeekFrom::Start(0))?;

    let chunk_size = (chunk_size_mib * 1024 * 1024) as usize;
    let mut image_buf = vec![0_u8; chunk_size];
    let mut device_buf = vec![0_u8; chunk_size];
    let mut offset = 0_u64;
    let progress = progress_bar(image_size, show_progress);

    loop {
        let image_read = image.read(&mut image_buf)?;
        if image_read == 0 {
            break;
        }

        device.read_exact(&mut device_buf[..image_read])?;

        if image_buf[..image_read] != device_buf[..image_read] {
            return Err(EutherError::VerificationFailed { offset });
        }

        offset += image_read as u64;
        progress.inc(image_read as u64);
    }

    progress.finish();
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
