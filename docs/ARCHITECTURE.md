# ServerGlass architecture

The decisions that are not obvious from reading the code, and the reasoning behind them. For what
exists versus what is planned, see the README. For why the interface looks the way it does, see
[DESIGN.md](DESIGN.md).

## The shape of the system

Everything follows from one trait and one split inside it.

```rust
pub trait Source: Send + Sync {
    fn descriptor(&self) -> &SourceDescriptor;
    fn requests(&self, ctx: &TargetCtx) -> Vec<Request>;
    fn parse(&self, ctx: &TargetCtx, r: &Responses, out: &mut SampleSink) -> ParseResult;
}
```

A collector *declares* what it needs, and separately *parses* what came back. It never performs
I/O. Four things fall out of that:

1. **One round trip per refresh.** The scheduler collects requests from every enabled source,
   deduplicates them, and issues one batch. Adding collectors costs bytes, not round trips.
2. **Deduplication is free.** `/proc/stat` wanted by three sources is fetched once.
3. **The trait is synchronous.** No async machinery, no per-call allocation — and, decisively, a
   WebAssembly plugin's synchronous exported functions map onto it exactly.
4. **Plugins need no capabilities.** A plugin can *ask* for a file and *parse* the answer, but
   cannot open a socket or run a command. That is a far stronger position than sandboxing a plugin
   that does its own I/O, and it matters for a tool holding SSH credentials.

## Transport

### One channel, not one per metric

The naive implementation opens an SSH `exec` channel per request: roughly two round trips each. On
a 200 ms link, twenty collectors make a refresh take eight seconds.

Instead, one channel is opened at connect time and `/bin/sh` runs on it for the life of the
connection. Each refresh writes one script and reads one framed reply.

`/bin/sh` explicitly, not the login shell: the account's shell might be fish or csh, whose syntax
the batch script is not written in. And **no PTY** — a PTY would echo the script back into the
output stream and translate LF to CRLF, corrupting every payload.

### Framing

```text
\n<nonce>B<id>\n
...raw output...
\n<nonce>E<id> <exit code>\n
```

The nonce is generated per connection from the OS entropy source. Without it, reading a file that
happened to contain the end marker would truncate the frame — and on a monitoring tool the file
being read is often a log somebody else writes to. A monitored host cannot predict a nonce created
after the connection came up, so it cannot forge a boundary. There is a test that tries.

Per-frame exit codes matter more than they look: a missing `/proc/pressure` is an expected outcome,
not an error. `Responses::text` returns `None` for a non-zero exit, which is what lets every parser
be written as `let Some(text) = r.text(&req) else { return Ok(()) };`.

### The exit-status trap, twice

Capability detection probes for binaries with one shell loop rather than one request per binary. A
shell loop exits with the status of its **last** iteration, so on any host missing the last-listed
binary the whole probe reported failure and its perfectly good output was discarded by the
exit-code filter above.

The same trap recurs wherever a batched command's output is the point rather than its status:

- the binary and path probes (`sg-transport/src/probe.rs`)
- the physical-interface and stacked-device probes (`sg-collect`)
- the process table, where `cat /proc/[0-9]*/stat` fails if any process exits mid-glob

All of them end in `exit 0`, and `list_probes_normalise_their_exit_status` is the regression test.

## Collection

### Counters stay raw

Sources emit the cumulative number the kernel gave them and declare `SeriesKind::Counter`. The
scheduler differentiates, using **measured** elapsed time — a tick that arrived 1.4 s after the
last one must not be divided by the nominal 1.0 s interval.

A counter produces no sample at all when it cannot honestly produce a rate: on first sighting, after
a counter goes backwards (reboot, recreated interface), or when no time has passed.

### CPU, and why `scale` exists

CPU utilisation is not a time rate. It is a ratio of two deltas — busy jiffies over elapsed jiffies
— which a source could only compute by remembering its previous reading.

Rather than make one collector stateful, `SeriesDescriptor` carries a `scale` applied after
differentiation. The CPU source emits raw busy jiffies as a counter with `scale = 100 /
clock_ticks`: the scheduler differentiates to jiffies-per-second, the scale converts that to percent
of one core, and the source stays a pure function. The aggregate divides again by the core count.

`clock_ticks` is measured with `getconf CLK_TCK` rather than assumed to be 100 — an assumption that
is right nearly everywhere and silently wrong where it isn't.

`iowait` counts as idle, not busy. The CPU was available; the disk was not.

The same mechanism gives per-process CPU for free: `utime + stime` is a counter with the same
scale, so a process spanning four cores reads 400%, exactly as `top` reports it.

### Host totals must not double-count

Two defects of the same shape, both found only by running against a real Proxmox host, both
producing wrong numbers in the largest type on the panel:

**Network.** Summing every non-loopback interface counts the same traffic on the physical port and
again on `vmbr0`, and again on each `tap`/`veth` — two to four times the real wire rate. Host totals
now sum only interfaces that have a sysfs `device` link. Asking the kernel what is hardware beats
blocklisting name patterns, and it gets bonds and VLANs right for free. A host with no such
interfaces (any container) falls back to the previous behaviour rather than reporting zero.

**Disk.** `parent_device` only recognises name-prefixed partitions, so an LVM volume, mdraid array
or ZFS zvol layered over a disk had its bytes added on top of the disk carrying the same I/O — and a
default Proxmox install is LVM-thin. Stacked devices are now identified from
`/sys/block/<dev>/slaves`, plus `zd*` by name since ZFS does not populate `slaves`.

### Memory

`used = MemTotal - MemAvailable`. The older `total - free - buffers - cached` arithmetic is why so
many tools report a healthy Linux host as out of memory; it is used only as a fallback on
pre-3.14 kernels.

A host with swap disabled gets no swap series at all — a 0/0 gauge renders as permanently full.

### The process table

`/proc/<pid>/stat`'s notorious trap is field 2: the command is parenthesised and may contain spaces
*and* parentheses — `(Web Content)`, `(foo (bar))`. Splitting on whitespace shifts every later field
and silently attributes one process's CPU to another's column. Scanning to the **last** `)` is the
only correct approach.

Kernel threads are dropped (no resident memory, nothing to explain) and the set is capped at 256 by
RSS so the live store stays bounded. Process entities are deliberately excluded from the snapshot's
entity list — hundreds of them, each with a 300-point window, would dominate the cost of a refresh
twice a second. Only the ranked twelve cross the FFI.

### Statelessness and churn

Sources re-declare their entities and descriptors every tick and never diff anything. The store
upserts. This is what lets parsers stay pure while container and pod sets change underneath them.

The counterpart is `LiveStore::retain_entities`: anything not reported this tick is dropped, along
with its series. Without it a container that exits stays on the dashboard forever with its last
reading frozen in place.

## Live-only

ServerGlass never opens a time-series database and never writes a sample to disk. What it keeps is a
bounded window per series — 300 points by default, five minutes at a one-second refresh.

The window is a hard cap, not a target. A host with 64 cores, 40 containers and a dozen disks
produces on the order of a thousand series; an unbounded store would grow for as long as the app is
open. `the_window_bounds_memory_no_matter_how_long_it_runs` holds that line.

History and alerting are delegated: live samples leave through `Sink` implementations to whatever
system already owns retention and alert rules.

## The FFI boundary

The core is a state machine; UIs send commands and render snapshots.

Each target runs a background task that ticks on its own interval and publishes a finished
`TargetSnapshot`. The UI polls it on a display timer. At a one-second refresh this is
indistinguishable from a push stream, needs no callback interface on any platform, and cannot
deadlock the tick loop behind a slow UI thread. A push-based event stream is the natural next step
once the terminal lands — a terminal cannot be polled.

**The first tick is withheld.** Counter-derived series do not exist until the second reading.
Publishing the first tick would render a status grid without its CPU and network tiles, which then
appear a refresh later and shove every other tile sideways.

View models are built in Rust, not in Swift or Kotlin. A gauge arrives already knowing its label,
bounds, unit suffix and sparkline; `format_value` and the health verdicts live in the core. This is
what keeps the "core owns all logic" invariant from eroding one convenience method at a time across
every front-end.

### Bindings per platform

- **macOS / iOS / iPadOS** — UniFFI's Swift backend. The Swift wrapper and the UI target are shared
  between the Mac and iOS apps verbatim.
- **Android** — UniFFI's Kotlin backend, calling into the `.so` through JNA. Built with `cargo-ndk`.
- **Linux** — `gtk4-rs` and libadwaita, linking the core directly. No FFI at all: `sg-ffi` builds
  an rlib alongside its staticlib and cdylib, so the view models, health verdicts and plain-language
  wording arrive as ordinary Rust types. There is no binding step and nothing generated to keep in
  sync, which is why it is the only front-end that cannot drift from the core by construction.
- **Windows** (planned) — `uniffi-bindgen-cs`, which is third-party and young; hand-written
  `extern "C"` plus `csbindgen` is the documented fallback.

A second binding backend earns its keep immediately. `SgError` originally carried a `message` field;
UniFFI maps an error enum onto a Kotlin `Exception` subclass, which already has `message`, and the
duplicate made every reference an overload-resolution ambiguity that failed the Android build. The
field is now `detail`. That defect was invisible with only Swift.

## Testing

- **Parsers** run against `/proc` text captured from real containers, on both a GNU and a BusyBox
  host. Hand-written fixtures encode what the author believed the format was — which is what a
  parser bug is.
- **Transport** is tested against real `sh`, including hostile arguments and payloads that
  impersonate the frame protocol.
- **The batching guarantee** has a regression test: `SshSession` counts round trips, and the
  end-to-end tests assert a refresh spends exactly one however many collectors are enabled. The
  running app displays the same counter, so the claim is observable rather than merely asserted.
- **The FFI layer** is driven exactly as a UI drives it — add target, start, poll snapshots —
  because it is the layer every app sits on.
- **Fixtures are required, not optional**, under `SG_REQUIRE_FIXTURES=1`. A skipped test reports as
  `ok`; this is how a suite quietly stops testing anything, and it happened once already when a
  fixture container died on a missing `/run/sshd`.


## Host key storage

Trusted host keys are recorded at a path the *application* chooses, not at `~/.ssh/known_hosts`.

A desktop has that directory; an app sandbox does not, and an Android app process has no `HOME` in
its environment at all. `learn_known_hosts` writes the file but will not create the directory
holding it, so on mobile the write failed every time — and because the failure was discarded, the
apps offered "remember this server's identity", recorded nothing, and would then have accepted a
substituted key on every later connection.

`ConnectionSpec::known_hosts` now carries the path. Apple passes
`Application Support/ServerGlass/known_hosts` (not Caches, which the system may purge — losing the
file silently downgrades security); Android passes `filesDir/ssh/known_hosts`. The transport
creates the containing directory, and a write that still fails is surfaced as
`HostKeyVerdict::AcceptedUnrecorded` rather than thrown away.


## Device pairing

`crates/sg-sync` moves a host inventory from one device to another, and is deliberately not a sync
service: there is no server, no account, and nothing persists anywhere between the two devices.

One device shows a QR and the other scans it. The code carries a **public** key, a session nonce
and every address the device might answer at — never a shared secret, because a screen can be
photographed and a screenshot of a QR is as good as the original. Both sides derive the same
six-digit verification code from the full transcript; the user compares the two screens, and only
then does anything transfer. The API is split along that line — connecting returns a code and
nothing else, sending is a separate call — because an interface where transfer happens in one call
cannot express the step the security rests on.

The offer lists several addresses because a device usually has several, and which one reaches
depends on where the other device is: over WireGuard or Tailscale the tunnel address is often the
only one that works. The scanner tries each in turn with a short per-address timeout.

What crosses: host records and `known_hosts` lines. What does not: passwords, passphrases and
pasted keys — they are not fields of the wire format at all. The merge rules are where the security
argument lands: a new pin merges silently, a *conflicting* pin is reported and never applied, and
local settings win over transferred ones. A sync channel that can quietly rewrite a pin is a
machine-in-the-middle with extra steps.

See [SYNC.md](SYNC.md) for the research behind those choices.
