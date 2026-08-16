## What this changes

<!-- And why. The why is the part that will not be obvious from the diff in six months. -->

## How it was verified

<!--
Which platforms you actually ran it on, and which you did not. "The same code shape on two
platforms is not the same behaviour" is a lesson this project learned by shipping uptime as
`11324 s` on Windows and `3h 8m` on the phone.
-->

- [ ] `./scripts/check.sh` passes
- [ ] Built and *opened* every app this touches — a green build is not a running app
- [ ] Ran against a real host, or the Docker fixtures (`./fixtures/up.sh`)

Platforms verified:
Platforms not verified:

## Invariants

<!-- Delete any that this change cannot affect. -->

- [ ] Nothing is installed, written, or modified on a monitored host
- [ ] No sample touches disk
- [ ] No threshold, unit conversion, or user-facing wording was added in Swift, Kotlin or C# —
      those live in `crates/sg-ffi`
- [ ] A refresh still costs one round trip (`crates/sg-core/tests/end_to_end.rs`)
- [ ] Any new shell sweep ends in `exit 0`, and interpolates nothing from a host or a user
- [ ] No credential is added to the pairing payload (`crates/sg-sync` has a test that fails if one is)
