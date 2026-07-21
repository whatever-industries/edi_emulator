#!/usr/bin/env bash
# Lay out the AppDir that appimagetool packages, for either architecture.
# Usage: scripts/make-appdir.sh <path-to-built-binary>
set -euo pipefail
cd "$(dirname "$0")/.."

BIN="${1:?usage: make-appdir.sh <binary>}"

rm -rf AppDir
mkdir -p AppDir/usr/bin \
         AppDir/usr/share/applications \
         AppDir/usr/share/icons/hicolor/256x256/apps

cp "$BIN" AppDir/usr/bin/e-di
chmod +x AppDir/usr/bin/e-di
cp assets/icon_256.png AppDir/usr/share/icons/hicolor/256x256/apps/e-di.png
cp assets/icon_256.png AppDir/e-di.png

cat > AppDir/usr/share/applications/e-di.desktop <<'EOF'
[Desktop Entry]
Name=E-Di
Comment=Emulator Disc Interactive — CD-i emulator
Exec=e-di
Icon=e-di
Type=Application
Categories=Game;Emulator;
StartupWMClass=e-di
EOF
cp AppDir/usr/share/applications/e-di.desktop AppDir/e-di.desktop

cat > AppDir/AppRun <<'EOF'
#!/bin/bash
SELF=$(readlink -f "$0")
HERE=$(dirname "$SELF")
exec "$HERE/usr/bin/e-di" "$@"
EOF
chmod +x AppDir/AppRun

echo "AppDir ready from $BIN"
