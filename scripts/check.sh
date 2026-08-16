#!/usr/bin/env bash
# Everything CI checks, in one command. The Unix twin of scripts/check.ps1.
#
#   ./scripts/check.sh            core only
#   ./scripts/check.sh --all      core, plus the Swift and Kotlin suites CI cannot run
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n==> %s\n' "$1"; }

step "format";  cargo fmt --all -- --check
step "clippy";  cargo clippy --workspace --all-targets --locked -- -D warnings
step "build";   cargo build --workspace --locked
step "test";    cargo test --workspace --locked

if [[ "${1:-}" == "--all" ]]; then
    # No runner exists for these, so nothing but a developer checks them.
    step "swift";  swift test --package-path apps
    step "kotlin"; (cd apps/android && gradle -q :app:testDebugUnitTest)
fi

printf '\nAll checks passed.\n'
printf 'The live SSH tests skip themselves without the fixtures.\n'
printf 'To include them: ./fixtures/up.sh && SG_REQUIRE_FIXTURES=1 cargo test --workspace\n'
