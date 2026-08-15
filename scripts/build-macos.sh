#!/usr/bin/env bash
# Build the macOS app: Rust core, then bindings, then Swift.
#
#   ./scripts/build-macos.sh                # debug
#   ./scripts/build-macos.sh release        # optimised
#
# The binding step must run after the Rust build and before the Swift build — uniffi-bindgen reads
# the compiled library's metadata, not the source.
set -euo pipefail

cd "$(dirname "$0")/.."
PROFILE="${1:-debug}"
GENERATED=apps/macos/Sources/ServerGlassFFI/generated

echo "==> building sg-ffi ($PROFILE)"
if [[ $PROFILE == release ]]; then
    cargo build --release -p sg-ffi
else
    cargo build -p sg-ffi
fi

echo "==> generating Swift bindings"
cargo run -q -p sg-bindgen -- generate \
    --library "target/$PROFILE/libsg_ffi.dylib" \
    --language swift \
    --out-dir "$GENERATED"

# SwiftPM requires a system-library target's module map to be named exactly `module.modulemap`;
# uniffi-bindgen emits `<name>FFI.modulemap`.
mv -f "$GENERATED/sg_ffiFFI.modulemap" "$GENERATED/module.modulemap"

echo "==> building the app"
(
    cd apps/macos
    if [[ $PROFILE == release ]]; then
        SG_PROFILE=release swift build -c release
    else
        swift build
    fi
)

# Wrap the executable in a real bundle. Without an Info.plist macOS runs the binary as an
# accessory process — it starts, owns no windows, and never comes to the front.
echo "==> packaging ServerGlass.app"
APP="target/ServerGlass.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "apps/macos/.build/$PROFILE/ServerGlass" "$APP/Contents/MacOS/ServerGlass"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>ServerGlass</string>
    <key>CFBundleDisplayName</key><string>ServerGlass</string>
    <key>CFBundleIdentifier</key><string>cloud.lazarev.serverglass</string>
    <key>CFBundleExecutable</key><string>ServerGlass</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

echo "built: $APP"
