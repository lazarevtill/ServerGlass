# ServerGlass architecture

This document explains the decisions that are not obvious from reading the code, and the reasoning
behind them. For what exists today versus what is planned, see the README's roadmap.

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
I/O. Four things fall out of that, and they are the reasons the split exists:

1. **One round trip per refresh.** The scheduler collects requests from every enabled source,
   deduplicates them, and issues one batch. Adding collectors costs bytes, not round trips.
2. **Deduplication is free.** `/proc/stat` wanted by three sources is fetched once.
3. **The trait is synchronous.** No async machinery, no per-call allocation — and, decisively, a
   WebAssembly plugin's synchronous exported functions map onto it exactly. Built-ins, declarative
   probes and plugins are genuinely the same kind of thing.
4. **Plugins need no capabilities.** A plugin can *ask* for a file and *parse* the answer, but
   cannot open a socket or run a command. The host's policy decides what its requests turn into.
   That is a far stronger position than sandboxing a plugin that does its own I/O, and it matters
   for a tool holding SSH credentials.

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

The `\n` before each marker is emitted by us, not by the command, so output lacking a trailing
newline cannot run into the marker; decoding strips exactly that one byte back off.

Per-frame exit codes matter more than they look: a missing `/proc/pressure` is an expected outcome
on many hosts, not an error. `Responses::text` returns `None` for a non-zero exit, which is what
lets every parser be written as `let Some(text) = r.text(&req) else { return Ok(()) };` — a source
silently produces nothing on hosts that lack its data.

### A bug worth remembering

Capability detection probes for binaries with one shell loop rather than one request per binary.
A shell loop exits with the status of its **last** iteration, so on any host missing the
last-listed binary the whole probe reported failure and its perfectly good output was discarded by
the exit-code filter above. The fix is a trailing `exit 0`; the lesson is that for a probe whose
output is the entire point, the final iteration's status is noise. `list_probes_normalise_their_exit_status`
is the regression test.

## Collection

### Counters stay raw

Sources emit the cumulative number the kernel gave them and declare `SeriesKind::Counter`. The
scheduler differentiates, using **measured** elapsed time — a tick that arrived 1.4 s after the
last one must not be divided by the nominal 1.0 s interval.

A counter produces no sample at all when it cannot honestly produce a rate: on first sighting (one
reading is not a rate), after a counter goes backwards (reboot or recreated interface), or when no
time has passed. Emitting zero or the raw value in those cases puts a spike or a trough on every
chart at connect time.

### CPU, and why `scale` exists

CPU utilisation is not a time rate. It is a ratio of two deltas — busy jiffies over elapsed jiffies
— which a source could only compute by remembering its previous reading.

Rather than make one collector stateful, `SeriesDescriptor` carries a `scale` applied after
differentiation. The CPU source emits raw busy jiffies as a counter with `scale = 100 /
clock_ticks`: the scheduler differentiates to jiffies-per-second, the scale converts that to
percent of one core, and the source stays a pure function. The aggregate row divides again by the
core count, normalising any machine to 0–100 %.

`clock_ticks` is measured with `getconf CLK_TCK` rather than assumed to be 100 — an assumption
that is right nearly everywhere and silently wrong where it isn't.

`iowait` counts as idle, not busy. The CPU was available; the disk was not. Counting it as busy
makes an I/O-bound host look saturated.

### Memory

`used = MemTotal - MemAvailable`. The older `total - free - buffers - cached` arithmetic is the
reason so many tools report a healthy Linux host as out of memory; it is used only as a fallback on
pre-3.14 kernels that do not publish `MemAvailable`.

A host with swap disabled gets no swap series at all — a 0/0 gauge renders as permanently full.

### Statelessness and churn

Sources re-declare their entities and descriptors every tick and never diff anything. The store
upserts. This is what lets parsers stay pure while container and pod sets change underneath them —
a stateful parser would have to reconcile the churn itself, in every collector.

The counterpart is `LiveStore::retain_entities`: anything not reported this tick is dropped, along
with its series. Without it a container that exits stays on the dashboard forever with its last
reading frozen in place, which reads as "still running" to anyone glancing at it.

## Live-only

ServerGlass never opens a time-series database and never writes a sample to disk. What it keeps is
a bounded window per series — 300 points by default, five minutes at a one-second refresh — so
gauges can be charts rather than numbers.

The window is a hard cap, not a target. A host with 64 cores, 40 containers and a dozen disks
produces on the order of a thousand series; an unbounded store would grow for as long as the app is
open. `the_window_bounds_memory_no_matter_how_long_it_runs` holds that line.

History and alerting are delegated: live samples leave through `Sink` implementations to whatever
system already owns retention and alert rules.

## The FFI boundary

The core is a state machine; UIs send commands and render snapshots.

Each target runs a background task that ticks on its own interval and publishes a finished
`TargetSnapshot`. The UI polls it on a display timer. At a one-second refresh this is
indistinguishable from a push stream, needs no callback interface implemented on four platforms,
and cannot deadlock the tick loop behind a slow UI thread. A push-based event stream is the natural
next step once the terminal lands — a terminal cannot be polled.

View models are built in Rust, not in Swift. A gauge arrives already knowing its label, its bounds,
its unit suffix and its sparkline, and `format_value` lives in the core so all four UIs format a
byte rate identically. This is what keeps the "core owns all logic" invariant from eroding one
convenience method at a time across four codebases.

### The first tick is withheld

Counter-derived series do not exist until the second reading. Publishing the first tick would
render a status grid without its CPU and network tiles, which then appear a refresh later and shove
every other tile sideways. The poller holds the first tick back: one extra refresh interval of
"Collecting…", in exchange for a grid that is complete and stable the moment it appears.

### Bindings per platform

- **macOS / Android** — UniFFI's officially supported Swift and Kotlin backends.
- **Linux** — `gtk4-rs`, linking the core directly. No FFI at all.
- **Windows** — `uniffi-bindgen-cs`, which is third-party and young; hand-written `extern "C"`
  plus `csbindgen` is the documented fallback if it proves unstable.

The core API is kept narrow and free of generics and complex trait objects precisely so all of
those backends can handle it.

## Testing

- **Parsers** run against `/proc` text captured from real containers, on both a GNU and a BusyBox
  host. Hand-written fixtures encode what the author believed the format was — which is what a
  parser bug is.
- **Transport** is tested against real `sh`, including hostile arguments and payloads that
  impersonate the frame protocol.
- **The batching guarantee** has a regression test: `SshSession` counts round trips, and the
  end-to-end tests assert a refresh spends exactly one however many collectors are enabled. The
  running app displays the same counter, so the claim is observable rather than merely asserted.
- **Fixtures are required, not optional**, under `SG_REQUIRE_FIXTURES=1`. A skipped test reports as
  `ok`; this is how a suite quietly stops testing anything.
