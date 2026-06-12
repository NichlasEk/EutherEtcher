mod config;
mod device;
mod error;
mod gui;
mod image;
mod music;
mod safety;
mod verify;
mod writer;

use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};

use crate::{
    config::{default_chunk_size_mib, Config},
    device::{find_device, flatten_visible_devices, list_devices},
    error::{EutherError, Result},
    image::inspect_image,
    safety::{confirm_write, run_safety_checks},
};

#[derive(Debug, Parser)]
#[command(name = "eutheretcher")]
#[command(about = "Safely write .iso and .img files to USB/SD block devices")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    List(ListArgs),
    Flash(FlashArgs),
    Verify(VerifyArgs),
    Gui,
}

#[derive(Debug, Args)]
struct ListArgs {
    #[arg(long)]
    show_internal_drives: bool,
    #[arg(long)]
    show_loops: bool,
}

#[derive(Debug, Args)]
struct FlashArgs {
    #[arg(long)]
    image: Option<PathBuf>,
    #[arg(long)]
    device: Option<PathBuf>,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    force: bool,
    #[arg(long)]
    yes: bool,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct VerifyArgs {
    #[arg(long)]
    image: PathBuf,
    #[arg(long)]
    device: PathBuf,
    #[arg(long)]
    config: Option<PathBuf>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::List(args) => list_command(args),
        Command::Flash(args) => flash_command(args),
        Command::Verify(args) => verify_command(args),
        Command::Gui => gui::run_gui(),
    }
}

fn list_command(args: ListArgs) -> Result<()> {
    let devices = list_devices()?;
    let mut flat = Vec::new();
    flatten_visible_devices(
        &devices,
        &mut flat,
        args.show_internal_drives,
        args.show_loops,
    );

    println!(
        "{:<12} {:<10} {:<10} {:<8} {:<6} {:<28} {:<24} NAME",
        "PATH", "SIZE", "RISK", "TRAN", "TYPE", "MODEL", "MOUNTPOINTS"
    );

    for device in flat {
        println!(
            "{:<12} {:<10} {:<10} {:<8} {:<6} {:<28} {:<24} {}",
            device.path,
            format_size(device.size_bytes),
            device.risk_label(),
            device.transport.as_deref().unwrap_or("-"),
            device.kind,
            truncate(device.model.as_deref().unwrap_or("-"), 28),
            truncate(&format_mountpoints(&device.mountpoints), 24),
            device.name
        );
    }

    Ok(())
}

fn flash_command(args: FlashArgs) -> Result<()> {
    let config = load_optional_config(args.config.as_deref())?;
    let image_path = args
        .image
        .or_else(|| config.flash.image.as_ref().map(PathBuf::from))
        .ok_or(EutherError::MissingValue("flash.image or --image"))?;
    let device_path = args
        .device
        .or_else(|| config.flash.device.as_ref().map(PathBuf::from))
        .ok_or(EutherError::MissingValue("flash.device or --device"))?;
    let chunk_size_mib = config
        .flash
        .chunk_size_mib
        .unwrap_or_else(default_chunk_size_mib);

    let image = inspect_image(&image_path)?;
    let devices = list_devices()?;
    let device = find_device(&devices, path_to_str(&device_path)?)
        .ok_or_else(|| EutherError::DeviceNotFound(device_path.display().to_string()))?;

    run_safety_checks(device, &image, &config.safety, args.force)?;

    safety::print_write_plan(device, &image, chunk_size_mib);

    if args.dry_run {
        println!("dry-run complete; no data was written");
        return Ok(());
    }

    if !args.yes {
        confirm_write(device, &image, config.safety.require_typed_confirmation)?;
    }

    if config.ui.verbose {
        eprintln!(
            "writing {} bytes from {} to {} with {} MiB chunks",
            image.size_bytes,
            image.path.display(),
            device_path.display(),
            chunk_size_mib
        );
    }

    writer::write_image(
        &image.path,
        &device_path,
        image.size_bytes,
        chunk_size_mib,
        config.ui.show_progress,
    )?;

    if config.flash.verify_after_write {
        if config.ui.verbose {
            eprintln!("verifying written bytes");
        }

        verify::verify_image(
            &image.path,
            &device_path,
            image.size_bytes,
            chunk_size_mib,
            config.ui.show_progress,
        )?;
    }

    println!("done");
    Ok(())
}

fn verify_command(args: VerifyArgs) -> Result<()> {
    let config = load_optional_config(args.config.as_deref())?;
    let image = inspect_image(&args.image)?;
    let devices = list_devices()?;
    let device = find_device(&devices, path_to_str(&args.device)?)
        .ok_or_else(|| EutherError::DeviceNotFound(args.device.display().to_string()))?;

    if let Some(size_bytes) = device.size_bytes {
        if image.size_bytes > size_bytes {
            return Err(EutherError::Safety(format!(
                "image is larger than target device ({} > {} bytes)",
                image.size_bytes, size_bytes
            )));
        }
    }

    verify::verify_image(
        &image.path,
        &args.device,
        image.size_bytes,
        config
            .flash
            .chunk_size_mib
            .unwrap_or_else(default_chunk_size_mib),
        config.ui.show_progress,
    )?;

    println!("verified");
    Ok(())
}

fn load_optional_config(path: Option<&Path>) -> Result<Config> {
    match path {
        Some(path) => Config::from_path(path),
        None => Ok(Config::default()),
    }
}

fn path_to_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or(EutherError::MissingValue("valid UTF-8 device path"))
}

fn format_mountpoints(mountpoints: &[String]) -> String {
    if mountpoints.is_empty() {
        "-".to_string()
    } else {
        mountpoints.join(",")
    }
}

fn format_size(bytes: Option<u64>) -> String {
    let Some(bytes) = bytes else {
        return "-".to_string();
    };

    let gib = bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    if gib >= 1.0 {
        format!("{gib:.1}G")
    } else {
        let mib = bytes as f64 / 1024.0 / 1024.0;
        format!("{mib:.1}M")
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        format!("{}...", &value[..max.saturating_sub(3)])
    }
}
