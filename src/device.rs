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
    pub serial: Option<String>,
    pub wwn: Option<String>,
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
    serial: Option<String>,
    wwn: Option<String>,
    #[serde(default)]
    children: Vec<LsblkDevice>,
}

pub fn list_devices() -> Result<Vec<BlockDevice>> {
    let output = Command::new("lsblk")
        .args([
            "--json",
            "--bytes",
            "--output",
            "NAME,PATH,SIZE,TRAN,MODEL,MOUNTPOINTS,TYPE,RM,SERIAL,WWN",
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

pub fn flatten_visible_devices<'a>(
    devices: &'a [BlockDevice],
    out: &mut Vec<&'a BlockDevice>,
    show_internal_drives: bool,
    show_loops: bool,
) {
    flatten_visible_devices_with_parent(devices, out, show_internal_drives, show_loops, false);
}

fn flatten_visible_devices_with_parent<'a>(
    devices: &'a [BlockDevice],
    out: &mut Vec<&'a BlockDevice>,
    show_internal_drives: bool,
    show_loops: bool,
    parent_visible: bool,
) {
    for device in devices {
        let visible = device.is_visible_candidate(show_internal_drives, show_loops, parent_visible);
        if visible {
            out.push(device);
        }

        flatten_visible_devices_with_parent(
            &device.children,
            out,
            show_internal_drives,
            show_loops,
            visible,
        );
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

    pub fn is_loop(&self) -> bool {
        self.kind == "loop" || self.path.starts_with("/dev/loop")
    }

    pub fn is_removable_target(&self) -> bool {
        self.kind == "disk"
            && !self.is_loop()
            && (self.removable || matches!(self.transport.as_deref(), Some("usb" | "mmc")))
    }

    pub fn is_dangerous_internal(&self) -> bool {
        self.kind == "disk"
            && !self.is_loop()
            && matches!(self.transport.as_deref(), Some("sata" | "nvme" | "ata"))
    }

    pub fn is_visible_candidate(
        &self,
        show_internal_drives: bool,
        show_loops: bool,
        parent_visible: bool,
    ) -> bool {
        if self.is_loop() {
            return show_loops;
        }

        if self.is_dangerous_internal() {
            return show_internal_drives;
        }

        self.is_removable_target() || (self.kind == "part" && parent_visible)
    }

    pub fn risk_label(&self) -> &'static str {
        if self.is_dangerous_internal() {
            "DANGER"
        } else if self.is_removable_target() {
            "REMOVABLE"
        } else if self.is_loop() {
            "LOOP"
        } else {
            "OTHER"
        }
    }

    pub fn identity_fingerprint(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.path,
            self.size_bytes
                .map(|size| size.to_string())
                .unwrap_or_else(|| "-".to_string()),
            self.transport.as_deref().unwrap_or("-"),
            self.model.as_deref().unwrap_or("-"),
            self.serial.as_deref().unwrap_or("-"),
            self.wwn.as_deref().unwrap_or("-")
        )
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
            serial: value.serial.map(|serial| serial.trim().to_string()),
            wwn: value.wwn.map(|wwn| wwn.trim().to_string()),
            children: value.children.into_iter().map(BlockDevice::from).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lsblk_json_with_children_and_mountpoints() {
        let json = br#"
        {
          "blockdevices": [
            {
              "name": "sdb",
              "path": "/dev/sdb",
              "size": 16000000000,
              "tran": "usb",
              "model": "USB DISK",
              "mountpoints": [null],
              "type": "disk",
              "rm": true,
              "serial": "ABC123",
              "wwn": "0x123",
              "children": [
                {
                  "name": "sdb1",
                  "path": "/dev/sdb1",
                  "size": 15999000000,
                  "tran": null,
                  "model": null,
                  "mountpoints": ["/run/media/user/USB"],
                  "type": "part",
                  "rm": true,
                  "serial": null,
                  "wwn": null
                }
              ]
            }
          ]
        }
        "#;

        let devices = parse_lsblk_json(json).expect("lsblk JSON should parse");
        let disk = &devices[0];

        assert_eq!(disk.name, "sdb");
        assert_eq!(disk.path, "/dev/sdb");
        assert_eq!(disk.size_bytes, Some(16_000_000_000));
        assert_eq!(disk.transport.as_deref(), Some("usb"));
        assert_eq!(disk.model.as_deref(), Some("USB DISK"));
        assert_eq!(disk.serial.as_deref(), Some("ABC123"));
        assert_eq!(disk.wwn.as_deref(), Some("0x123"));
        assert!(disk.mountpoints.is_empty());
        assert_eq!(disk.children.len(), 1);
        assert_eq!(disk.children[0].mountpoints, ["/run/media/user/USB"]);
        assert!(disk.has_mountpoints_recursive());
    }

    #[test]
    fn finds_nested_device_by_path() {
        let json = br#"
        {
          "blockdevices": [
            {
              "name": "sdb",
              "path": "/dev/sdb",
              "size": 1024,
              "tran": "usb",
              "model": "USB",
              "mountpoints": [],
              "type": "disk",
              "rm": true,
              "serial": null,
              "wwn": null,
              "children": [
                {
                  "name": "sdb1",
                  "path": "/dev/sdb1",
                  "size": 512,
                  "tran": null,
                  "model": null,
                  "mountpoints": [],
                  "type": "part",
                  "rm": true,
                  "serial": null,
                  "wwn": null
                }
              ]
            }
          ]
        }
        "#;

        let devices = parse_lsblk_json(json).expect("lsblk JSON should parse");
        let found = find_device(&devices, "/dev/sdb1").expect("nested device should be found");

        assert_eq!(found.name, "sdb1");
        assert_eq!(found.kind, "part");
    }

    #[test]
    fn hides_loop_devices_by_default() {
        let mut loop_device = test_disk("/dev/loop0", "loop", None, false);
        loop_device.kind = "loop".to_string();
        let devices = vec![loop_device];
        let mut visible = Vec::new();

        flatten_visible_devices(&devices, &mut visible, false, false);
        assert!(visible.is_empty());

        flatten_visible_devices(&devices, &mut visible, false, true);
        assert_eq!(visible.len(), 1);
    }

    #[test]
    fn hides_internal_sata_and_nvme_without_opt_in() {
        let devices = vec![
            test_disk("/dev/sda", "disk", Some("sata"), false),
            test_disk("/dev/nvme0n1", "disk", Some("nvme"), false),
            test_disk("/dev/sdb", "disk", Some("usb"), true),
        ];
        let mut visible = Vec::new();

        flatten_visible_devices(&devices, &mut visible, false, false);
        assert_eq!(
            visible
                .iter()
                .map(|device| device.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/dev/sdb"]
        );

        visible.clear();
        flatten_visible_devices(&devices, &mut visible, true, false);
        assert_eq!(visible.len(), 3);
        assert!(visible[0].is_dangerous_internal());
        assert_eq!(visible[0].risk_label(), "DANGER");
    }

    #[test]
    fn hides_partitions_when_parent_drive_is_hidden() {
        let mut internal = test_disk("/dev/sda", "disk", Some("sata"), false);
        internal
            .children
            .push(test_disk("/dev/sda1", "part", None, false));
        let mut removable = test_disk("/dev/sdb", "disk", Some("usb"), true);
        removable
            .children
            .push(test_disk("/dev/sdb1", "part", None, true));
        let devices = vec![internal, removable];
        let mut visible = Vec::new();

        flatten_visible_devices(&devices, &mut visible, false, false);

        assert_eq!(
            visible
                .iter()
                .map(|device| device.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/dev/sdb", "/dev/sdb1"]
        );
    }

    fn test_disk(path: &str, kind: &str, transport: Option<&str>, removable: bool) -> BlockDevice {
        BlockDevice {
            name: path.trim_start_matches("/dev/").to_string(),
            path: path.to_string(),
            size_bytes: Some(1024),
            transport: transport.map(str::to_string),
            model: Some("Test".to_string()),
            mountpoints: Vec::new(),
            kind: kind.to_string(),
            removable,
            serial: None,
            wwn: None,
            children: Vec::new(),
        }
    }
}
