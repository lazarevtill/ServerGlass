#!/usr/bin/env bash
# Build ServerGlass for the iOS Simulator: Rust core, bindings, Xcode project, app.
#
#   ./scripts/build-ios.sh            # build
#   ./scripts/build-ios.sh --run      # build, boot a simulator, install and launch
#
# Device builds additionally need a signing identity; the target `aarch64-apple-ios` is installed
# and the only change is the SDK and CODE_SIGNING settings.
set -euo pipefail

cd "$(dirname "$0")/.."
RUN=${1:-}
GENERATED=apps/shared/ServerGlassFFI/generated
SIM_TARGET=aarch64-apple-ios-sim
DEVICE=${SG_SIM_DEVICE:-iPhone 17 Pro}

echo "==> building sg-ffi for $SIM_TARGET"
cargo build -p sg-ffi --target "$SIM_TARGET"

# Xcode resolves -lsg_ffi from one directory, so the slice for the SDK being built goes there.
mkdir -p target/ios
cp "target/$SIM_TARGET/debug/libsg_ffi.a" target/ios/libsg_ffi.a

echo "==> generating Swift bindings"
# Generated from the host-architecture dylib: the metadata uniffi-bindgen reads is
# architecture-independent, and a simulator .a has no dylib to introspect.
cargo build -p sg-ffi
# `--bin` is not optional: the crate also ships a C# generator for the Windows app, so
# an unqualified `cargo run` is ambiguous and fails.
cargo run -q -p sg-bindgen --bin uniffi-bindgen -- generate \
    --library target/debug/libsg_ffi.dylib \
    --language swift \
    --out-dir "$GENERATED"
mv -f "$GENERATED/sg_ffiFFI.modulemap" "$GENERATED/module.modulemap"

echo "==> generating the Xcode project"
(cd apps/ios && xcodegen generate --quiet)

echo "==> building the app"
xcodebuild \
    -project apps/ios/ServerGlass.xcodeproj \
    -scheme ServerGlass \
    -sdk iphonesimulator \
    -configuration Debug \
    -derivedDataPath target/ios/DerivedData \
    -quiet \
    build

APP="target/ios/DerivedData/Build/Products/Debug-iphonesimulator/ServerGlass.app"
echo "built: $APP"

if [[ $RUN == "--run" ]]; then
    echo "==> booting $DEVICE"
    # `boot` fails when the device is already booted, which is a success for our purposes.
    xcrun simctl boot "$DEVICE" 2>/dev/null || true
    xcrun simctl bootstatus "$DEVICE" -b >/dev/null 2>&1 || true
    xcrun simctl install "$DEVICE" "$APP"
    xcrun simctl launch "$DEVICE" cloud.lazarev.serverglass
    echo "launched on $DEVICE"
fi
