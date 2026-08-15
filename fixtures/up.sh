#!/usr/bin/env bash
# Bring up the SSH fixtures and verify each port reaches the distribution it is supposed to.
#
#   ./fixtures/up.sh && SG_REQUIRE_FIXTURES=1 cargo test
#
# The verification step is not ceremony. Container runtimes that proxy published ports (OrbStack,
# Docker Desktop) can keep a stale forward pointing at a previous container, and the failure mode
# is silent: the BusyBox tests pass while actually talking to Debian. Recreating the stack fixes
# it; this script proves it is fixed before any test runs.
set -euo pipefail

cd "$(dirname "$0")"
KEY="$PWD/id_test"

if [[ ! -f $KEY ]]; then
    echo "generating throwaway fixture key"
    ssh-keygen -t ed25519 -f "$KEY" -N "" -C "serverglass-fixture-only" -q
    chmod 600 "$KEY"
fi

# `down` first: recreating from scratch is what refreshes the published-port forwards.
docker compose down --remove-orphans >/dev/null 2>&1 || true
docker compose up -d --build "$@"

expect_distro() {
    local port=$1 want=$2 tries=30
    while (( tries-- > 0 )); do
        got=$(ssh -i "$KEY" -p "$port" \
                  -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
                  -o ConnectTimeout=2 -o LogLevel=ERROR \
                  root@127.0.0.1 '. /etc/os-release && echo "$ID"' 2>/dev/null) && break
        sleep 1
    done
    if [[ ${got:-} != "$want" ]]; then
        echo "FAIL: port $port serves '${got:-nothing}', expected '$want'." >&2
        echo "      A stale published-port forward is the usual cause; rerun this script." >&2
        exit 1
    fi
    echo "  ok  127.0.0.1:$port -> $want"
}

echo "verifying fixtures:"
expect_distro 2222 debian
expect_distro 2223 alpine
echo "fixtures ready"
