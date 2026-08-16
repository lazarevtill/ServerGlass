#!/usr/bin/env bash
# Everything CI checks, in one command. The Unix twin of scripts/check.ps1.
#
#   ./scripts/check.sh            core, plus the Linux app when GTK is installed
#   ./scripts/check.sh --all      also the Swift and Kotlin suites CI cannot run
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n==> %s\n' "$1"; }

step "format";  cargo fmt --all -- --check
step "clippy";  cargo clippy --workspace --all-targets --locked -- -D warnings
step "build";   cargo build --workspace --locked
step "test";    cargo test --workspace --locked

# The Linux app is its own cargo workspace — the core has to keep building on a machine with no
# desktop, and making it a member would put libgtk-4-dev in the way of every Rust job. It is
# checked here rather than left to CI alone because this is where somebody is actually editing it.
if pkg-config --exists gtk4 libadwaita-1; then
    step "linux app"
    (
        cd apps/linux
        cargo fmt -- --check
        cargo clippy --all-targets --locked -- -D warnings
        cargo test --locked
    )
elif [[ -n "${SG_REQUIRE_LINUX_APP:-}" ]]; then
    # The same reasoning as SG_REQUIRE_FIXTURES: a check that quietly does not run reports success,
    # and that is how a suite stops testing anything. CI sets this.
    printf '\nerror: SG_REQUIRE_LINUX_APP is set but GTK 4 and libadwaita were not found.\n' >&2
    exit 1
else
    printf '\n==> linux app: SKIPPED, no GTK 4 development packages\n'
    printf '    Install them (see scripts/build-linux.sh) or set SG_REQUIRE_LINUX_APP=1 to fail here.\n'
    # One thing is still checked without a toolkit: that apps/linux/Cargo.lock is not stale.
    # It is a separate lockfile over the same crates, so adding a dependency to sg-ffi leaves it
    # behind, and `linux:app` then fails on `--locked` — several pipelines after the change, since
    # nothing a developer without GTK runs would have noticed. Resolving needs no GTK at all.
    step "linux app lockfile"
    cargo metadata --locked --manifest-path apps/linux/Cargo.toml --format-version 1 >/dev/null
fi

# Every generator call has to name its binary. `sg-bindgen` ships two, the uniffi one for Apple and
# Android and the C# one for Windows, so a call that does not say which is ambiguous and fails. It
# broke the macOS, iOS and Android builds at once when the second binary landed, and nothing caught
# it because no test builds an app.
step "build scripts name their generator"
if grep -rn 'cargo run.*sg-bindgen' scripts/build-*.sh scripts/build-*.ps1 | grep -v -- '--bin'; then
    echo "the calls above must pass --bin: sg-bindgen has more than one binary" >&2
    exit 1
fi

# The version lives in five places and F-Droid rejects a release whose tag, manifest and recipe
# disagree — the kind of failure that arrives days later as a rejected merge request rather than as
# a build error here. It also insists a versionCode is never reused and that a changelog exists for
# each one, so both are checked while we are counting.
step "versions agree"
{
    gradle_file=apps/android/app/build.gradle.kts
    recipe=docs/fdroid/cloud.lazarev.serverglass.yml
    vn=$(sed -n 's/.*versionName = "\(.*\)".*/\1/p' "$gradle_file")
    vc=$(sed -n 's/.*versionCode = \([0-9]*\).*/\1/p' "$gradle_file")
    fail=0
    check_version() {
        [[ "$2" == "$vn" ]] || { echo "  $1 says $2, $gradle_file says $vn" >&2; fail=1; }
    }
    check_version Cargo.toml            "$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
    check_version apps/linux/Cargo.toml "$(sed -n 's/^version = "\(.*\)"/\1/p' apps/linux/Cargo.toml | head -1)"
    grep -q "versionName: $vn" "$recipe" || { echo "  $recipe does not build versionName $vn" >&2; fail=1; }
    grep -q "versionCode: $vc" "$recipe" || { echo "  $recipe does not build versionCode $vc" >&2; fail=1; }
    grep -q "commit: v$vn"     "$recipe" || { echo "  $recipe does not pin the tag v$vn" >&2; fail=1; }
    changelog=fastlane/metadata/android/en-US/changelogs/$vc.txt
    if [[ ! -f $changelog ]]; then
        echo "  $changelog is missing: F-Droid needs one per versionCode" >&2
        fail=1
    elif [[ $(wc -m < "$changelog") -gt 500 ]]; then
        # Silently truncated past 500, so the end of the note simply vanishes from the listing.
        echo "  $changelog is $(wc -m < "$changelog") characters; F-Droid truncates past 500" >&2
        fail=1
    fi
    [[ $fail -eq 0 ]] || exit 1
}

if [[ "${1:-}" == "--all" ]]; then
    # No runner exists for these, so nothing but a developer checks them.
    step "swift";  swift test --package-path apps
    step "kotlin"; (cd apps/android && gradle -q :app:testDebugUnitTest)
fi

printf '\nAll checks passed.\n'
printf 'The live SSH tests skip themselves without the fixtures.\n'
printf 'To include them: ./fixtures/up.sh && SG_REQUIRE_FIXTURES=1 cargo test --workspace\n'
printf 'The Linux app has live tests too: SG_REQUIRE_FIXTURES=1 cargo test --manifest-path apps/linux/Cargo.toml\n'
