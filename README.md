# ServerGlass

Agentless server monitoring for macOS, iOS, iPadOS and Android.

Nothing is installed on the servers you monitor. ServerGlass opens one SSH connection, reads
`/proc` and `/sys`, and renders it — so any box you can already SSH into is already monitorable.

The default screen is written for someone who does not know what SSH is: a plain-language verdict,
three readings that mean something without training, and the names of whatever is working hardest.
Every other number is one tap away.

---

## Status

| Platform | State |
|---|---|
| **macOS** | Built. Sidebar, fleet grid, per-host detail. |
| **iOS / iPadOS** | Built. Navigation stack on a phone, two-column split on iPad. |
| **Android** | Built. Foldable-aware two-pane layout. |
| Windows 11 | Not started. WinUI 3 planned. |
| Linux | Not started. GTK4, would link the core directly with no FFI. |

All of them share one Rust core: the collectors, the scheduler, the rate maths, the health
verdicts, the wording and the number formatting are written once.

**194 tests**, including live runs against Debian and Alpine SSH fixtures, and a regression test
asserting that a full refresh costs exactly one network round trip.

## Why this exists

[ServerCat](https://apps.apple.com/app/id1501532023) proved the premise: agentless SSH monitoring
with a dense gauge dashboard is genuinely pleasant. Its limits are the opening — Apple-only,
Docker-only (its "Pods" page has no real Kubernetes), a fixed metric set you cannot extend, and no
history or alerting.

ServerGlass keeps the premise and widens it.

## Design

Four invariants, each held by a test rather than by good intentions:

1. **Nothing is installed, written or modified on a monitored host.** Requests can read a file,
   list a directory, or run a program that is already there — the request type offers no way to
   express a write. The fixture containers run with a read-only root filesystem.
2. **No sample is ever written to disk.** A bounded in-memory window exists so charts have
   something to draw; retention and alerting belong to whatever system consumes the exported
   samples.
3. **The core owns all logic.** The UIs render state and send commands. Parsing, scheduling, rate
   derivation, health assessment and wording are shared Rust.
4. **The widget must match the metric.** A ring implies a proportion; drawing one for "context
   switches per second" tells the reader nothing. See [docs/DESIGN.md](docs/DESIGN.md).

### One round trip per refresh

The obvious implementation opens an SSH channel per metric — two round trips each, which makes a
twenty-collector dashboard unusable over a bastion hop. ServerGlass never does more than one round
trip per refresh, however many collectors are running.

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

Full reasoning in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Building

Requirements: Rust 1.85+. Then per platform:

```bash
./scripts/build-macos.sh          # Rust core → Swift bindings → ServerGlass.app
./scripts/build-ios.sh --run      # ...and launch in a booted simulator
./scripts/build-android.sh --run  # ...and install on a running emulator
```

Each script does the same three steps in order: compile the Rust core for the target, generate the
bindings *from the compiled library*, then build the app. Running the platform build tool on its
own only works if those first two steps already happened.

<details>
<summary>Toolchain setup (macOS host, no <code>sudo</code> required)</summary>

```bash
# iOS
rustup target add aarch64-apple-ios-sim aarch64-apple-ios
brew install xcodegen

# Android
brew install openjdk@21 gradle
brew install --cask android-commandlinetools
sdkmanager "platform-tools" "platforms;android-35" "build-tools;36.1.0" \
           "ndk;27.3.13750724" "emulator" "system-images;android-36;google_apis;arm64-v8a"
cargo install cargo-ndk
rustup target add aarch64-linux-android
```

Everything installs into user space. `scripts/build-android.sh` expects `JAVA_HOME`,
`ANDROID_HOME` and `ANDROID_NDK_HOME`, and defaults them to the paths the above produces.

</details>

## Using it

Add a host with **+**. SSH agent authentication is the default and the best option: ServerGlass
never sees your key material at all. A private key file or a password also work.

The app opens on the plain-language summary. **Show every reading** switches to the full technical
dashboard — per-core CPU, memory breakdown, per-interface traffic, per-device disk I/O,
filesystems, sockets and TCP — and the choice is remembered.

### Where your servers are kept

The list of servers survives closing the app, and is split in two on every platform:

| | Apple | Android |
|---|---|---|
| Address, port, username, sign-in method | `UserDefaults` | `SharedPreferences` |
| Password or key passphrase | Keychain, `AfterFirstUnlockThisDeviceOnly` | `EncryptedSharedPreferences`, key in the Keystore |

Secrets never sit beside the rest of the host record, and are fetched per connection rather than
held in memory for the life of the app. The Apple side is marked device-only and non-syncing: a
server password should not travel with an iCloud restore. Removing a server erases its secret with
it.

This is the one deliberate exception to "the core owns all logic". The Keychain and the Keystore
are operating-system facilities backed by hardware that Rust cannot reach from inside the app;
reimplementing them in the core would mean inventing key management instead of using the one the
platform already audits. The core stays stateless about secrets and is handed one per connection.

Where the kernel supports it (4.20+ with `CONFIG_PSI`), ServerGlass reads
[Pressure Stall Information](https://docs.kernel.org/accounting/psi.html) and prefers it for the
health verdict. A host can sit at 100% CPU and be perfectly healthy — that is what a server is for
— while a host at 30% CPU whose tasks stall on I/O a third of the time is genuinely unwell. Only
pressure tells those apart, so "Waiting on storage" outranks any utilisation percentage.

## Development

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

To run an app against a fixture:

```bash
# macOS
SG_DEMO_HOST="root@127.0.0.1:2222" SG_DEMO_KEY="$PWD/fixtures/id_test" \
  ./target/ServerGlass.app/Contents/MacOS/ServerGlass

# iOS Simulator
SIMCTL_CHILD_SG_DEMO_HOST="root@127.0.0.1:2222" SIMCTL_CHILD_SG_DEMO_KEY="$PWD/fixtures/id_test" \
  xcrun simctl launch "iPhone 17 Pro" cloud.lazarev.serverglass

# Android emulator — 10.0.2.2 is the host machine as seen from inside the emulator
adb push fixtures/id_test /data/local/tmp/id_test && adb shell chmod 644 /data/local/tmp/id_test
adb shell am start -n cloud.lazarev.serverglass/.MainActivity \
  -e host "root@10.0.2.2:2222" -e key /data/local/tmp/id_test
```

## Layout

| Path | What it is |
|---|---|
| `crates/sg-model` | Domain types and the `Source` trait. No I/O, no async, `serde` only. |
| `crates/sg-transport` | russh client, the batched shell protocol, capability detection. |
| `crates/sg-collect` | Collectors: CPU, memory, load, pressure, filesystems, disk I/O, network, TCP, processes. |
| `crates/sg-core` | Request merging, rate derivation, the live store, the per-target runtime. |
| `crates/sg-ffi` | UniFFI surface, UI-shaped view models, and the plain-language layer. |
| `crates/sg-bindgen` | Binding generator, separate so `clap` never reaches the shipped library. |
| `apps/shared/ServerGlassUI` | Every SwiftUI view, shared verbatim by macOS and iOS. |
| `apps/shared/ServerGlassFFI` | Generated Swift bindings. Never edited by hand. |
| `apps/macos`, `apps/ios`, `apps/android` | Per-platform entry points and build config. |
| `fixtures/` | Docker SSH targets and captured `/proc` corpora. |

The SwiftPM manifest lives at `apps/Package.swift` rather than under `apps/macos/`, because
SwiftPM refuses targets outside the package root and the UI target is shared.

Parsers are tested against `/proc` text captured from real containers (`fixtures/capture.sh`),
not hand-written strings — a hand-written fixture encodes what the author *believed* the format
was, which is exactly what a parser bug is made of. Both a GNU and a BusyBox host are covered.

## Roadmap

**Next**

- Terminal (`alacritty_terminal` in the core, so one implementation serves every platform),
  snippets, SFTP file browsing
- Declarative probes and Prometheus/OpenMetrics scraping
- Containers and orchestration (Docker, Podman, real Kubernetes), virtualisation and hardware,
  services/databases/logs, network and external checks
- WebAssembly plugin SDK (WIT + wasmtime); plugins are pure functions that request data and never
  perform I/O
- `Sink` exporters, for handing live samples to an external metrics and alerting system
- Windows 11 and Linux front-ends

**Known gaps**

- Setup still assumes someone who knows what a hostname and an SSH key are. The *reading*
  experience is written for a non-technical person; the *adding* experience is not yet.
- CI builds and tests the Rust core only. No macOS or Android runner is configured, so the apps
  are built locally.

## Licence

MIT OR Apache-2.0.
