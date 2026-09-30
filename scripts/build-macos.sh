#!/usr/bin/env bash
# Build the macOS app: Rust core, then bindings, then Swift.
#
#   ./scripts/build-macos.sh                # debug
#   ./scripts/build-macos.sh release        # optimised
#
# The binding step must run after the Rust build and before the Swift build — uniffi-bindgen reads
# the compiled library's metadata, not the source.
set -euo pipefail

# Applies to Rust's C/assembly dependencies too. Otherwise a newer Xcode quietly compiles the
# bundled crypto library for the build machine's OS, above the app's advertised minimum.
export MACOSX_DEPLOYMENT_TARGET=14.0

cd "$(dirname "$0")/.."
PROFILE="${1:-debug}"
case "$PROFILE" in
    debug|release) ;;
    *) echo "usage: $0 [debug|release]" >&2; exit 1 ;;
esac
# The path `apps/Package.swift` actually consumes. It used to point under `apps/macos/`, which
# nothing reads: the macOS build then compiled the UI against whatever bindings were last written
# by `build-ios.sh`, so a change to the FFI surface reached iOS and silently did not reach macOS.
GENERATED=apps/shared/ServerGlassFFI/generated

echo "==> building sg-ffi ($PROFILE)"
if [[ $PROFILE == release ]]; then
    cargo build --release -p sg-ffi
else
    cargo build -p sg-ffi
fi

echo "==> generating Swift bindings"
# `--bin` is not optional: the crate also ships a C# generator for the Windows app, so
# an unqualified `cargo run` is ambiguous and fails.
cargo run -q -p sg-bindgen --bin uniffi-bindgen -- generate \
    --library "target/$PROFILE/libsg_ffi.dylib" \
    --language swift \
    --out-dir "$GENERATED"

# SwiftPM requires a system-library target's module map to be named exactly `module.modulemap`;
# uniffi-bindgen emits `<name>FFI.modulemap`.
mv -f "$GENERATED/sg_ffiFFI.modulemap" "$GENERATED/module.modulemap"

echo "==> building the app"
(
    cd apps
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
cp "apps/.build/$PROFILE/ServerGlass" "$APP/Contents/MacOS/ServerGlass"

# An installed bundle must never load a core from a development checkout. That can trap in
# addTarget after the checkout's dylib is rebuilt with a different UniFFI record layout.
if otool -L "$APP/Contents/MacOS/ServerGlass" | grep 'libsg_ffi' >/dev/null; then
    echo "error: ServerGlass still dynamically links the Rust core" >&2
    exit 1
fi

# The icon is generated from source (scripts/make-icons.swift) rather than committed as binaries.
echo "==> icon"
swift scripts/make-icons.swift >/dev/null
iconutil -c icns target/ServerGlass.iconset -o "$APP/Contents/Resources/ServerGlass.icns"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>ServerGlass</string>
    <key>CFBundleDisplayName</key><string>ServerGlass</string>
    <key>CFBundleIdentifier</key><string>cloud.lazarev.serverglass</string>
    <key>CFBundleExecutable</key><string>ServerGlass</string>
    <key>CFBundleIconFile</key><string>ServerGlass</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.1</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

VERSION=$(sed -n 's/^version = "\([^"]*\)".*/\1/p' Cargo.toml | head -1)
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP/Contents/Info.plist"

codesign --force --sign - "$APP"
codesign --verify --strict "$APP"

echo "built: $APP"
