# EutherEtcher Music Packs

EutherEtcher plays real music files from this directory before falling back to
its built-in generated synthwave loops.

Use `music.toml` in this directory:

```toml
[[track]]
title = "Night Bus Terminal"
author = "Example Artist"
file = "night-bus-terminal.ogg"
license = "CC0"
source = "https://example.invalid/source"
start_offset_seconds = 0.8
```

Supported file types depend on `rodio`/Symphonia support in the local build;
`.ogg`, `.flac`, and `.wav` are the safest choices.

Use `start_offset_seconds` when a track has a silent or slow intro that should
not be heard every time EutherEtcher starts.

Only add music you are allowed to redistribute. Keep artist, license, and source
metadata in the manifest so release packages preserve attribution.

Search order:

- `assets/music/music.toml` in a development checkout
- `$XDG_DATA_HOME/eutheretcher/music/music.toml`
- `~/.local/share/eutheretcher/music/music.toml`
- `/usr/local/share/eutheretcher/music/music.toml`
- `/usr/share/eutheretcher/music/music.toml`
- `assets/music/music.toml` next to the executable
- `../share/eutheretcher/music/music.toml` next to the executable
