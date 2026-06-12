#!/usr/bin/env sh
set -eu

if [ -x ./eutheretcher ]; then
  built_bin="./eutheretcher"
else
  cargo build --release
  built_bin="target/release/eutheretcher"
fi

bin_dir="${HOME}/.local/bin"
icon_dir="${HOME}/.local/share/icons/hicolor/scalable/apps"
desktop_dir="${HOME}/.local/share/applications"
data_dir="${HOME}/.local/share/eutheretcher"
policy_dir="/usr/share/polkit-1/actions"

mkdir -p "$bin_dir" "$icon_dir" "$desktop_dir" "$data_dir/music"

install -m 0755 "$built_bin" "$bin_dir/eutheretcher"
install -m 0644 assets/eutheretcher.svg "$icon_dir/eutheretcher.svg"
install -m 0644 packaging/eutheretcher.desktop "$desktop_dir/eutheretcher.desktop"
for music_file in assets/music/*; do
  if [ -f "$music_file" ]; then
    install -m 0644 "$music_file" "$data_dir/music/$(basename "$music_file")"
  fi
done

if [ "$(id -u)" -eq 0 ]; then
  install -m 0755 "$built_bin" /usr/local/bin/eutheretcher
  install -m 0644 packaging/dev.euther.EutherEtcher.policy "$policy_dir/dev.euther.EutherEtcher.policy"
else
  printf 'Polkit policy not installed. Run this for system integration:\n'
  printf '  sudo install -m 0755 %s /usr/local/bin/eutheretcher\n' "$built_bin"
  printf '  sudo install -m 0644 packaging/dev.euther.EutherEtcher.policy %s/dev.euther.EutherEtcher.policy\n' "$policy_dir"
fi

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$desktop_dir" >/dev/null 2>&1 || true
fi

printf 'Installed EutherEtcher to %s\n' "$bin_dir/eutheretcher"
