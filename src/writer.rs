use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::Command,
};

use indicatif::{ProgressBar, ProgressStyle};

use crate::{cancel::CancelFlag, error::Result};

pub fn write_image(
    image_path: &Path,
    device_path: &Path,
    image_size: u64,
    chunk_size_mib: u64,
    show_progress: bool,
) -> Result<()> {
    let progress = progress_bar(image_size, show_progress);

    write_image_with_progress(
        image_path,
        device_path,
        chunk_size_mib,
        |written| {
            progress.set_position(written);
            Ok(())
        },
        &CancelFlag::default(),
    )?;

    progress.finish();
    Ok(())
}

pub fn write_image_with_progress<F>(
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
    let mut device = OpenOptions::new().write(true).open(device_path)?;
    let mut buffer = vec![0_u8; (chunk_size_mib * 1024 * 1024) as usize];
    let mut written = 0_u64;

    loop {
        cancel.check()?;
        let read = image.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        device.write_all(&buffer[..read])?;
        written += read as u64;
        on_progress(written)?;
    }

    device.flush()?;
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

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use tempfile::NamedTempFile;

    use super::*;
    use crate::cancel::CancelFlag;

    #[test]
    fn writes_image_to_target_file() {
        let mut image = NamedTempFile::new().expect("image temp file");
        let target = NamedTempFile::new().expect("target temp file");
        image.write_all(b"eutheretcher").expect("write image");

        write_image_with_progress(
            image.path(),
            target.path(),
            1,
            |_written| Ok(()),
            &CancelFlag::default(),
        )
        .expect("write should succeed");

        assert_eq!(
            fs::read(target.path()).expect("read target"),
            b"eutheretcher"
        );
    }

    #[test]
    fn cancels_write_before_next_chunk() {
        let mut image = NamedTempFile::new().expect("image temp file");
        let target = NamedTempFile::new().expect("target temp file");
        image
            .write_all(&vec![7_u8; 2 * 1024 * 1024])
            .expect("write image");
        let cancel = CancelFlag::default();

        let err = write_image_with_progress(
            image.path(),
            target.path(),
            1,
            |_written| {
                cancel.cancel();
                Ok(())
            },
            &cancel,
        )
        .expect_err("write should be cancelled");

        assert!(matches!(err, crate::error::EutherError::Cancelled));
    }
}
