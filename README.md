# ServerGlass

Agentless server monitoring and SSH terminal for macOS, Windows 11, Linux and Android.

Nothing is installed on the servers you monitor. ServerGlass opens one SSH connection, reads
`/proc` and `/sys`, and renders it — so any box you can already SSH into is already monitorable.

**Status: v0.** The Rust core and the macOS app are working end to end against real hosts. The
other three apps, the terminal, and the plugin SDK are designed but not yet built — see
[Roadmap](#roadmap) for exactly what is and is not done.

---

## Why this exists

[ServerCat](https://apps.apple.com/app/id1501532023) proved the premise: agentless SSH monitoring
with a dense gauge dashboard is genuinely pleasant to use. Its limits are the opening — Apple-only,
Docker-only (its "Pods" page has no real Kubernetes), a fixed metric set you cannot extend, and no
history or alerting.

ServerGlass keeps the premise and fixes the rest.

## Design

Three invariants, each enforced by a test rather than by good intentions:

1. **Nothing is installed, written or modified on a monitored host.** Requests can read a file,
   list a directory, or run a program that is already there — the request type offers no way to
   express a write. The fixture containers run with a read-only root filesystem.
2. **No sample is ever written to disk.** A bounded in-memory window exists so charts have
   something to draw; retention and alerting belong to whatever system consumes the exported
   samples.
3. **The core owns all logic.** The UIs render state and send commands. Parsing, scheduling, rate
   derivation and connection handling are shared Rust, so four native front-ends stay consistent.

### One round trip per refresh

The obvious implementation opens an SSH channel per metric — two round trips each, which makes a
twenty-collector dashboard unusable over a bastion hop or a satellite link. ServerGlass never does
more than one round trip per refresh, however many collectors are running.

This falls out of splitting the collector trait in two: a source *declares* what it needs, and
separately *parses* what came back.

```
Collector::requests    every enabled source declares what it needs, merged and deduplicated
       ↓
SshSession::batch      one script over one long-lived channel, one framed reply
       ↓
Collector::collect     every source parses the same responses
       ↓
RateEngine             counters become rates using measured elapsed time
       ↓
LiveStore              bounded in-memory window; nothing reaches disk
```

Because no source performs I/O, `/proc/stat` wanted by three collectors is fetched once — and a
sandboxed WebAssembly plugin can implement the same trait as a built-in without being granted any
I/O capability at all. It can only ask; the host decides.

The batch protocol frames each response with a per-connection random nonce, so content on the
monitored host cannot forge a frame boundary — which matters when the file being read is a log
somebody else writes to.

## Getting started

Requirements: Rust 1.85+, and Xcode 15+ for the macOS app.

```bash
./scripts/build-macos.sh          # Rust core → Swift bindings → ServerGlass.app
open target/ServerGlass.app
```

Add a host with the **+** button. SSH agent authentication is the default and the best option:
ServerGlass never sees your key material at all.

### Development

```bash
./fixtures/up.sh                            # Debian + Alpine SSH targets in Docker
SG_REQUIRE_FIXTURES=1 cargo test --workspace
```

`fixtures/up.sh` verifies each published port actually reaches the distribution it should. That
check is not ceremony: container runtimes that proxy published ports can keep a stale forward
pointing at a previous container, and the failure mode is silent — the BusyBox tests pass while
talking to Debian.

`SG_REQUIRE_FIXTURES=1` turns "fixture missing" from a skip into a failure. A skipped test reports
as `ok`, and a whole suite can quietly stop testing anything.

To run the app against a fixture:

```bash
SG_DEMO_HOST="root@127.0.0.1:2222" SG_DEMO_KEY="$PWD/fixtures/id_test" \
  ./target/ServerGlass.app/Contents/MacOS/ServerGlass
```

## Layout

| Crate | What it does |
|---|---|
| `sg-model` | Domain types. No I/O, no async, `serde` only. Defines the `Source` trait everything hangs off. |
| `sg-transport` | russh client, the batched shell protocol, capability detection. |
| `sg-collect` | Built-in collectors: CPU, memory, load, filesystems, disk I/O, network, TCP. |
| `sg-core` | Request merging, rate derivation, the live store, the per-target runtime. |
| `sg-ffi` | UniFFI surface and the UI-shaped view models. |
| `sg-bindgen` | Binding generator, separate so `clap` never reaches the shipped library. |
| `apps/macos` | SwiftUI app. |

Parsers are tested against `/proc` text captured from real containers (`fixtures/capture.sh`),
not hand-written strings — a hand-written fixture encodes what the author *believed* the format
was, which is exactly what a parser bug is made of. Both a GNU and a BusyBox host are covered,
because their `df`, `ps` and `ls` output differ enough that passing on one means little.

## Roadmap

**Done and verified against real hosts**

- Agentless SSH transport, one round trip per refresh, nonce-framed batch protocol
- Capability detection; collectors gate themselves on what the host can actually report
- CPU (per-core and aggregate), memory, load, uptime, filesystems, disk I/O, network, TCP
- Rate derivation with counter-reset and reconnect handling
- Bounded live store, entity tree, connection lifecycle with backoff
- UniFFI bindings; macOS SwiftUI app with the status grid and per-entity cards

**Next**

- Terminal (`alacritty_terminal` in the core, so one implementation serves all four platforms),
  snippets, credential vault
- Declarative probes and Prometheus/OpenMetrics scraping
- Containers and orchestration (Docker, Podman, real Kubernetes), virtualisation and hardware,
  services/databases/logs, network and external checks
- WebAssembly plugin SDK (WIT + wasmtime); plugins are pure functions that request data and never
  perform I/O
- `Sink` exporters, for handing live samples to an external metrics and alerting system
- Windows 11 (WinUI 3), Linux (GTK4, links the core directly with no FFI), Android (Compose)

## Licence

MIT OR Apache-2.0.
