#!/usr/bin/env bash
# Build distributable installers and, optionally, publish them to a GitLab release.
#
#   ./scripts/release.sh                 # build artefacts into dist/
#   ./scripts/release.sh --publish v0.2.0
#
# Publishing needs a GitLab token with `api` scope in SG_GITLAB_TOKEN. The token is read from the
# environment and never written to disk or echoed.
#
# What comes out:
#
#   dist/ServerGlass-<version>-macos-arm64.dmg   drag-to-Applications disk image
#   dist/ServerGlass-<version>-android-arm64.apk signed with a local release key
#
# iOS is deliberately absent. An .ipa that anyone can install requires an Apple Developer signing
# identity and a provisioning profile; without one the only possible output is a simulator build,
# which is not an installer. Build it from source with scripts/build-ios.sh.
set -euo pipefail

cd "$(dirname "$0")/.."

PUBLISH=""
VERSION=""
if [[ ${1:-} == "--publish" ]]; then
    PUBLISH=1
    VERSION=${2:?"usage: release.sh --publish <version>, e.g. v0.2.0"}
else
    VERSION=${1:-v$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)}
fi
VERSION=${VERSION#v}

DIST=dist
mkdir -p "$DIST"
echo "==> building ServerGlass $VERSION"

# ---------------------------------------------------------------- macOS .dmg
build_macos() {
    echo "==> macOS"
    SG_PROFILE=release ./scripts/build-macos.sh release >/dev/null

    local app=target/ServerGlass.app
    local dmg="$DIST/ServerGlass-$VERSION-macos-arm64.dmg"
    local staging
    staging=$(mktemp -d)

    cp -R "$app" "$staging/"
    # The conventional drag-to-install layout: the app and a shortcut to /Applications.
    ln -s /Applications "$staging/Applications"

    # Ad-hoc signature. It does not satisfy Gatekeeper — the first launch still needs
    # right-click > Open, or `xattr -dr com.apple.quarantine` — but it does stop macOS treating
    # the binary as damaged, which is what an entirely unsigned bundle looks like.
    codesign --force --deep --sign - "$staging/ServerGlass.app" 2>/dev/null || true

    rm -f "$dmg"
    hdiutil create -volname "ServerGlass" -srcfolder "$staging" -ov -format UDZO "$dmg" >/dev/null
    rm -rf "$staging"
    echo "    $dmg  ($(du -h "$dmg" | cut -f1))"
}

# ------------------------------------------------------------- Android .apk
build_android() {
    echo "==> Android"
    export JAVA_HOME=${JAVA_HOME:-/opt/homebrew/opt/openjdk@21}
    export ANDROID_HOME=${ANDROID_HOME:-$HOME/Library/Android/sdk}
    export ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/27.3.13750724}

    local keystore=release/serverglass.jks
    # A local signing key, generated once and gitignored. Android refuses to install an unsigned
    # APK at all, so there is no "unsigned" option the way there is on macOS. This key identifies
    # *this build machine*; shipping through Play would use an upload key held by Google instead.
    if [[ ! -f $keystore ]]; then
        echo "    generating a local release signing key (gitignored)"
        mkdir -p release
        "$JAVA_HOME/bin/keytool" -genkeypair -v -keystore "$keystore" \
            -alias serverglass -keyalg RSA -keysize 4096 -validity 10000 \
            -storepass serverglass -keypass serverglass \
            -dname "CN=ServerGlass, OU=lazarev.cloud, O=lazarev.cloud, C=GB" >/dev/null 2>&1
    fi

    SG_PROFILE=release ./scripts/build-android.sh >/dev/null

    (
        cd apps/android
        SG_KEYSTORE="$PWD/../../$keystore" gradle --quiet :app:assembleRelease
    )

    local unsigned=apps/android/app/build/outputs/apk/release/app-release-unsigned.apk
    local signed=apps/android/app/build/outputs/apk/release/app-release.apk
    local out="$DIST/ServerGlass-$VERSION-android-arm64.apk"

    if [[ -f $signed ]]; then
        cp "$signed" "$out"
    else
        local aligned="$DIST/.aligned.apk"
        "$ANDROID_HOME/build-tools/36.1.0/zipalign" -f 4 "$unsigned" "$aligned"
        "$ANDROID_HOME/build-tools/36.1.0/apksigner" sign \
            --ks "$keystore" --ks-pass pass:serverglass --key-pass pass:serverglass \
            --out "$out" "$aligned"
        rm -f "$aligned" "$out.idsig"
    fi
    echo "    $out  ($(du -h "$out" | cut -f1))"
}

build_macos
build_android

echo
echo "artefacts in $DIST:"
ls -1 "$DIST"

# ------------------------------------------------------------ GitLab release
if [[ -n $PUBLISH ]]; then
    : "${SG_GITLAB_TOKEN:?set SG_GITLAB_TOKEN to a token with api scope}"
    API=https://gitlab.lazarev.cloud/api/v4/projects/lazarevtill%2Fserverglass

    echo
    echo "==> uploading to GitLab"
    LINKS=""
    for file in "$DIST"/*; do
        url=$(curl -sS --fail -H "PRIVATE-TOKEN: $SG_GITLAB_TOKEN" \
            -F "file=@$file" "$API/uploads" | sed -n 's/.*"full_path":"\([^"]*\)".*/\1/p')
        name=$(basename "$file")
        LINKS="$LINKS{\"name\":\"$name\",\"url\":\"https://gitlab.lazarev.cloud$url\"},"
        echo "    uploaded $name"
    done
    LINKS=${LINKS%,}

    curl -sS --fail -X POST -H "PRIVATE-TOKEN: $SG_GITLAB_TOKEN" \
        -H "Content-Type: application/json" \
        -d "{\"name\":\"ServerGlass $VERSION\",\"tag_name\":\"v$VERSION\",\"ref\":\"main\",
             \"description\":\"Agentless server monitoring for macOS, iOS, iPadOS and Android.\\n\\nmacOS: open the .dmg and drag ServerGlass to Applications. The build is ad-hoc signed, so the first launch needs right-click > Open.\\n\\nAndroid: install the APK; it is signed with a local key, so enable installation from unknown sources.\\n\\niOS: build from source with scripts/build-ios.sh — distributing an .ipa needs an Apple Developer identity.\",
             \"assets\":{\"links\":[$LINKS]}}" \
        "$API/releases" >/dev/null
    echo "    release v$VERSION created"
fi
