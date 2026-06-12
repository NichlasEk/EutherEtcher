# EutherEtcher

EutherEtcher is a Linux-first, Rust-based CLI prototype for safely writing raw
`.iso` and `.img` images to USB and SD block devices.

The first version is deliberately conservative. It would rather refuse to write
than risk writing to the wrong disk.

## Commands

```bash
eutheretcher list
eutheretcher list --show-internal-drives
eutheretcher flash --image ./file.iso --device /dev/sdX
eutheretcher flash --config ./eutheretcher.toml
eutheretcher verify --image ./file.iso --device /dev/sdX
eutheretcher gui
```

## Safety

Before flashing, EutherEtcher performs basic checks:

- Refuses mounted devices by default.
- Refuses likely internal drives by default.
- Refuses devices above a configured size limit unless `--force` is passed.
- Requires double confirmation before writing.
- Refuses when the image is larger than the target device.
- Runs `sync` after writing.
- Supports a dry-run mode before writing.
- Hides loop devices by default.
- Hides internal SATA/NVMe drives unless explicitly requested.
- Marks internal SATA/NVMe drives as `DANGER`.
- Highlights removable USB/SD-style targets as the normal path.

Run `eutheretcher list` and inspect the device path carefully before flashing.

## Configuration

All EutherEtcher configuration is TOML. See
[`examples/eutheretcher.toml`](examples/eutheretcher.toml).

```toml
[flash]
image = "./archlinux.iso"
device = "/dev/sdX"
verify_after_write = true
chunk_size_mib = 4

[safety]
refuse_internal_drives = true
refuse_mounted_devices = true
max_device_size_gib_without_force = 256
require_typed_confirmation = true

[ui]
show_progress = true
verbose = true
```

## Build

```bash
cargo build
```

## Install Desktop App

```bash
scripts/install-desktop.sh
```

This installs the release binary, app icon, and desktop launcher under
`~/.local`.

For the cleanest `pkexec` prompt, install the system polkit policy too:

```bash
sudo install -m 0755 target/release/eutheretcher /usr/local/bin/eutheretcher
sudo install -m 0644 packaging/dev.euther.EutherEtcher.policy /usr/share/polkit-1/actions/dev.euther.EutherEtcher.policy
```

## GUI

The GUI is available as a native Linux window:

```bash
cargo run -- gui
```

It uses the same safety checks as the CLI. Flashing from the GUI still requires
typing the exact target device path.

The GUI flow is intentionally simple:

- Select an `.iso` or `.img` with the native file picker.
- Or drag-and-drop an `.iso` or `.img` into the window.
- Select a whole-disk USB/SD target card.
- Review the pre-flight confirmation.
- Type the exact target path before the final flash button unlocks.

Partitions are shown as target details, not as clickable flash targets.
The pre-flight view shows SHA256, mountpoints, risk, model, transport, and size.
Mounted targets remain blocked.

The GUI normally runs without root privileges. When flashing is started, it uses
`pkexec` to run EutherEtcher's hidden writer helper with elevated privileges.

The GUI starts a built-in procedural cyberpunk loop by default. It includes ten
free generated loops, picks one at startup, loops it continuously, and exposes
music on/off plus next-loop controls.

External CC0/CC-BY music can be added through `assets/music/music.toml`. Missing
files are ignored and the generated loops remain the fallback.

## Notes

EutherEtcher uses `lsblk --json` for device discovery because that is the native
structured output available from the Linux toolchain. JSON is not used for
EutherEtcher's own configuration or manifests.
