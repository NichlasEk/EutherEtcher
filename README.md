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

## Release

Pushing a tag like `v0.1.0` runs the release workflow and publishes a `.tar.gz`
containing the binary, icon, desktop file, polkit policy, README, and install
script.

## GUI

The GUI is available as a native Linux window:

```bash
cargo run -- gui
```

It uses the same safety checks as the CLI. Flashing from the GUI asks for
administrator authorization through polkit when the final write starts.

The GUI flow is intentionally simple:

- Select an `.iso` or `.img` with the native file picker.
- Or drag-and-drop an `.iso` or `.img` into the window.
- Select a whole-disk USB/SD target card.
- Review the pre-flight confirmation.
- Press `Flash now` and authorize the write through polkit.

Partitions are shown as target details, not as clickable flash targets.
The pre-flight view shows SHA256, mountpoints, risk, model, transport, and size.
Mounted targets remain blocked.
If a target has mounted partitions, the GUI can explicitly request an unmount
through the privileged helper. It never unmounts automatically.

EutherEtcher locks the selected target identity before flashing. If `/dev/sdX`
now points at a different model, size, serial, or WWN than the selected card, the
write is refused and the device list must be refreshed.

The GUI normally runs without root privileges. When flashing is started, it uses
`pkexec` to run EutherEtcher's hidden writer helper with elevated privileges.
The helper reports structured phase, progress, and error messages back to the
GUI for clearer failures.

The GUI ships with a small real OGG music pack, picks one track at startup,
loops it continuously, and exposes music on/off plus next-track controls. On
Linux, real music playback uses `mpv` with PipeWire output and JSON IPC for
live volume control. The old procedural synth loops remain only as a fallback
when no playable external track is found.

### Music Packs

Bundled or external music is configured through a TOML manifest named
`music.toml`. Missing files are ignored and the generated loops remain the
fallback.

For GUI music playback, install `mpv` with PipeWire output support. EutherEtcher
starts it headless with `--ao=pipewire` and controls volume over mpv's JSON IPC.

Example:

```toml
[[track]]
title = "Night Bus Terminal"
author = "Example Artist"
file = "night-bus-terminal.ogg"
license = "CC0"
source = "https://example.invalid/source"
start_offset_seconds = 0.8
```

Paths are relative to the manifest file unless absolute. EutherEtcher searches:

- `assets/music/music.toml` in a development checkout
- `$XDG_DATA_HOME/eutheretcher/music/music.toml`
- `~/.local/share/eutheretcher/music/music.toml`
- `/usr/local/share/eutheretcher/music/music.toml`
- `/usr/share/eutheretcher/music/music.toml`
- bundled `assets/music/music.toml` next to an extracted release binary

Only add music you are allowed to redistribute. Keep artist, license, and source
metadata in the manifest so release packages preserve attribution.

The GUI saves personal audio settings as TOML in
`$XDG_CONFIG_HOME/eutheretcher/gui.toml` or `~/.config/eutheretcher/gui.toml`:

```toml
music_enabled = true
music_volume = 0.12
```

`music_volume` is stored as `0.0` to `1.0`, matching 0-100% in the GUI.

## Notes

EutherEtcher uses `lsblk --json` for device discovery because that is the native
structured output available from the Linux toolchain. JSON is not used for
EutherEtcher's own configuration or manifests.
