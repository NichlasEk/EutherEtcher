use std::{
    io::{self, Write},
    path::Path,
};

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
    if !device.path.starts_with("/dev/") {
        return Err(EutherError::Safety(format!(
            "{} is not under /dev",
            device.path
        )));
    }

    let device_path = Path::new(&device.path);
    if device_path
        .symlink_metadata()
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(EutherError::Safety(format!(
            "{} is a symlink; select the real block device path",
            device.path
        )));
    }

    if device.kind != "disk" {
        return Err(EutherError::Safety(format!(
            "{} is type '{}', not a whole disk",
            device.path, device.kind
        )));
    }

    if device.size_bytes.is_none() && !force {
        return Err(EutherError::Safety(format!(
            "{} has unknown size; pass --force only if you are certain",
            device.path
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

pub fn print_write_plan(device: &BlockDevice, image: &ImageInfo, chunk_size_mib: u64) {
    eprintln!("Write plan:");
    eprintln!("  image:       {}", image.path.display());
    eprintln!("  image bytes: {}", image.size_bytes);
    eprintln!("  device:      {}", device.path);
    eprintln!(
        "  device size: {}",
        device
            .size_bytes
            .map(|size| size.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    eprintln!(
        "  transport:   {}",
        device.transport.as_deref().unwrap_or("unknown")
    );
    eprintln!(
        "  model:       {}",
        device.model.as_deref().unwrap_or("unknown")
    );
    eprintln!("  chunk size:  {chunk_size_mib} MiB");
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn image(size_bytes: u64) -> ImageInfo {
        ImageInfo {
            path: PathBuf::from("test.iso"),
            size_bytes,
        }
    }

    fn disk(path: &str) -> BlockDevice {
        BlockDevice {
            name: path.trim_start_matches("/dev/").to_string(),
            path: path.to_string(),
            size_bytes: Some(8 * 1024 * 1024),
            transport: Some("usb".to_string()),
            model: Some("Test USB".to_string()),
            mountpoints: Vec::new(),
            kind: "disk".to_string(),
            removable: true,
            serial: None,
            wwn: None,
            children: Vec::new(),
        }
    }

    #[test]
    fn refuses_partition_targets() {
        let mut device = disk("/dev/sdb1");
        device.kind = "part".to_string();

        let err = run_safety_checks(&device, &image(1024), &SafetyConfig::default(), false)
            .expect_err("partition target should be refused");

        assert!(err.to_string().contains("not a whole disk"));
    }

    #[test]
    fn refuses_mounted_child_partition() {
        let mut device = disk("/dev/sdb");
        let mut child = disk("/dev/sdb1");
        child.kind = "part".to_string();
        child.mountpoints.push("/mnt/usb".to_string());
        device.children.push(child);

        let err = run_safety_checks(&device, &image(1024), &SafetyConfig::default(), false)
            .expect_err("mounted child should be refused");

        assert!(err.to_string().contains("mounted"));
    }

    #[test]
    fn refuses_internal_drive_without_force() {
        let mut device = disk("/dev/nvme0n1");
        device.transport = Some("nvme".to_string());
        device.removable = false;

        let err = run_safety_checks(&device, &image(1024), &SafetyConfig::default(), false)
            .expect_err("internal drive should be refused");

        assert!(err.to_string().contains("internal drive"));
    }

    #[test]
    fn allows_internal_drive_with_force() {
        let mut device = disk("/dev/nvme0n1");
        device.transport = Some("nvme".to_string());
        device.removable = false;

        run_safety_checks(&device, &image(1024), &SafetyConfig::default(), true)
            .expect("force should allow internal drive after other checks pass");
    }

    #[test]
    fn refuses_image_larger_than_device() {
        let device = disk("/dev/sdb");

        let err = run_safety_checks(
            &device,
            &image(16 * 1024 * 1024),
            &SafetyConfig::default(),
            false,
        )
        .expect_err("oversized image should be refused");

        assert!(err.to_string().contains("larger than target"));
    }

    #[test]
    fn refuses_unknown_device_size_without_force() {
        let mut device = disk("/dev/sdb");
        device.size_bytes = None;

        let err = run_safety_checks(&device, &image(1024), &SafetyConfig::default(), false)
            .expect_err("unknown size should be refused");

        assert!(err.to_string().contains("unknown size"));
    }
}
