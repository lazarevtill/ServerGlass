#!/usr/bin/env bash
# Build ServerGlass for Android: Rust core -> Kotlin bindings -> APK.
#
#   ./scripts/build-android.sh              # build the debug APK
#   ./scripts/build-android.sh --run        # ...and install it on a running emulator
#
# Prerequisites, all installed into user space with no sudo:
#   brew install openjdk@21 gradle
#   brew install --cask android-commandlinetools
#   sdkmanager platform-tools "platforms;android-35" "build-tools;36.1.0" \
#              "ndk;27.3.13750724" emulator "system-images;android-36;google_apis;arm64-v8a"
#   cargo install cargo-ndk && rustup target add aarch64-linux-android
set -euo pipefail

cd "$(dirname "$0")/.."

export JAVA_HOME=${JAVA_HOME:-/opt/homebrew/opt/openjdk@21}
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Library/Android/sdk}
export ANDROID_SDK_ROOT=$ANDROID_HOME
export ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/27.3.13750724}
export PATH="$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$PATH"

RUN=${1:-}
APP=apps/android/app
ABI=arm64-v8a
# SG_PROFILE=release strips symbols and optimises. It is the difference between a 75 MB APK and a
# shippable one, so scripts/release.sh always sets it.
PROFILE=${SG_PROFILE:-debug}

echo "==> cross-compiling sg-ffi for $ABI ($PROFILE)"
# minSdk 26 must match app/build.gradle.kts: the NDK links against that API level's libc.
if [[ $PROFILE == release ]]; then
    cargo ndk -t "$ABI" -P 26 -o "$APP/src/main/jniLibs" build --release -p sg-ffi
else
    cargo ndk -t "$ABI" -P 26 -o "$APP/src/main/jniLibs" build -p sg-ffi
fi

echo "==> generating Kotlin bindings"
# Generated from the host dylib: uniffi reads architecture-independent metadata, and there is no
# .so to introspect on the build machine's own architecture.
cargo build -p sg-ffi
# `--bin` is not optional: the crate also ships a C# generator for the Windows app, so
# an unqualified `cargo run` is ambiguous and fails.
cargo run -q -p sg-bindgen --bin uniffi-bindgen -- generate \
    --library target/debug/libsg_ffi.dylib \
    --language kotlin \
    --out-dir "$APP/build/generated/uniffi"

echo "==> building the APK"
(cd apps/android && gradle --quiet :app:assembleDebug)

APK="$APP/build/outputs/apk/debug/app-debug.apk"
echo "built: $APK"

if [[ $RUN == "--run" ]]; then
    if ! adb devices | grep -q "emulator.*device"; then
        echo "no emulator running. Start one with:" >&2
        echo "  \$ANDROID_HOME/emulator/emulator -avd sg_fold &" >&2
        exit 1
    fi
    echo "==> installing"
    adb install -r "$APK"
    adb shell am start -n cloud.lazarev.serverglass/.MainActivity
    echo "launched"
fi
