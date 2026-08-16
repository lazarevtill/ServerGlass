# Working on ServerGlass

Agentless SSH server monitoring for macOS, iOS, Android, Windows and Linux. A Rust core does all
the work; each platform contributes only a view layer.

Read [README.md](README.md) for what it is, [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it
is built, and [docs/DESIGN.md](docs/DESIGN.md) for why the dashboard looks the way it does. This
file is the part that is easy to get wrong.

## The four invariants

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
cargo test --workspace                              # 236 tests
SG_REQUIRE_FIXTURES=1 cargo test --workspace        # turns "fixture missing" into a failure
swift test --package-path apps                      # Apple storage and vault
(cd apps/android && gradle :app:testDebugUnitTest)  # Android record format
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

The live tests need the fixtures: `./fixtures/up.sh` (Debian and Alpine sshd containers, catching
GNU-versus-busybox parsing differences). Without `SG_REQUIRE_FIXTURES=1` they skip themselves and
report `ok`, which has already hidden a broken container once.

**CI runs the Rust jobs on Linux and Windows.** No macOS or Android runner is configured, so the
Swift and Kotlin suites are yours to run before pushing. The Windows job is a shell runner that
installs its own toolchain — it is the only thing checking that the core compiles for a platform
none of us develops on, and it earned that on its first run by catching a Unix-only agent call.

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
- **The same code shape on two platforms is not the same behaviour.** Verify on each.

## Layout

```
crates/sg-model      domain types, the Source trait — no I/O
crates/sg-transport  russh client, the framed batch channel, host key policy
crates/sg-collect    the collectors, one module per subsystem
crates/sg-core       target registry, tick scheduler, rate derivation, ring buffer
crates/sg-ffi        the UniFFI surface: view models, health verdicts, plain language
apps/shared          SwiftUI views, shared byte for byte between macOS and iOS
apps/android         Jetpack Compose
apps/ios, apps/macos thin shells around apps/shared
fixtures             docker compose sshd targets and a throwaway key
scripts              build-{macos,ios,android}.sh, release.sh, make-icons.swift
```

`apps/shared/ServerGlassUI` is compiled by both Apple apps. A change there lands on the Mac and the
phone at once; build both.

## Never commit

`fixtures/id_test` (throwaway SSH key) and `release/serverglass.jks` (Android signing key) are
gitignored. Nothing that authenticates to anything belongs in the tree.

## Style

Match the surrounding code. Comments explain *why*, especially when the code looks odd — most of
the odd-looking code here is odd because of a specific host, kernel, or platform behaviour, and the
comment saying which is the thing that stops it being "simplified" back into a bug.
