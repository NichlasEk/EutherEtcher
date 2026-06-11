use std::process::Command;

use serde::Deserialize;

use crate::error::{EutherError, Result};

#[derive(Debug, Clone)]
pub struct BlockDevice {
    pub name: String,
    pub path: String,
    pub size_bytes: Option<u64>,
    pub transport: Option<String>,
    pub model: Option<String>,
    pub mountpoints: Vec<String>,
    pub kind: String,
    pub removable: bool,
    pub children: Vec<BlockDevice>,
}

#[derive(Debug, Deserialize)]
struct LsblkOutput {
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Debug, Deserialize)]
struct LsblkDevice {
    name: String,
    path: Option<String>,
    size: Option<u64>,
    tran: Option<String>,
    model: Option<String>,
    #[serde(default)]
    mountpoints: Vec<Option<String>>,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    rm: bool,
    #[serde(default)]
    children: Vec<LsblkDevice>,
}

pub fn list_devices() -> Result<Vec<BlockDevice>> {
    let output = Command::new("lsblk")
        .args([
            "--json",
            "--bytes",
            "--output",
            "NAME,PATH,SIZE,TRAN,MODEL,MOUNTPOINTS,TYPE,RM",
        ])
        .output()?;

    if !output.status.success() {
        return Err(EutherError::Lsblk(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    parse_lsblk_json(&output.stdout)
}

pub fn parse_lsblk_json(bytes: &[u8]) -> Result<Vec<BlockDevice>> {
    let parsed: LsblkOutput = serde_json::from_slice(bytes)?;
    Ok(parsed
        .blockdevices
        .into_iter()
        .map(BlockDevice::from)
        .collect())
}

pub fn find_device<'a>(devices: &'a [BlockDevice], path: &str) -> Option<&'a BlockDevice> {
    for device in devices {
        if device.path == path {
            return Some(device);
        }
        if let Some(found) = find_device(&device.children, path) {
            return Some(found);
        }
    }
    None
}

pub fn flatten_devices<'a>(devices: &'a [BlockDevice], out: &mut Vec<&'a BlockDevice>) {
    for device in devices {
        out.push(device);
        flatten_devices(&device.children, out);
    }
}

impl BlockDevice {
    pub fn has_mountpoints_recursive(&self) -> bool {
        !self.mountpoints.is_empty()
            || self
                .children
                .iter()
                .any(BlockDevice::has_mountpoints_recursive)
    }

    pub fn is_likely_internal(&self) -> bool {
        matches!(self.transport.as_deref(), Some("sata" | "nvme" | "ata"))
            || (!self.removable && self.transport.as_deref() != Some("usb"))
    }
}

impl From<LsblkDevice> for BlockDevice {
    fn from(value: LsblkDevice) -> Self {
        Self {
            path: value.path.unwrap_or_else(|| format!("/dev/{}", value.name)),
            name: value.name,
            size_bytes: value.size,
            transport: value.tran,
            model: value.model.map(|model| model.trim().to_string()),
            mountpoints: value.mountpoints.into_iter().flatten().collect(),
            kind: value.kind,
            removable: value.rm,
            children: value.children.into_iter().map(BlockDevice::from).collect(),
        }
    }
}
