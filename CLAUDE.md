# Working on ServerGlass

Agentless SSH server monitoring for macOS, iOS, Android, Linux and Windows. A Rust core does all
the work; each platform contributes only a view layer.

Read [README.md](README.md) for what it is, [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it
is built, [docs/DESIGN.md](docs/DESIGN.md) for why the dashboard looks the way it does, and
[docs/GUIDE.md](docs/GUIDE.md) for what each flow looks like to the person using it. This file is
the part that is easy to get wrong.

## The invariants

These are not preferences. A change that breaks one is rejected however well it works.

1. **Nothing is installed, written, or modified on a monitored host.** Collectors may read files
   and run binaries that are already there. No agent, no package, no config edit, no temp file.
   The one exception is the command runner, where the *user* types the command — ServerGlass
   itself still only ever reads.
2. **No sample ever touches disk.** A bounded in-memory ring buffer (300 points per series), then
   out through a sink or nowhere. History and alerting belong to whatever the samples are exported
   to, not here.
3. **The core owns all logic.** Parsing, scheduling, rate derivation, health verdicts, number
   formatting, plain-language wording, and how worrying a reading is — all Rust, shared by every
   platform. A UI maps a level onto a colour and lays things out. Nothing else.
4. **The widget must match the metric.** A ring implies a proportion, so it is only ever drawn for
   a reading with a real maximum. A rate gets a number and a sparkline.
5. **A credential never leaves the device it was entered on.** Pairing transfers the inventory and
   the host key pins; the receiving device asks for each credential once and keeps it in its own
   keystore. `crates/sg-sync` has a test asserting the exact set of fields on the wire, so adding
   one fails on purpose — see `docs/SYNC.md` for why this is not an oversight but the design.

Invariant 3 is the one that erodes. It has been broken twice, both times by a small convenience —
a colour threshold written in Swift because it was two lines, then written again in Kotlin with
different numbers. The same host read amber on a phone and green on a desk for days. If you find
yourself writing a threshold, a unit conversion, or a piece of wording in Swift or Kotlin, it
belongs in `sg-ffi` instead.

## One round trip per refresh

The central design claim: however many collectors are enabled, a refresh costs exactly one network
round trip. `Source` is split into `requests()` and `parse()` so the scheduler can gather every
request up front, deduplicate them (`/proc/stat` is wanted by three collectors and fetched once),
issue them as one framed batch over a single long-lived `/bin/sh` channel, and fan the responses
back out.

`crates/sg-core/tests/end_to_end.rs` asserts this against a live fixture. If a change makes a
collector fetch something on its own, that test is the one that will fail, and the right response
is to fix the collector rather than the test.

## Shell in collectors

Several collectors run a small `sh` sweep rather than reading one file. Two rules, both learned the
hard way:

- **End every loop with `exit 0`.** A `for` loop exits with the status of its last iteration, and
  `Responses::text()` discards the body of a non-zero request. A host missing the last-listed
  binary threw away a perfectly good payload. There is a regression test per collector.
- **Never interpolate anything from a host or a user into a script.** Argv is escaped by
  `sg-transport/src/quote.rs`; the scripts themselves are constants.

## Verifying

```bash
./scripts/check.sh                                  # fmt, clippy, build, 272 tests
./scripts/check.sh --all                            # plus Swift and Kotlin
.\scripts\check.ps1 -All                            # the same, on Windows, plus the WinUI app
./scripts/build-linux.sh --run                      # build and run the GTK app
SG_REQUIRE_FIXTURES=1 cargo test --workspace        # turns "fixture missing" into a failure
swift test --package-path apps                      # Apple storage and vault
(cd apps/android && gradle :app:testDebugUnitTest)  # Android record format
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

Narrowing it down while iterating — the whole suite is the gate, not the loop:

```bash
cargo test -p sg-collect memory                     # one crate, tests matching a substring
cargo test -p sg-core --test end_to_end             # one integration target
cargo test -p sg-collect cpu::tests::iowait_counts_as_idle_not_busy -- --nocapture
swift test --package-path apps --filter HostStoreTests
(cd apps/android && gradle :app:testDebugUnitTest --tests '*HostStoreTest*')
```

The Linux app has its own suite, including four tests that drive it against a live fixture:

```bash
cargo test --manifest-path apps/linux/Cargo.toml
SG_FIXTURE_PORT=2322 cargo test --manifest-path apps/linux/Cargo.toml --test live_engine
```

`SG_FIXTURE_PORT` exists because Windows reserves TCP ranges for Hyper-V and 2222/2223 can land
inside one; see docs/WINDOWS.md before concluding a container is broken.

Three integration targets need the SSH fixtures and skip without them —
`sg-core/tests/end_to_end.rs`, `sg-transport/tests/live.rs`, `sg-ffi/tests/live_snapshot.rs`. The
fourth, `sg-sync/tests/pairing_over_tcp.rs`, pairs two devices over a loopback socket and always
runs.

The live tests need the fixtures: `./fixtures/up.sh` (Debian and Alpine sshd containers, catching
GNU-versus-busybox parsing differences). Without `SG_REQUIRE_FIXTURES=1` they skip themselves and
report `ok`, which has already hidden a broken container once.

**CI is GitLab (`.gitlab-ci.yml`, gitlab.lazarev.cloud) and runs the Rust jobs on Linux and
Windows, the Linux app on Linux, and the Windows app on Windows.** No macOS or Android runner is
configured, so the Swift and Kotlin suites are yours to run before pushing. The Windows jobs are a
shell runner that installs its own toolchain — `rust:windows` is the only thing checking that the
core compiles for a platform none of us develops on, and it earned that on its first run by
catching a Unix-only agent call.

Check the pipeline after pushing. It was left red for eight commits once, because tests passing
locally says nothing about `fmt` and `clippy`.

## Things that look fine and are not

Every one of these shipped, and every one was found by using the app rather than by reading it:

- **A test target that does not exist is not coverage.** The Swift and Kotlin layers had none, and
  every bug that reached a device lived there.
- **A skipped test reports `ok`.** Hence `SG_REQUIRE_FIXTURES`.
- **A discarded error is a silent failure.** `let _ = learn_known_hosts(…)` meant "trust this
  server" recorded nothing on mobile for weeks while the UI promised it did.
- **A constant that is never read is not behaviour.** `backoff_for` climbed properly, was
  unit-tested, and was called with a hardcoded `1`.
- **Driving a screen from a debug intent is not driving the app.** Android shipped with no way to
  add a server because every test launch passed the host in by intent.
- **The same code shape on two platforms is not the same behaviour.** Verify on each. The Windows
  app rendered uptime as `11324 s` where the phone says `3h 8m`, because Swift and Kotlin both
  special-case that one metric and the third front-end did not know to.
- **A green build is not a running app.** The WinUI app crashed on launch — a layout cycle, which
  is a runtime failure by construction — with every test passing and no compiler warning.

## Working on Windows

The core and the app both build and test there. Setup, the PowerShell equivalents of the commands
above, what can and cannot be verified from a Windows machine, and the platform traps that have
already caused real bugs are in **[docs/WINDOWS.md](docs/WINDOWS.md)**.

The short version: there is no `$HOME` (it is `USERPROFILE`), the SSH agent is Pageant rather than
a Unix socket, `#[cfg(unix)]` blocks are invisible until something compiles them, and the setup
needs two things beyond Rust that nothing tells you about until a build fails several minutes in —
the MSVC **C++ workload** (Build Tools can be installed without it) and **NASM**. Do not claim a
change is verified on Apple or Android from a Windows machine — say which platform you checked.

`scripts/check.ps1` and the other PowerShell scripts are UTF-8 **with a BOM**, and that is
load-bearing rather than incidental: Windows PowerShell 5.1 reads a BOM-less file as the ANSI
codepage, and an em dash then decodes to a character it treats as a closing quote, so the script
fails to parse entirely. Keep the BOM if you rewrite one.

## Layout

```
crates/sg-model      domain types, the Source trait — no I/O
crates/sg-transport  russh client, the framed batch channel, host key policy
crates/sg-collect    the collectors, one module per subsystem
crates/sg-core       target registry, tick scheduler, rate derivation, ring buffer
crates/sg-ffi        the UniFFI surface: view models, health verdicts, plain language
crates/sg-sync       device pairing: the QR handshake, the LAN transfer, the merge rules
crates/sg-bindgen    the uniffi-bindgen binary, separate so clap never reaches the shipped library
apps/shared          SwiftUI views, shared byte for byte between macOS and iOS
apps/android         Jetpack Compose
apps/ios, apps/macos thin shells around apps/shared
apps/linux           GTK4 and libadwaita — its own cargo workspace, links the core with no FFI
apps/windows         WinUI 3 on .NET 9 — reaches the core through the C ABI in sg-ffi/src/cabi.rs
fixtures             docker compose sshd targets and a throwaway key
scripts              build-{macos,ios,android,windows}.{sh,ps1}, check.{sh,ps1}, release.sh
```

`apps/windows` is not a cargo crate at all — it is two C# projects and a WinUI app, built by
`.\scripts\build-windows.ps1` and by the `windows:app` CI job. It reaches the core through
`crates/sg-ffi/src/cabi.rs`, a hand-written `extern "C"` surface that moves whole view models across
as JSON, because UniFFI has no C# backend and `uniffi-bindgen-cs` is pinned a uniffi version behind
this workspace. `csbindgen` generates the C# declarations from those signatures; the result is
committed and `scripts/check-bindings.ps1` fails if it goes stale.

`apps/linux` is deliberately **not** a member of the root cargo workspace. The core has to keep
building on a machine with no desktop, and a member would put `libgtk-4-dev` in the way of every
Rust job in the pipeline. It has its own `Cargo.lock`, its own `linux:app` CI job, and
`./scripts/check.sh` runs it whenever GTK is installed — set `SG_REQUIRE_LINUX_APP=1` to turn a
missing toolkit into a failure rather than a skip.

It is also the only front-end that cannot drift from the core: it links `sg-ffi` as an rlib, so a
view model or a health verdict is a function call rather than a generated binding. When a rule
lives in Swift and Kotlin but not in Rust — the sparkline noise floor was written by hand in both —
the fix is to move it into `sg-ffi` and call it from here, not to write it a third time.

`apps/shared/ServerGlassUI` is compiled by both Apple apps. A change there lands on the Mac and the
phone at once; build both.

The Swift bindings in `apps/shared/ServerGlassFFI/generated/` are produced by `sg-bindgen` from the
*compiled* `libsg_ffi`, not from the source, which is why the build scripts always order it Rust
build → bindgen → Swift build. They are committed, so a change to the `sg-ffi` surface is only real
once `./scripts/build-macos.sh` has regenerated them and the result is in the commit.

## Never commit

`fixtures/id_test` (throwaway SSH key) and `release/serverglass.jks` (Android signing key) are
gitignored. Nothing that authenticates to anything belongs in the tree.

## Style

Match the surrounding code. Comments explain *why*, especially when the code looks odd — most of
the odd-looking code here is odd because of a specific host, kernel, or platform behaviour, and the
comment saying which is the thing that stops it being "simplified" back into a bug.
