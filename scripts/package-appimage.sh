#!/usr/bin/env sh
set -eu

version="${1:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)}"
binary="${EUTHERETCHER_BINARY:-target/release/eutheretcher}"
appdir="dist/EutherEtcher.AppDir"
linuxdeploy="${LINUXDEPLOY:-linuxdeploy-x86_64.AppImage}"

test -x "$binary"
test -x "$linuxdeploy"
rm -rf "$appdir"
mkdir -p "$appdir/usr/share/eutheretcher/music"

"$linuxdeploy" --appimage-extract-and-run \
  --appdir "$appdir" \
  --executable "$binary" \
  --desktop-file packaging/eutheretcher.desktop \
  --icon-file assets/eutheretcher.svg

install -m 0644 assets/music/* "$appdir/usr/share/eutheretcher/music/"
install -m 0644 LICENSE "$appdir/LICENSE"
install -m 0644 README.md "$appdir/README.md"

rm -f "$appdir/AppRun"
cat > "$appdir/AppRun" <<'EOF'
#!/usr/bin/env sh
set -eu
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
export PATH="$here/usr/bin:${PATH:-}"
export XDG_DATA_DIRS="$here/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
if [ "$#" -eq 0 ]; then
  set -- gui
fi
exec "$here/usr/bin/eutheretcher" "$@"
EOF
chmod 0755 "$appdir/AppRun"

ARCH=x86_64 LINUXDEPLOY_OUTPUT_VERSION="$version" "$linuxdeploy" --appimage-extract-and-run \
  --appdir "$appdir" \
  --output appimage
mv EutherEtcher-*.AppImage "dist/EutherEtcher-${version}-x86_64.AppImage"
