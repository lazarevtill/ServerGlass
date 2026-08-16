# Security policy

ServerGlass holds SSH credentials for machines people care about, so a vulnerability here is worth
more than the code suggests. Reports are taken seriously and answered.

## Reporting a vulnerability

**Do not open a public issue.** Use GitHub's private reporting —
[Security → Report a vulnerability](https://github.com/lazarevtill/ServerGlass/security/advisories/new) —
or email <lazarevtill@lazarev.cloud>.

Please include what you need to demonstrate it: the version or commit, the platform, and the steps.
A proof of concept against your own machine is welcome; please do not test against hosts you do not
own.

Expect an acknowledgement within 72 hours and an assessment within a week. If a fix is warranted it
ships in the next release, and the advisory credits you unless you would rather it did not.

## Supported versions

Only the latest release. This is a young project with one maintainer; there is no branch to
backport to, and pretending otherwise would be a promise that could not be kept.

## The security model, so reports can be aimed usefully

**Credentials.** A password or key passphrase is stored in the platform keystore — the Keychain on
Apple with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`, the Android Keystore via
`EncryptedSharedPreferences`, DPAPI on Windows. The Linux app is the exception and says so in its
own UI: it holds a credential in memory for the run and does not persist it. Nothing is written to
disk in plaintext on any platform, and nothing is sent anywhere but the server being connected to.

**Host keys** are pinned on first connection and checked on every one after. A changed host key
stops the connection and says so; it is never silently accepted.

**Pairing between devices** transfers the server inventory and the pinned host keys, and
deliberately *not* the credentials — the receiving device asks for each one and puts it in its own
keystore. The transfer is X25519 + HKDF-SHA256 + ChaCha20-Poly1305 over the LAN, with a
short authentication string shown on both devices to confirm out of band. `crates/sg-sync` has a
test asserting the exact set of fields on the wire, so adding one fails on purpose. The reasoning
is in [docs/SYNC.md](docs/SYNC.md).

**Monitored hosts are read from, never written to.** Collectors read files and run binaries already
present. The scripts they run are compile-time constants, and anything from a host or a user that
reaches a command line is escaped by `crates/sg-transport/src/quote.rs`. The command runner is the
one place a user-supplied command is executed, at the user's explicit request.

**No sample is ever written to disk**, and there is no account, no telemetry, and no server
component. A bounded in-memory ring buffer is the entire history the app keeps.

### Things that are in scope

Anything that would let an attacker read a credential, defeat host key pinning, get a monitored
host written to, inject a command into a collector script, extract data from the pairing exchange,
or take data off the device.

### Things that are not

- **A malicious server you deliberately connected to being able to make the UI display nonsense.**
  Collectors parse untrusted text and are fuzzed against malformed input, but a hostile host can
  always lie about its own numbers. It should not be able to escape the parser — that part *is* in
  scope.
- **Physical access to an unlocked device.** The keystore protects data at rest; it cannot protect
  an unlocked phone from its holder.
- **Credentials the user pasted into the command runner**, which is a terminal by request.
- **The absence of Gatekeeper notarisation on the macOS `.dmg`.** It is ad-hoc signed, the README
  says so, and building from source avoids it entirely.
