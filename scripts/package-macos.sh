#!/usr/bin/env bash
# Build a universal Cloudboard.app and zip it (also used by the in-app updater).
#   scripts/package-macos.sh [pdfium-version|latest]
# Output: dist/Cloudboard-<ver>-macos-universal.zip
# Optional signing: set MACOS_SIGN_IDENTITY (Developer ID) to sign instead of ad-hoc.
set -euo pipefail
cd "$(dirname "$0")/.."
PDFIUM_VERSION="${1:-latest}"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
echo "Cloudboard $VERSION (macOS universal)"

rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
cargo build --release --locked --target aarch64-apple-darwin -p cloudboard-macos
cargo build --release --locked --target x86_64-apple-darwin -p cloudboard-macos

DIST=dist/macos
APP="$DIST/Cloudboard.app"
rm -rf "$DIST"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/Resources"

lipo -create -output "$APP/Contents/MacOS/cloudboard" \
  target/aarch64-apple-darwin/release/cloudboard \
  target/x86_64-apple-darwin/release/cloudboard
sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"

# Icon
ICONSET="$DIST/cloudboard.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/icons/cloudboard-icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s*2)) $((s*2)) assets/icons/cloudboard-icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/cloudboard.icns"
rm -rf "$ICONSET"

# PDFium (universal dylib)
TMP="$DIST/pdfium"
mkdir -p "$TMP"
if [ "$PDFIUM_VERSION" = "latest" ]; then
  URL="https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-mac-univ.tgz"
else
  URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F$PDFIUM_VERSION/pdfium-mac-univ.tgz"
fi
curl -fsSL "$URL" -o "$TMP/pdfium.tgz"
tar -xzf "$TMP/pdfium.tgz" -C "$TMP"
cp "$TMP/lib/libpdfium.dylib" "$APP/Contents/Frameworks/"
cp "$TMP/LICENSE" "$APP/Contents/Resources/PDFIUM-LICENSE.txt" 2>/dev/null || true
rm -rf "$TMP"

if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
  # Developer ID signing with hardened runtime (notarization-ready).
  codesign --force --options runtime --timestamp -s "$MACOS_SIGN_IDENTITY" "$APP/Contents/Frameworks/libpdfium.dylib"
  codesign --force --options runtime --timestamp -s "$MACOS_SIGN_IDENTITY" "$APP"
else
  # Ad-hoc signature (required to run on Apple silicon; users may need to
  # right-click > Open the first time).
  codesign --force -s - "$APP/Contents/Frameworks/libpdfium.dylib"
  codesign --force -s - "$APP"
fi

ZIP="dist/Cloudboard-$VERSION-macos-universal.zip"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"
echo "Package: $ZIP"
