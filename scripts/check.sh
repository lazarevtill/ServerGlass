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
fi

if [[ "${1:-}" == "--all" ]]; then
    # No runner exists for these, so nothing but a developer checks them.
    step "swift";  swift test --package-path apps
    step "kotlin"; (cd apps/android && gradle -q :app:testDebugUnitTest)
fi

printf '\nAll checks passed.\n'
printf 'The live SSH tests skip themselves without the fixtures.\n'
printf 'To include them: ./fixtures/up.sh && SG_REQUIRE_FIXTURES=1 cargo test --workspace\n'
printf 'The Linux app has live tests too: SG_REQUIRE_FIXTURES=1 cargo test --manifest-path apps/linux/Cargo.toml\n'
