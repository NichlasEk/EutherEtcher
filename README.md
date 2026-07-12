# EutherEtcher

EutherEtcher is a Linux-first, Rust-based image writer for safely flashing raw
`.iso` and `.img` files to USB and SD block devices.

The project is currently an alpha. It is intentionally conservative: it would
rather refuse to write than risk writing to the wrong disk.

## Status

Recommended release label: `v0.1.0-alpha`.

What works today:

- CLI image flashing and verification.
- Native Linux GUI.
- Device discovery through `lsblk --json`.
- GUI hotplug refresh for newly attached or removed USB/SD devices.
- Removable USB/SD targets highlighted by default.
- Loop devices hidden by default.
- Internal SATA/NVMe drives hidden unless explicitly requested.
- Internal SATA/NVMe drives marked as `DANGER`.
- Mounted devices blocked by default.
- Whole-disk target identity locked before writing.
- GUI flashing through polkit instead of running the full GUI as root.
- Optional verify-after-write.
- Cyberpunk visual layer with real OGG music playback through mpv/PipeWire.

What still needs real hardware validation before a non-alpha release:

- Repeated flash tests across multiple USB/SD devices.
- Cancel behavior during long writes and verify passes.
- More distro coverage for polkit agents and desktop environments.
- Installer/package testing outside the development machine.

## Requirements

Runtime requirements:

- Linux
- `lsblk` from `util-linux`
- `sync`
- `pkexec` from polkit for GUI flashing as a normal user
- A running graphical polkit agent for GUI authorization prompts
- `mpv` with PipeWire output support for GUI music playback
- PipeWire for the default GUI music path

Build requirements:

- Rust stable toolchain
- Cargo

Common package names:

```bash
# Arch
sudo pacman -S rust cargo util-linux polkit mpv pipewire

# Debian/Ubuntu
sudo apt install cargo rustc util-linux policykit-1 mpv pipewire
```

Package names differ between distributions. On some desktops the polkit agent is
provided by the desktop shell; on lighter window managers it may need to be
started separately.

## Commands

```bash
eutheretcher --version
eutheretcher list
eutheretcher list --show-internal-drives
eutheretcher list --show-loops
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
- Requires confirmation before writing.
- Refuses when the image is larger than the target device.
- Runs `sync` after writing.
- Supports a dry-run mode before writing.
- Hides loop devices by default.
- Hides internal SATA/NVMe drives unless explicitly requested.
- Marks internal SATA/NVMe drives as `DANGER`.
- Highlights removable USB/SD-style targets as the normal path.

Run `eutheretcher list` and inspect the device path carefully before flashing.
For the GUI, select only whole-disk removable targets, not partitions.

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
cargo build --release
```

Run from the development checkout:

```bash
cargo run -- --version
cargo run -- list
cargo run -- gui
```

Run checks before publishing:

```bash
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

## Install Desktop App

```bash
cargo build --release
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

It uses the same safety checks as the CLI. Flashing from the GUI asks for
administrator authorization through polkit when the final write starts.

The GUI flow is intentionally simple:

- Select an `.iso` or `.img` with the native file picker.
- Or drag-and-drop an `.iso` or `.img` into the window.
- Select a whole-disk USB/SD target card.
- Review the pre-flight confirmation.
- Press `Flash now` and authorize the write through polkit.

Partitions are shown as target details, not as clickable flash targets. The
pre-flight view shows SHA256 status, mountpoints, risk, model, transport, and
size. Mounted targets remain blocked. If a target has mounted partitions, the
GUI can explicitly request an unmount through the privileged helper. It never
unmounts automatically.

EutherEtcher locks the selected target identity before flashing. If `/dev/sdX`
now points at a different model, size, serial, or WWN than the selected card, the
write is refused and the device list must be refreshed.

The GUI normally runs without root privileges. When flashing is started, it uses
`pkexec` to run EutherEtcher's hidden writer helper with elevated privileges.
The helper reports structured phase, progress, and error messages back to the
GUI for clearer failures.

The GUI refreshes the block device list automatically while idle, so USB/SD
devices attached after startup should appear without restarting the app. Manual
refresh is still available through the `Refresh devices` button.

## Music And Visuals

The GUI ships with a small real OGG music pack, loops music continuously, and
exposes music on/off, next-track, and volume controls. Music changes with the
workflow state: neutral idle, image armed, ready to flash, active
write/verification, and successful completion each get their own cue.

On Linux, music playback uses `mpv` with PipeWire output and JSON IPC for live
volume control. The old procedural synth loops remain only as a fallback when no
playable external track is found.

The neon stream visualizer analyzes the currently selected OGG file in the
background and drives the GUI animation from the decoded audio envelope. While a
track is being analyzed, the GUI uses a lightweight fallback animation.

### Music Packs

Bundled or external music is configured through a TOML manifest named
`music.toml`. Missing files are ignored and the generated loops remain the
fallback.

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

## Flash Test Plan

Use this before tagging an alpha release:

1. Build release binary.

   ```bash
   cargo build --release
   ```

2. Confirm dependencies.

   ```bash
   command -v lsblk
   command -v pkexec
   command -v mpv
   ```

3. List devices and confirm the intended target is removable.

   ```bash
   target/release/eutheretcher list
   ```

4. Start the GUI, then attach and remove a USB/SD device to confirm hotplug
   refresh updates the target list without restarting.

   ```bash
   target/release/eutheretcher gui
   ```

5. Run a CLI dry-run.

   ```bash
   target/release/eutheretcher flash --image ./file.iso --device /dev/sdX --dry-run
   ```

6. Run the GUI from a normal user session.

   ```bash
   target/release/eutheretcher gui
   ```

7. Flash a real USB/SD target and confirm the polkit prompt appears.

8. Let verify-after-write complete at least once.

9. Re-list devices after flashing and confirm the target identity still matches
   what was selected.

## Known Issues

- EutherEtcher is Linux-first and currently depends on Linux block device tools.
- The GUI music path expects `mpv --ao=pipewire`; without it, music may fall
  back to generated audio or stay silent depending on the local audio stack.
- A graphical polkit agent must be running for GUI authorization prompts.
- Device naming such as `/dev/sdX` can change when hardware is unplugged and
  replugged. EutherEtcher mitigates this by checking target identity before
  writing, but users should still refresh and re-check devices.
- The GUI is an alpha interface and needs broader distro testing before a stable
  release.

## Release

Pushing a tag like `v0.1.0-alpha.3` runs the release workflow and publishes a
`.tar.gz` containing the binary, icon, desktop file, polkit policy, README,
license, music pack, and install script.

Suggested alpha release flow:

```bash
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
git tag v0.1.0-alpha.3
git push origin v0.1.0-alpha.3
```

## License

EutherEtcher source code is available under the [MIT License](LICENSE).

The bundled ApothicTECH ACE-Step music pack is dedicated under CC0-1.0 for any
copyright and related rights ApothicTECH may hold. Its generation and mastering
provenance is documented in
[`assets/music/ACE_STEP_PROVENANCE.md`](assets/music/ACE_STEP_PROVENANCE.md).

Third-party Rust dependencies remain under their respective licenses.

## Notes

EutherEtcher uses `lsblk --json` for device discovery because that is the native
structured output available from the Linux toolchain. JSON is not used for
EutherEtcher's own configuration or manifests.
