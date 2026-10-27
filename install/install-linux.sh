#!/usr/bin/env bash
# Builds OpenJersey and adds it to the app launcher, with its icon. The
# launcher runs the binary in this checkout's target/ folder, so after
# `git pull && cargo build --release` it starts the new version.
#
#   ./install/install-linux.sh            install (or update)
#   ./install/install-linux.sh --remove   remove the launcher entry and icons
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$REPO/target/release/openjersey"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
DESKTOP="$DATA/applications/openjersey.desktop"
ICONS="$DATA/icons/hicolor"
SIZES="16 24 32 48 64 128 256 512"

refresh() {
    command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" || true
    command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -f -t "$ICONS" >/dev/null 2>&1 || true
}

if [[ "${1:-}" == "--remove" ]]; then
    rm -f "$DESKTOP" "$ICONS/scalable/apps/openjersey.svg"
    for s in $SIZES; do rm -f "$ICONS/${s}x${s}/apps/openjersey.png"; done
    refresh
    echo "OpenJersey removed from the launcher (the checkout and its build are untouched)."
    exit 0
fi

command -v cargo >/dev/null || { echo "Rust is missing: install it from https://rustup.rs and run this again." >&2; exit 1; }

echo "Building OpenJersey (the first build takes a few minutes) ..."
(cd "$REPO" && cargo build --release)

echo "Installing the icon ..."
mkdir -p "$ICONS/scalable/apps"
cp "$REPO/assets/icon.svg" "$ICONS/scalable/apps/openjersey.svg"
if command -v magick >/dev/null || command -v convert >/dev/null; then
    resize() { if command -v magick >/dev/null; then magick "$@"; else convert "$@"; fi; }
    for s in $SIZES; do
        mkdir -p "$ICONS/${s}x${s}/apps"
        resize "$REPO/assets/icon-1024.png" -resize "${s}x${s}" "$ICONS/${s}x${s}/apps/openjersey.png"
    done
else
    mkdir -p "$ICONS/256x256/apps"
    cp "$REPO/assets/icon.png" "$ICONS/256x256/apps/openjersey.png"
fi

echo "Adding the launcher entry ..."
mkdir -p "$(dirname "$DESKTOP")"
cat > "$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=OpenJersey
GenericName=Trikot-Designer
Comment=Volleyball-Trikots und Einspielshirts gestalten
Exec=$BIN %f
TryExec=$BIN
Path=$REPO
Icon=openjersey
Terminal=false
Categories=Graphics;2DGraphics;
Keywords=Trikot;Jersey;Volleyball;Shirt;Design;
EOF
refresh

echo "Done. OpenJersey is in your app launcher; it runs $BIN."
