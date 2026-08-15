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

# The release description. Kept out of the JSON literal so quoting stays readable; newlines are
# escaped because the description is embedded in a JSON string.
release_notes() {
    printf '%s' "Agentless server monitoring over SSH. Nothing is installed on the monitored host.\\n\\n\\
**macOS** — open the .dmg and drag ServerGlass to Applications. The build is ad-hoc signed rather \\
than notarised, so the first launch needs right-click > Open.\\n\\n\\
**Android** — install the .apk. Signed with this project's own key, so you may need to allow \\
installs from your browser or file manager.\\n\\n\\
Built from $(git rev-parse --short HEAD)."
}

if [[ -n $PUBLISH ]]; then
    : "${SG_GITLAB_TOKEN:?set SG_GITLAB_TOKEN to a token with api scope}"
    API=https://gitlab.lazarev.cloud/api/v4/projects/lazarevtill%2Fserverglass

    echo
    echo "==> uploading to the package registry"
    # The generic package registry, not /uploads. Project uploads are served to browser sessions;
    # fetching one with an API token returns the login page, so a release whose assets point there
    # is only downloadable by someone already signed in. Package registry files are fetchable with
    # a token, and anonymously when the project is public.
    # Only this version's artefacts. `"$DIST"/*` would re-upload every older build under the new
    # version's path, so 0.1.1's dmg would be published as a 0.1.2 asset.
    LINKS=""
    for file in "$DIST"/ServerGlass-"$VERSION"-*; do
        name=$(basename "$file")
        curl -sS --fail -H "PRIVATE-TOKEN: $SG_GITLAB_TOKEN" \
            --upload-file "$file" \
            "$API/packages/generic/serverglass/$VERSION/$name" >/dev/null
        LINKS="$LINKS{\"name\":\"$name\",\"url\":\"$API/packages/generic/serverglass/$VERSION/$name\",\"link_type\":\"package\"},"
        echo "    uploaded $name"
    done
    LINKS=${LINKS%,}

    # Create the release itself. Without this the files sit in the package registry with nothing
    # pointing at them, and the Releases page stays empty.
    echo "==> creating the release"
    curl -sS --fail -X POST -H "PRIVATE-TOKEN: $SG_GITLAB_TOKEN" \
        -H "Content-Type: application/json" \
        -d "{\"name\":\"v$VERSION\",\"tag_name\":\"v$VERSION\",\"ref\":\"main\",\
             \"description\":\"$(release_notes)\",\"assets\":{\"links\":[$LINKS]}}" \
        "$API/releases" >/dev/null

    echo "    release v$VERSION created: https://gitlab.lazarev.cloud/lazarevtill/serverglass/-/releases/v$VERSION"
fi
