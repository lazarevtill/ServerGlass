# ServerGlass on Windows

Everything needed to get a coding agent — or a person — productive on Windows, and an honest
statement of what does and does not exist here yet.

## Where Windows actually stands

| | |
|---|---|
| **The Rust core** | Builds and passes its tests on Windows. Verified on every push by the `rust:windows` CI job. |
| **The Windows app** | **Does not exist.** `apps/windows/` is empty. |

Nothing has been faked: the half that opens SSH connections, collects, parses, derives rates,
assesses health and produces the view models runs on Windows today. The window around it is not
written. See [Building the Windows app](#building-the-windows-app) for what that involves.

## Setting up

### 1. Rust

```powershell
winget install Rustlang.Rustup
rustup default 1.89.0
```

The CI runner installs exactly this and nothing else:

```
1.89.0-x86_64-pc-windows-msvc
rustc 1.89.0 (29483883e 2025-08-04)
cargo 1.89.0 (c24e10642 2025-06-23)
```

The version is pinned deliberately — `Cargo.lock` pins `cargo-platform` to a version with a
toolchain floor, and a silently moving compiler is not something a CI failure should trace back to.

### 2. The MSVC toolchain

`rustup` on Windows defaults to the `x86_64-pc-windows-msvc` target, which needs Microsoft's
linker and C toolchain. `aws-lc-sys` (russh's crypto backend) compiles C during the build, so this
is a hard requirement, not an optional extra.

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools
```

In the installer, select **Desktop development with C++**. `rustup` will tell you if it is missing.

### 3. Git

```powershell
winget install Git.Git
```

That is the whole setup. There is no Node, no Python, no Docker needed to build and test the core.

## Building and testing

```powershell
cargo build --workspace
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

All four are what CI runs. A clean checkout takes a few minutes on the first build, mostly
compiling `aws-lc-sys`.

### The live tests

Some tests connect to a real SSH server. They **detect the absence of a fixture and skip
themselves**, so `cargo test --workspace` is green on a machine with no Docker — which is how the
Windows job passes today.

To run them for real you need the fixture containers, which are Linux:

```powershell
# Docker Desktop with the WSL 2 backend, or Docker inside WSL
wsl ./fixtures/up.sh
$env:SG_REQUIRE_FIXTURES = "1"
cargo test --workspace
```

`SG_REQUIRE_FIXTURES=1` turns "fixture missing" from a skip into a failure. Set it whenever you
believe the fixtures are up, because a skipped test reports `ok` and that has hidden a broken
container before.

## Windows-specific traps

Every one of these was a real bug caught by the CI job, not a hypothetical:

- **There is no `$HOME`.** Windows uses `USERPROFILE`. Code that resolves a home directory must
  check both — `crates/sg-transport/src/session.rs` does, and did not until Windows CI existed.
- **The SSH agent is a different agent.** `AgentClient::connect_env` is Unix-only. Windows uses
  Pageant, the protocol both PuTTY and OpenSSH for Windows serve, reached by `connect_pageant()`
  with a *different stream type*. That is why the agent connection dispatches by platform and the
  identity loop is generic.
- **Not every `Result` matches across platforms.** `connect_pageant()` returns a `Result` where the
  Unix connector does not.
- **Paths are backslashed and case-insensitive.** Use `Path::join`, never string concatenation.
- **`#[cfg(unix)]` blocks are invisible until something compiles them.** If you add one, assume the
  Windows side is wrong until CI says otherwise.

The pattern: a platform nobody develops on drifts silently. The `rust:windows` job exists to make
that impossible, and it found three separate breakages on its first two runs.

## For an LLM agent working here

Read [CLAUDE.md](../CLAUDE.md) first — the four invariants apply on every platform and are not
negotiable. Then the Windows-specific rules:

**Shell.** The CI job and these instructions are PowerShell. `sh`-isms (`&&`, `export`, `$(…)`,
`rm -rf`) do not work in PowerShell. Use `;` to sequence, `$env:NAME = "value"` to set a variable,
and `Remove-Item -Recurse -Force` to delete.

**What you can verify locally on Windows, and what you cannot:**

| Runs on Windows | Needs another machine |
|---|---|
| `cargo build` / `test` / `clippy` / `fmt` | `swift test` (macOS only) |
| The core's whole unit suite | `gradle :app:testDebugUnitTest` (needs the Android SDK) |
| Live tests, with Docker Desktop + WSL | Building the macOS, iOS or Android apps |

Do not claim a change is verified on the Apple or Android side from a Windows machine. Say which
platform you checked. The single most repeated mistake in this project's history is assuming the
same code shape behaves the same way on another platform — it has not, four separate times.

**Before pushing:** run all four cargo commands above. CI has been left red for eight commits
because tests passing locally says nothing about `fmt` and `clippy`.

**When a collector needs a shell sweep**, remember the rules in CLAUDE.md: every loop ends with
`exit 0`, and nothing from a host or a user is ever interpolated into the script. These are about
the *remote* shell — always POSIX `sh` on the monitored Linux host — regardless of what you are
developing on.

## The CI job

`.gitlab-ci.yml`, job `rust:windows`:

- Runs on the `windows` shell runner rather than a container, which also means it keeps working
  while the image registry is unavailable.
- Installs its own toolchain on first run, because a shell runner starts with whatever the machine
  happens to have.
- Caches `.cargo/registry`, `.rustup/toolchains` and `target/` against `Cargo.lock`.
- Is **required**. It is not `allow_failure` — a job that is allowed to fail is a job nobody reads.

## Building the Windows app

Not started. The design is fixed and the FFI surface it binds to is stable, so this is
implementation rather than exploration:

1. **C# bindings.** `uniffi-bindgen-cs` is third-party and young; the documented fallback is
   hand-written `extern "C"` plus `csbindgen`. Evaluate the former, expect to need the latter.
   The Rust side already exports a `cdylib` — see `crates/sg-ffi/Cargo.toml`.
2. **A WinUI 3 project** under `apps/windows/`, consuming those bindings.
3. **The views**, which must match what the other platforms show — same panels, same order, same
   widget rules. [docs/DESIGN.md](DESIGN.md) is the spec and
   `apps/shared/ServerGlassUI/HostDetailView.swift` is the reference implementation; the Android
   `Technical.kt` is a worked example of matching it exactly on a second toolkit.
4. **Storage.** The Apple apps use the Keychain and Android uses the Keystore, both wrapped by a
   small platform-specific store. Windows should use DPAPI or the Credential Manager the same way.
   Secrets never go in the host record — see `HostStore` on either platform.

The one thing not to do is invent behaviour. Every threshold, unit, verdict and piece of wording is
already in `crates/sg-ffi`; a Windows view layer maps a level onto a colour and lays things out.
