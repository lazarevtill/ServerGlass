#!/usr/bin/env bash
# Refresh the /proc corpora that the collector unit tests parse.
#
#   ./fixtures/up.sh && ./fixtures/capture.sh
#
# Parsers are tested against real kernel output rather than hand-written strings, because a
# hand-written fixture encodes what the author believed the format was — which is exactly what a
# parser bug is made of.
set -euo pipefail

cd "$(dirname "$0")"
OUT=proc-corpus

FILES=(stat meminfo loadavg uptime net/dev net/snmp net/sockstat diskstats mounts)

for host in debian alpine; do
    container="sg-fixture-$host"
    if ! docker inspect "$container" >/dev/null 2>&1; then
        echo "FAIL: $container is not running; run ./fixtures/up.sh first" >&2
        exit 1
    fi

    mkdir -p "$OUT/$host"
    for f in "${FILES[@]}"; do
        # /proc/net/dev is stored as net-dev; the test helper derives the name the same way.
        dest="$OUT/$host/${f//\//-}"
        docker exec "$container" cat "/proc/$f" > "$dest"
    done
    docker exec "$container" df -P -k > "$OUT/$host/df-P-k"

    echo "captured $(ls -1 "$OUT/$host" | wc -l | tr -d ' ') files from $host"
done
