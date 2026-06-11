use std::io::{self, Write};

use crate::{
    config::SafetyConfig,
    device::BlockDevice,
    error::{EutherError, Result},
    image::ImageInfo,
};

pub fn run_safety_checks(
    device: &BlockDevice,
    image: &ImageInfo,
    config: &SafetyConfig,
    force: bool,
) -> Result<()> {
    if device.kind != "disk" {
        return Err(EutherError::Safety(format!(
            "{} is type '{}', not a whole disk",
            device.path, device.kind
        )));
    }

    if config.refuse_mounted_devices && device.has_mountpoints_recursive() {
        return Err(EutherError::Safety(format!(
            "{} or one of its partitions is mounted",
            device.path
        )));
    }

    if config.refuse_internal_drives && device.is_likely_internal() && !force {
        return Err(EutherError::Safety(format!(
            "{} looks like an internal drive; pass --force only if you are certain",
            device.path
        )));
    }

    if let Some(size_bytes) = device.size_bytes {
        if image.size_bytes > size_bytes {
            return Err(EutherError::Safety(format!(
                "image is larger than target device ({} > {} bytes)",
                image.size_bytes, size_bytes
            )));
        }

        let max_bytes = config.max_device_size_gib_without_force * 1024 * 1024 * 1024;
        if size_bytes > max_bytes && !force {
            return Err(EutherError::Safety(format!(
                "{} is larger than {} GiB; pass --force only if you are certain",
                device.path, config.max_device_size_gib_without_force
            )));
        }
    }

    Ok(())
}

pub fn confirm_write(device: &BlockDevice, image: &ImageInfo, require_typed: bool) -> Result<()> {
    eprintln!("About to write:");
    eprintln!(
        "  image:  {} ({} bytes)",
        image.path.display(),
        image.size_bytes
    );
    eprintln!(
        "  device: {} ({})",
        device.path,
        device.model.as_deref().unwrap_or("unknown model")
    );
    eprintln!();
    eprintln!("This will destroy data on the target device.");
    eprint!("Type YES to continue: ");
    io::stderr().flush()?;

    let mut first = String::new();
    io::stdin().read_line(&mut first)?;
    if first.trim() != "YES" {
        return Err(EutherError::ConfirmationFailed);
    }

    if require_typed {
        eprint!("Type the exact device path ({}) to confirm: ", device.path);
        io::stderr().flush()?;

        let mut second = String::new();
        io::stdin().read_line(&mut second)?;
        if second.trim() != device.path {
            return Err(EutherError::ConfirmationFailed);
        }
    }

    Ok(())
}
