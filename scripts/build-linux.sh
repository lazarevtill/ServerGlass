#!/usr/bin/env bash
# Build ServerGlass for Linux: GTK4, linking the Rust core directly.
#
#   ./scripts/build-linux.sh                # debug build
#   ./scripts/build-linux.sh release        # optimised
#   ./scripts/build-linux.sh --run          # build and run it
#   ./scripts/build-linux.sh --install      # install for the current user only
#
# There is no binding step, unlike the Apple and Android builds. The Linux app links `sg-ffi` as an
# ordinary Rust library, so the view models and health verdicts arrive as plain Rust types and
# there is nothing to generate and nothing to keep in sync.
#
# Prerequisites, from the distribution's packages:
#
#   Debian/Ubuntu   sudo apt install libgtk-4-dev libadwaita-1-dev build-essential
#   Fedora          sudo dnf install gtk4-devel libadwaita-devel gcc
#   Arch            sudo pacman -S gtk4 libadwaita base-devel
#
# **GTK 4.10 and libadwaita 1.4 are the floor.** Debian bookworm ships 4.8 and 1.2 and cannot
# build this: AdwToolbarView, AdwNavigationSplitView, AdwSpinRow and GtkFileDialog all arrived
# afterwards. Ubuntu 24.04, Debian trixie, Fedora 39 and any rolling distribution are fine.
set -euo pipefail

cd "$(dirname "$0")/.."
APP=apps/linux

# The app is its own cargo workspace, so the CI container that builds the core is never asked to
# find GTK. See the comment at the top of apps/linux/Cargo.toml.
cd "$APP"

MODE=${1:-debug}
case "$MODE" in
    release) PROFILE=release; FLAGS=(--release) ;;
    --run|--install|debug|"") PROFILE=debug; FLAGS=() ;;
    *) echo "unknown argument: $MODE" >&2; exit 2 ;;
esac

if ! pkg-config --exists gtk4 libadwaita-1; then
    # Saying which package is missing beats a hundred lines of C linker errors.
    echo "error: GTK 4 and libadwaita development packages are required." >&2
    echo "       See the header of this script for the package names." >&2
    exit 1
fi

echo "==> building serverglass ($PROFILE)"
cargo build "${FLAGS[@]}"

BINARY="target/$PROFILE/serverglass"
echo "==> built $APP/$BINARY"

if [[ ${1:-} == --run ]]; then
    exec "$BINARY"
fi

if [[ ${1:-} == --install ]]; then
    # Into the user's own prefix: nothing here needs root, and a monitoring tool asking for it
    # would be a poor advertisement for a program whose whole claim is that it installs nothing.
    PREFIX="${PREFIX:-$HOME/.local}"
    install -Dm755 "$BINARY" "$PREFIX/bin/serverglass"
    install -Dm644 data/cloud.lazarev.ServerGlass.desktop \
        "$PREFIX/share/applications/cloud.lazarev.ServerGlass.desktop"
    echo "==> installed to $PREFIX"
    echo "    If it does not appear in your launcher, $PREFIX/share may not be in XDG_DATA_DIRS."
fi
