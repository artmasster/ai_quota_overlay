#!/usr/bin/env bash
# Linux: an AppImage (updates itself through latest-linux.yml, like the Electron AppImage did) and a .deb that
# replaces the Electron package of the same name.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
V=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
BIN=target/release/ai-quota-overlay
WORK=$(mktemp -d)
mkdir -p dist

DESKTOP='[Desktop Entry]
Name=Ai Quota Overlay
Comment=Claude, Codex and Cursor quota left, always on top
Exec=ai-quota-overlay
Icon=ai-quota-overlay
Type=Application
Categories=Utility;
StartupWMClass=ai-quota-overlay'

# ---------- AppImage ----------
APPDIR="$WORK/AppDir"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/512x512/apps"
install -m 755 "$BIN" "$APPDIR/usr/bin/ai-quota-overlay"
echo "$DESKTOP" > "$APPDIR/usr/share/applications/ai-quota-overlay.desktop"
cp "$APPDIR/usr/share/applications/ai-quota-overlay.desktop" "$APPDIR/"
cp build/icon.png "$APPDIR/usr/share/icons/hicolor/512x512/apps/ai-quota-overlay.png"
cp build/icon.png "$APPDIR/ai-quota-overlay.png"
cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/ai-quota-overlay" "$@"
EOF
chmod +x "$APPDIR/AppRun"
curl -fsSL -o "$WORK/appimagetool" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
chmod +x "$WORK/appimagetool"
ARCH=x86_64 "$WORK/appimagetool" --appimage-extract-and-run "$APPDIR" "dist/AiQuotaOverlay-$V-linux-x86_64.AppImage"

# ---------- deb ----------
PKG="$WORK/deb"
mkdir -p "$PKG/DEBIAN" "$PKG/usr/bin" "$PKG/usr/share/applications" "$PKG/usr/share/icons/hicolor/512x512/apps"
install -m 755 "$BIN" "$PKG/usr/bin/ai-quota-overlay"
echo "$DESKTOP" > "$PKG/usr/share/applications/ai-quota-overlay.desktop"
cp build/icon.png "$PKG/usr/share/icons/hicolor/512x512/apps/ai-quota-overlay.png"
SIZE=$(du -sk "$PKG/usr" | cut -f1)
cat > "$PKG/DEBIAN/control" <<EOF
Package: ai-quota-overlay
Version: $V
Architecture: amd64
Maintainer: artmasster <artmasster@users.noreply.github.com>
Installed-Size: $SIZE
Depends: libx11-6, libxcursor1, libxrandr2, libxi6, libxkbcommon-x11-0
Section: utils
Priority: optional
Homepage: https://github.com/artmasster/ai_quota_overlay
Description: Tiny always-on-top rings showing how much Claude, Codex and Cursor quota you have left
EOF
# The Electron package installed under /opt; its files go when dpkg replaces it.
dpkg-deb --root-owner-group --build "$PKG" "dist/AiQuotaOverlay-$V-linux-amd64.deb"

python3 -I scripts/latest-yml.py "$V" dist/latest-linux.yml "dist/AiQuotaOverlay-$V-linux-x86_64.AppImage"
ls -la dist
