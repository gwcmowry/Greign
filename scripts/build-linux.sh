#!/bin/sh
# Build the x86-64 Debian package on Ubuntu 22.04 for the release ABI baseline.
set -eu
cd "$(dirname "$0")/.."
test "$(dpkg --print-architecture)" = amd64
version=${1:?pass the Debian package version}
dpkg --validate-version "$version"
cargo build --locked --release -p greign-app
mkdir -p dist
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
stage="$scratch/package"
docs="$stage/usr/share/doc/greign"
install -Dm755 target/release/greign-app "$stage/usr/bin/greign"
install -Dm644 crates/greign-app/assets/greign.png "$stage/usr/share/icons/hicolor/512x512/apps/greign.png"
install -Dm644 Greign.desktop "$stage/usr/share/applications/greign.desktop"
install -Dm644 LICENSE "$docs/copyright"
install -m644 docs/linux.md "$docs/README.md"
python3 scripts/license-notices.py x86_64-unknown-linux-gnu "$docs/ThirdPartyNotices.txt"
mkdir -p "$stage/DEBIAN"
# Derive ELF dependency versions from this builder's symbols (including glibc).
mkdir -p "$scratch/debian"
printf 'Source: greign\n\nPackage: greign\nArchitecture: amd64\n' > "$scratch/debian/control"
linked=$(cd "$scratch" && dpkg-shlibdeps -O -e"$stage/usr/bin/greign")
linked=${linked#shlibs:Depends=}
test -n "$linked"
cat > "$stage/DEBIAN/control" <<EOF
Package: greign
Version: $version
Architecture: amd64
Section: graphics
Priority: optional
Maintainer: Greign project <gwcmowry@users.noreply.github.com>
Homepage: https://github.com/gwcmowry/Greign
Depends: $linked, libgl1, libegl1, libx11-6, libxcursor1, libxi6, libxrandr2, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, xdg-utils, xdg-desktop-portal
Recommends: xdg-desktop-portal-gtk | xdg-desktop-portal-kde | xdg-desktop-portal-gnome
Description: Film-negative conversion and roll editing
 Import camera scans, convert negatives, adjust individual frames and export positives.
EOF
dpkg-deb --root-owner-group -Zxz -z3 --build "$stage" dist/Greign-linux-amd64.deb
