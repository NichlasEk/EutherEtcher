# EutherEtcher Music Packs

EutherEtcher plays real music files from this directory through headless `mpv`
with PipeWire output before falling back to its built-in generated synthwave
loops. Live GUI volume control uses mpv's JSON IPC socket.

Use `music.toml` in this directory:

```toml
[[track]]
title = "Night Bus Terminal"
cue = "neutral"
author = "Example Artist"
file = "night-bus-terminal.ogg"
license = "CC0"
source = "https://example.invalid/source"
start_offset_seconds = 0.8
```

Bundled tracks can set an explicit workflow `cue`:

- `neutral`
- `image_armed`
- `ready`
- `flashing`
- `success`

If `cue` is omitted, EutherEtcher retains the older title-keyword fallback for
external music packs.

Supported file types depend on the local `mpv` build; `.ogg`, `.flac`, and
`.wav` are the safest choices.

Use `start_offset_seconds` when a track has a silent or slow intro that should
not be heard every time EutherEtcher starts.

Only add music you are allowed to redistribute. Keep artist, license, and source
metadata in the manifest so release packages preserve attribution.

The bundled ApothicTECH pack was generated locally with ACE-Step 1.5 without
third-party reference audio. Prompts, model identifiers, job IDs, mastering
notes, and the CC0 dedication are recorded in
[`ACE_STEP_PROVENANCE.md`](ACE_STEP_PROVENANCE.md).

Only the five tracks referenced by the bundled `music.toml` are shipped in the
release package. Earlier OpenGameArt test tracks are intentionally not bundled.

Search order:

- `assets/music/music.toml` in a development checkout
- `$XDG_DATA_HOME/eutheretcher/music/music.toml`
- `~/.local/share/eutheretcher/music/music.toml`
- `/usr/local/share/eutheretcher/music/music.toml`
- `/usr/share/eutheretcher/music/music.toml`
- `assets/music/music.toml` next to the executable
- `../share/eutheretcher/music/music.toml` next to the executable
