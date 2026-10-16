#!/usr/bin/env bash
# macOS: "Ai Quota Overlay.app" (menu-bar app, no Dock icon) in a dmg, ad-hoc signed.
#   bash scripts/package-mac.sh <rust target> <arm64|x64>
set -euo pipefail
TARGET=$1
ARCH=$2
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
V=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
NAME="Ai Quota Overlay"
WORK=$(mktemp -d)
APP="$WORK/dmg/$NAME.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" dist
cp "target/$TARGET/release/ai-quota-overlay" "$APP/Contents/MacOS/$NAME"

ICONSET="$WORK/icon.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s build/icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) build/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/icon.icns"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>io.github.artmasster.ai-quota-overlay</string>
  <key>CFBundleExecutable</key><string>$NAME</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>$V</string>
  <key>CFBundleShortVersionString</key><string>$V</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

codesign --force --deep -s - "$APP"
ln -s /Applications "$WORK/dmg/Applications"
hdiutil create -volname "$NAME" -srcfolder "$WORK/dmg" -ov -format UDZO "dist/AiQuotaOverlay-$V-mac-$ARCH.dmg"
ls -la dist
