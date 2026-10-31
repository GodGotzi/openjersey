#!/usr/bin/env bash
# Builds OpenJersey, adds it to your Applications as OpenJersey.app with its
# icon, and sets up its configuration folder (~/.config/openjersey):
# settings.toml if there is none, and the example themes in themes/. Files
# that already exist there are never overwritten. The app starts the binary
# in this checkout's target/ folder, so after `git pull && cargo build
# --release` it starts the new version.
#
#   ./install/install-macos.sh                install (or update)
#   ./install/install-macos.sh --no-themes    the same, without the example themes
#   ./install/install-macos.sh --remove       remove OpenJersey.app (your configuration stays)
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$REPO/target/release/openjersey"
APP="$HOME/Applications/OpenJersey.app"

THEMES=1
case "${1:-}" in
    --no-themes) THEMES=0 ;;
    --remove) ;;
    "") ;;
    *) echo "Unknown option: $1 (see the top of this script)" >&2; exit 2 ;;
esac

if [[ "${1:-}" == "--remove" ]]; then
    rm -rf "$APP"
    echo "OpenJersey.app removed (the checkout, its build and your settings are untouched)."
    exit 0
fi

command -v cargo >/dev/null || { echo "Rust is missing: install it from https://rustup.rs and run this again." >&2; exit 1; }

echo "Building OpenJersey (the first build takes a few minutes) ..."
(cd "$REPO" && cargo build --release)

echo "Setting up the configuration ..."
# the program makes the folder, settings.toml and themes/ and tells us where they are
CONFIG="$("$BIN" --config-dir)"
if [[ "$THEMES" == 1 ]]; then
    for theme in "$REPO"/examples/themes/*.toml; do
        target="$CONFIG/themes/$(basename "$theme")"
        if [[ -e "$target" ]]; then
            echo "  keeping your $(basename "$theme")"
        else
            cp "$theme" "$target"
            echo "  added theme $(basename "$theme" .toml)"
        fi
    done
fi

echo "Creating OpenJersey.app ..."
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

# the icon: sips scales the 1024 px picture into the sizes an .icns holds
ICONSET="$(mktemp -d)/OpenJersey.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
    sips -z "$s" "$s" "$REPO/assets/icon-1024.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
    sips -z "$((s * 2))" "$((s * 2))" "$REPO/assets/icon-1024.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/OpenJersey.icns"
rm -rf "$(dirname "$ICONSET")"

# the app is a small script that hands over to the binary
cat > "$APP/Contents/MacOS/OpenJersey" <<EOF
#!/bin/bash
exec "$BIN" "\$@"
EOF
chmod +x "$APP/Contents/MacOS/OpenJersey"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>OpenJersey</string>
    <key>CFBundleDisplayName</key><string>OpenJersey</string>
    <key>CFBundleIdentifier</key><string>org.openjersey.app</string>
    <key>CFBundleExecutable</key><string>OpenJersey</string>
    <key>CFBundleIconFile</key><string>OpenJersey</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>1.0</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
touch "$APP"

echo "Done. OpenJersey is in $HOME/Applications (find it with Spotlight); it runs $BIN."
echo "Settings and your own themes live in $CONFIG."
