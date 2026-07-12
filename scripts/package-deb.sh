#!/usr/bin/env sh
set -eu

version="${1:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)}"
arch="${2:-amd64}"
binary="${EUTHERETCHER_BINARY:-target/release/eutheretcher}"
package_root="dist/eutheretcher_${version}_${arch}"

test -x "$binary"
rm -rf "$package_root"
mkdir -p \
  "$package_root/DEBIAN" \
  "$package_root/usr/bin" \
  "$package_root/usr/share/applications" \
  "$package_root/usr/share/icons/hicolor/scalable/apps" \
  "$package_root/usr/share/eutheretcher/music" \
  "$package_root/usr/share/doc/eutheretcher" \
  "$package_root/usr/share/polkit-1/actions"

install -m 0755 "$binary" "$package_root/usr/bin/eutheretcher"
install -m 0644 packaging/eutheretcher.desktop "$package_root/usr/share/applications/eutheretcher.desktop"
install -m 0644 assets/eutheretcher.svg "$package_root/usr/share/icons/hicolor/scalable/apps/eutheretcher.svg"
install -m 0644 LICENSE "$package_root/usr/share/doc/eutheretcher/copyright"
install -m 0644 README.md "$package_root/usr/share/doc/eutheretcher/README.md"
install -m 0644 assets/music/* "$package_root/usr/share/eutheretcher/music/"
sed 's#/usr/local/bin/eutheretcher#/usr/bin/eutheretcher#g' \
  packaging/dev.euther.EutherEtcher.policy \
  > "$package_root/usr/share/polkit-1/actions/dev.euther.EutherEtcher.policy"

installed_size="$(du -sk "$package_root" | cut -f1)"
cat > "$package_root/DEBIAN/control" <<EOF
Package: eutheretcher
Version: ${version}
Section: utils
Priority: optional
Architecture: ${arch}
Installed-Size: ${installed_size}
Maintainer: ApothicTECH <info@apothictech.com>
Depends: libc6, libasound2, libx11-6, libxcb1, libxkbcommon0, util-linux, policykit-1, mpv
Homepage: https://github.com/NichlasEk/EutherEtcher
Description: Safety-first Linux image writer for USB and SD media
 EutherEtcher writes ISO and IMG files with conservative target checks,
 optional post-write verification, a native GUI, and stage-aware music.
EOF

dpkg-deb --root-owner-group --build "$package_root" "dist/eutheretcher_${version}_${arch}.deb"
