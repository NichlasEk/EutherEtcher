# EutherEtcher

EutherEtcher is a Linux-first, Rust-based CLI prototype for safely writing raw
`.iso` and `.img` images to USB and SD block devices.

The first version is deliberately conservative. It would rather refuse to write
than risk writing to the wrong disk.

## Commands

```bash
eutheretcher list
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

## GUI

The GUI is available as a native Linux window:

```bash
cargo run -- gui
```

It uses the same safety checks as the CLI. Flashing from the GUI still requires
typing the exact target device path.

## Notes

EutherEtcher uses `lsblk --json` for device discovery because that is the native
structured output available from the Linux toolchain. JSON is not used for
EutherEtcher's own configuration or manifests.
