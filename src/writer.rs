use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::Command,
};

use indicatif::{ProgressBar, ProgressStyle};

use crate::error::Result;

pub fn write_image(
    image_path: &Path,
    device_path: &Path,
    image_size: u64,
    chunk_size_mib: u64,
    show_progress: bool,
) -> Result<()> {
    let mut image = File::open(image_path)?;
    let mut device = OpenOptions::new().write(true).open(device_path)?;
    let mut buffer = vec![0_u8; (chunk_size_mib * 1024 * 1024) as usize];
    let progress = progress_bar(image_size, show_progress);

    loop {
        let read = image.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        device.write_all(&buffer[..read])?;
        progress.inc(read as u64);
    }

    device.flush()?;
    progress.finish();
    sync_system()?;
    Ok(())
}

fn progress_bar(total: u64, show: bool) -> ProgressBar {
    if !show {
        return ProgressBar::hidden();
    }

    let bar = ProgressBar::new(total);
    let style = ProgressStyle::with_template(
        "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
    )
    .unwrap_or_else(|_| ProgressStyle::default_bar())
    .progress_chars("#>-");
    bar.set_style(style);
    bar
}

fn sync_system() -> Result<()> {
    Command::new("sync").status()?;
    Ok(())
}
