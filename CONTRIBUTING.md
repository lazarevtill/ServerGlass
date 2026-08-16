# Contributing to ServerGlass

Bug reports, and patches that keep the invariants below, are welcome.

## The invariants

These are not preferences. A change that breaks one is rejected however well it works, so it is
worth reading them before writing anything.

1. **Nothing is installed, written, or modified on a monitored host.** Collectors may read files
   and run binaries that are already there. No agent, no package, no config edit, no temp file.
   The one exception is the command runner, where the *user* types the command — ServerGlass
   itself still only ever reads.
2. **No sample ever touches disk.** A bounded in-memory ring buffer, then out through a sink or
   nowhere. History and alerting belong to whatever the samples are exported to.
3. **The core owns all logic.** Parsing, scheduling, rate derivation, health verdicts, number
   formatting, plain-language wording, and how worrying a reading is — all Rust, shared by every
   platform. A UI maps a level onto a colour and lays things out. Nothing else.
4. **The widget must match the metric.** A ring implies a proportion, so it is only ever drawn for
   a reading with a real maximum. A rate gets a number and a sparkline.
5. **A credential never leaves the device it was entered on.** See [docs/SYNC.md](docs/SYNC.md).

Invariant 3 is the one that erodes, and it erodes through small conveniences. It has been broken
twice, both times by a threshold written in Swift because it was two lines, then written again in
Kotlin with different numbers — the same host read amber on a phone and green on a desk for days.
If you find yourself writing a threshold, a unit conversion, or a piece of user-facing wording in
Swift, Kotlin or C#, it belongs in `crates/sg-ffi` instead.

## Before you open a pull request

```bash
./scripts/check.sh          # fmt, clippy, build, the whole Rust suite
./scripts/check.sh --all    # plus the Swift and Kotlin suites, if you have those toolchains
```

`check.sh` is the same set of checks CI runs. Running it is not optional politeness: `cargo test`
passing locally says nothing about `fmt` and `clippy`, and the pipeline has been left red for eight
commits over exactly that.

The live SSH tests need the Docker fixtures, and **skip themselves silently without them**, which
has hidden a broken container once already:

```bash
./fixtures/up.sh
SG_REQUIRE_FIXTURES=1 cargo test --workspace   # turns "fixture missing" into a failure
```

## What gets a patch sent back

- **A UI-only change to something the core should decide.** See invariant 3.
- **A collector that fetches something on its own.** A refresh costs exactly one round trip
  whatever is enabled; `crates/sg-core/tests/end_to_end.rs` asserts it. If that test fails, the
  collector is wrong, not the test.
- **A shell loop that does not end in `exit 0`.** A `for` loop exits with the status of its last
  iteration, and a non-zero request has its body discarded — so a host missing the last-listed
  binary throws away a perfectly good payload. There is a regression test per collector.
- **Anything interpolated from a host or a user into a script.** Argv is escaped by
  `crates/sg-transport/src/quote.rs`; the scripts themselves are constants.
- **A discarded error.** `let _ = learn_known_hosts(…)` meant "trust this server" recorded nothing
  on mobile for weeks, while the UI promised that it did.
- **A new feature with no test.** The Swift and Kotlin layers had no test target for most of this
  project's life, and every bug that reached a device lived there.

## Style

Match the surrounding code. Comments explain *why*, especially when the code looks odd — most of
the odd-looking code here is odd because of a specific host, kernel, or platform behaviour, and the
comment naming that behaviour is the thing that stops it being "simplified" back into a bug.

Commit messages say what changed and why, in the imperative, on one line under about 72
characters. `git log` is the house style guide.

## Where things are

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it is built, [docs/DESIGN.md](docs/DESIGN.md)
for why the dashboard looks the way it does, [docs/GUIDE.md](docs/GUIDE.md) for what each flow
looks like to the person using it, and [AGENTS.md](AGENTS.md) for the things that look fine and are
not. Windows contributors want [docs/WINDOWS.md](docs/WINDOWS.md) first — the setup needs two
things beyond Rust that nothing tells you about until a build fails several minutes in.

## Verifying on more than one platform

The same code shape on two platforms is not the same behaviour. The Windows app rendered uptime as
`11324 s` where the phone said `3h 8m`, because Swift and Kotlin both special-cased that one metric
and the third front-end did not know to. If a change touches something every app renders, say in
the pull request which platforms you actually ran it on, and which you did not.

A green build is also not a running app: the WinUI app once crashed on launch, from a layout cycle,
with every test passing and no compiler warning. Open the thing.

## Licence

By contributing you agree that your work is licensed under MIT OR Apache-2.0, matching the project.
