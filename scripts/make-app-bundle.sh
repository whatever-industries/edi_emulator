#!/usr/bin/env bash
# Assemble a macOS application bundle for the E-Di desktop frontend.
# Output: dist/E-Di.app
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

cargo build --release -p cdi-frontend

APP=dist/E-Di.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/cdi-frontend "$APP/Contents/MacOS/E-Di"
cp assets/E-Di.icns "$APP/Contents/Resources/E-Di.icns"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>E-Di</string>
    <key>CFBundleDisplayName</key><string>E-Di</string>
    <key>CFBundleExecutable</key><string>E-Di</string>
    <key>CFBundleIconFile</key><string>E-Di</string>
    <key>CFBundleIdentifier</key><string>industries.whatever.edi</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

echo "Built $APP (version ${VERSION})"
