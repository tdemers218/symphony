#!/usr/bin/env bash
# Package a release build for one target (ENGINEERING E13.3).
# Usage: scripts/package.sh <version> <target>   (run after cargo build --release --target <target>)
# Produces in dist/:
#   symphony-<version>-<target>.tar.gz|zip     the app, read by the launcher
#   symphony-launcher-<target>.zip|tar.gz      what testers download
set -euo pipefail
VERSION="$1"
TARGET="$2"
BIN="target/$TARGET/release"
EXE=""
[[ "$TARGET" == *windows* ]] && EXE=".exe"
mkdir -p dist
STAGE="$(mktemp -d)"

# App archive: executable at the top level, plus licence and readme.
APP="$STAGE/app"
mkdir -p "$APP"
cp "$BIN/symphony$EXE" "$APP/"
cp LICENSE README.md "$APP/"
PHOSPHOR_LICENSE="crates/symphony-ui/assets/phosphor/LICENSE"
cp "$PHOSPHOR_LICENSE" "$APP/LICENSE-Phosphor.txt"
cp "$PHOSPHOR_LICENSE" "$STAGE/LICENSE-Phosphor.txt"
NAME="symphony-$VERSION-$TARGET"
if [[ "$TARGET" == *windows* ]]; then
  (cd "$APP" && 7z a -tzip "$OLDPWD/dist/$NAME.zip" . > /dev/null)
else
  chmod +x "$APP/symphony"
  tar -C "$APP" -czf "dist/$NAME.tar.gz" .
fi

# Launcher.
L="symphony-launcher-$TARGET"
if [[ "$TARGET" == *apple* ]]; then
  BUNDLE="$STAGE/Symphony Launcher.app"
  mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
  cp "$PHOSPHOR_LICENSE" "$BUNDLE/Contents/Resources/LICENSE-Phosphor.txt"
  cp "$BIN/symphony-launcher" "$BUNDLE/Contents/MacOS/"
  cat > "$BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Symphony Launcher</string>
  <key>CFBundleDisplayName</key><string>Symphony Launcher</string>
  <key>CFBundleIdentifier</key><string>io.github.tdemers218.symphony.launcher</string>
  <key>CFBundleExecutable</key><string>symphony-launcher</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
  ditto -c -k --norsrc --noextattr --keepParent "$BUNDLE" "dist/$L.zip"
elif [[ "$TARGET" == *windows* ]]; then
  cp "$BIN/symphony-launcher.exe" "$STAGE/"
  (cd "$STAGE" && 7z a -tzip "$OLDPWD/dist/$L.zip" symphony-launcher.exe LICENSE-Phosphor.txt > /dev/null)
else
  cp "$BIN/symphony-launcher" "$STAGE/"
  chmod +x "$STAGE/symphony-launcher"
  tar -C "$STAGE" -czf "dist/$L.tar.gz" symphony-launcher LICENSE-Phosphor.txt
fi
rm -rf "$STAGE"
ls -l dist
