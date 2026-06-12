#!/usr/bin/env sh
set -eu

cargo build --release

bin_dir="${HOME}/.local/bin"
icon_dir="${HOME}/.local/share/icons/hicolor/scalable/apps"
desktop_dir="${HOME}/.local/share/applications"

mkdir -p "$bin_dir" "$icon_dir" "$desktop_dir"

install -m 0755 target/release/eutheretcher "$bin_dir/eutheretcher"
install -m 0644 assets/eutheretcher.svg "$icon_dir/eutheretcher.svg"
install -m 0644 packaging/eutheretcher.desktop "$desktop_dir/eutheretcher.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$desktop_dir" >/dev/null 2>&1 || true
fi

printf 'Installed EutherEtcher to %s\n' "$bin_dir/eutheretcher"
