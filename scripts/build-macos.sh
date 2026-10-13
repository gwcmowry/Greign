#!/bin/sh
# Produce a native app, ZIP and drag-to-Applications disk image on a Mac.
set -eu
cd "$(dirname "$0")/.."
test "$(uname -s)" = Darwin || { echo "Run this script on macOS." >&2; exit 1; }
target=${1:-$(rustc -vV | sed -n 's/^host: //p')}
case "$target" in
    aarch64-apple-darwin) arch=arm64 ;;
    x86_64-apple-darwin) arch=intel ;;
    *) echo "Unsupported Mac target: $target" >&2; exit 1 ;;
esac
export MACOSX_DEPLOYMENT_TARGET=12.0
cargo build --locked --release -p greign-app --target "$target"
mkdir -p dist
stage=$(mktemp -d "$PWD/dist/macos-$arch.XXXXXX")
app="$stage/Greign.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$target/release/greign-app" "$app/Contents/MacOS/greign-app"
cp LICENSE docs/third-party.md "$app/Contents/Resources/"
python3 scripts/license-notices.py "$target" "$app/Contents/Resources/ThirdPartyNotices.txt"
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Greign</string>
<key>CFBundleDisplayName</key><string>Greign</string>
<key>CFBundleIdentifier</key><string>com.gwcmowry.greign</string>
<key>CFBundleExecutable</key><string>greign-app</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>CFBundleVersion</key><string>$version</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>
EOF
plutil -lint "$app/Contents/Info.plist"
# Local ad-hoc signing needs no Apple account. These builds are not notarized.
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"
otool -L "$app/Contents/MacOS/greign-app" > "$stage/linked-libraries.txt"
if grep -E '^[[:space:]]+/(opt/homebrew|usr/local|Users)/' "$stage/linked-libraries.txt"; then
    echo "App links to a build-machine library; refusing to package." >&2
    exit 1
fi
cp docs/macos.md "$stage/Read Me.txt"
ln -s /Applications "$stage/Applications"
ditto -c -k --sequesterRsrc --keepParent "$app" "dist/Greign-macos-$arch.zip"
hdiutil create -volname Greign -srcfolder "$stage" -ov -format UDZO "dist/Greign-macos-$arch.dmg"
printf '%s\n' "$app" > "dist/app-path-$arch.txt"
echo "Built dist/Greign-macos-$arch.dmg and .zip"
