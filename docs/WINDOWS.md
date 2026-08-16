# ServerGlass on Windows

Everything needed to get a coding agent — or a person — productive on Windows.

## Where Windows stands

| | |
|---|---|
| **The Rust core** | Builds and passes its tests on Windows. Verified on every push by the `rust:windows` CI job. |
| **The Windows app** | WinUI 3 on .NET 9, in `apps/windows/`. Built and tested on every push by the `windows:app` CI job. |

The app shows the same two screens as every other platform — the plain overview and the dense
technical view — plus the command runner and device pairing. It does not scan a QR, because a
desktop has no camera; it renders one to receive an inventory and takes a pasted code to send one.

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

**The C++ workload is not installed by default, and Build Tools can be present without it** — which
looks like a working install right up until `cargo build` fails with `linker link.exe not found`.
Select **Desktop development with C++** in the installer, or add it without the UI:

```powershell
& "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\setup.exe" modify `
  --installPath "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools" `
  --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --quiet --norestart
```

`setup.exe modify` has no `--wait`; passing one fails with exit code 87 and a usage dump.

### 3. NASM

```powershell
winget install NASM.NASM
```

`aws-lc-sys` assembles x86-64 assembly as well as compiling C, and without NASM the build panics
several minutes in with `NASM command not found! Build cannot continue.` — an error naming a crate
rather than the missing tool. `scripts\check.ps1` checks for it up front.

### 4. .NET, for the app only

```powershell
winget install Microsoft.DotNet.SDK.9
```

Not needed for the core. Nothing else is: the Windows App SDK arrives as a NuGet package, so there
is no Visual Studio, no workload install and no MSIX certificate involved.

### 5. Git

```powershell
winget install Git.Git
```

That is the whole setup. There is no Node, no Python, and no Docker needed to build and test the
core.

## Building and testing

One command runs everything CI runs, in the same order:

```powershell
.\scripts\check.ps1          # the Rust core
.\scripts\check.ps1 -All     # the core, plus the Windows app and its tests
```

It checks formatting, then clippy with warnings denied, then builds, then tests — and stops at the
first failure with the name of the step. A green run here means a green pipeline. It also tells you
what to install if anything is missing.

`.\scripts\check.ps1` rather than `pwsh scripts/check.ps1`: PowerShell 7 is not on a stock Windows
machine, and the script runs under the Windows PowerShell 5.1 that is. **The file is UTF-8 with a
BOM on purpose** — 5.1 reads a BOM-less file as the ANSI codepage, and the em dashes then decode to
a character it treats as a closing quote, so the script fails to parse at all. If you rewrite it,
keep the BOM.

The four steps individually, if you want to run one:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
cargo test --workspace --locked
```

A clean checkout takes a few minutes on the first build, mostly compiling `aws-lc-sys`.

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

## Starting cold: the whole loop

```powershell
git clone https://gitlab.lazarev.cloud/lazarevtill/serverglass.git
cd serverglass
.\scripts\check.ps1 -All
```

If that passes, the core and the app are built and tested and you are current. Then:

1. Read [CLAUDE.md](../CLAUDE.md) — the invariants, and the list of mistakes this project has
   already made.
2. Read the section below for what Windows can and cannot verify.
3. Make the change, run `.\scripts\check.ps1 -All` again, push, **and check the pipeline** — both
   Windows jobs are required, and `rust:windows` has caught three real portability breakages that a
   local build on another platform would never have shown.

## For an LLM agent working here

**Shell.** The CI job and these instructions are PowerShell. `sh`-isms (`&&`, `export`, `$(…)`,
`rm -rf`) do not work in PowerShell. Use `;` to sequence, `$env:NAME = "value"` to set a variable,
and `Remove-Item -Recurse -Force` to delete.

**What you can verify locally on Windows, and what you cannot:**

| Runs on Windows | Needs another machine |
|---|---|
| `.\scripts\check.ps1` — fmt, clippy, build, the core's tests | `swift test` (macOS only) |
| The whole core: transport, collectors, scheduler, FFI, pairing | `gradle :app:testDebugUnitTest` (needs the Android SDK) |
| The Windows app, its tests, and running it against a real host | Building the macOS, iOS or Android apps |
| Live SSH tests, with Docker Desktop + WSL | Building the Linux app (needs GTK 4.10+) |
| The device-pairing protocol end to end (`crates/sg-sync`) | Driving a camera to scan a QR |

That first row is most of the project. The core is where the parsing, scheduling, rate derivation,
health verdicts, wording and pairing live; the app layers are views over it. A Windows machine can
work on nearly everything and verify it properly.

**Do not stop at a green build.** The app has crashed on launch with everything compiling and every
test passing — a WinUI layout cycle, which is a runtime failure by construction. Start it against
the fixtures and look at it.

Do not claim a change is verified on the Apple or Android side from a Windows machine. Say which
platform you checked. The single most repeated mistake in this project's history is assuming the
same code shape behaves the same way on another platform — it has not, four separate times.

**Before pushing:** `.\scripts\check.ps1 -All`. CI has been left red for eight commits because tests
passing locally says nothing about `fmt` and `clippy`, and the script exists so that is one command
rather than four to remember.

**When a collector needs a shell sweep**, remember the rules in CLAUDE.md: every loop ends with
`exit 0`, and nothing from a host or a user is ever interpolated into the script. These are about
the *remote* shell — always POSIX `sh` on the monitored Linux host — regardless of what you are
developing on.

## The CI jobs

Both run on the `windows` shell runner rather than a container, which also means they keep working
while the image registry is unavailable; both install their own toolchain on first run, because a
shell runner starts with whatever the machine happens to have; and neither is `allow_failure`, since
a job that is allowed to fail is a job nobody reads.

`rust:windows` builds and tests the core. It is the portability guard, and it is kept narrow and
fast on purpose.

`windows:app` builds the app. It additionally installs the .NET SDK into the project directory, and
it checks one thing the core's job cannot: that the committed C# bindings still match the Rust they
were generated from. Generated code that is allowed to go stale is a crash at runtime and nothing at
build time, so `scripts\check-bindings.ps1` regenerates into a temporary file and fails if anything
moved. Into a temporary file, not over the committed one: a check that modifies what it is checking
cannot be run on a release branch.

They are split so the failure is legible: a red `rust:windows` means the core broke on a platform
nobody develops on, a red `windows:app` means the app did.

Both share one runner and therefore one build directory, and that has one sharp edge worth knowing
before it bites: a job begins with `git clean -ffdx`, and on Windows a file another process still
holds open cannot be deleted. `dotnet build` leaves MSBuild workers alive for minutes, so a job
starting moments after the app job died inside `get_sources` — before a line of its script — with
`failed to remove .dotnet\dotnet.exe: Invalid argument`. Because it only happened when two jobs ran
back to back, it read as flakiness rather than as something one job did to the next.

Both jobs now exclude the cached toolchains from the clean (`GIT_CLEAN_FLAGS`), and `windows:app`
turns off MSBuild's node reuse and shuts the build servers down in `after_script`, so it leaves
nothing holding a handle. If a Windows job ever fails in seconds with an empty-looking log, read the
`get_sources` section rather than the script.

## The Windows app

```
apps/windows/
  ServerGlass.Core/        the bridge to Rust: P/Invoke, the view models, the stores
  ServerGlass.Core.Tests/  its tests, including ones that call the real sg_ffi.dll
  ServerGlass/             WinUI 3 on .NET 9, unpackaged
```

Build and run it:

```powershell
.\scripts\build-windows.ps1                    # debug
.\scripts\build-windows.ps1 -Release -Publish  # a folder you can copy anywhere
.\scripts\package-windows.ps1 -Install         # an installer, installed here
```

`WindowsPackageType=None` and `WindowsAppSDKSelfContained=true`, so it runs from a folder with no
MSIX identity and no signing certificate, and carries its own copy of the Windows App SDK. That is
also what makes it buildable on a CI runner that has nothing but an SDK.

The .NET runtime is the exception, and the distinction matters when you hand the folder to someone.
An ordinary build is framework-dependent: it needs the .NET 9 **Desktop** Runtime, which every
machine with the SDK already has and almost no other machine does — so it launches for whoever
built it and fails for whoever received it. `-Publish` therefore passes `SelfContained=true` and
carries the runtime too, which is what takes the folder from about 20 MB to about 220 MB.

Publishing has one more trap, and it is the reason `ServerGlass.csproj` carries a target called
`PublishTheCompiledXaml`. `dotnet publish` drops `ServerGlass.pri` and the `.xbf` files that
`dotnet build` writes beside the executable — the compiled XAML and the index that finds it — so
the app crashes in `MainWindow.InitializeComponent` with `XamlParseException` the moment it is
started from a published folder. The .NET SDK does not publish `.pri` files because they used to be
UWP-only; the Windows App SDK ships a target that adds them back, but only inside the MSIX
packaging targets, which an unpackaged app never imports. The csproj target closes that seam and
fails the build if either input is missing. `scripts\package-windows.ps1` checks the finished
payload for them as well, because this shipped once.

### Making an installer

```powershell
.\scripts\package-windows.ps1              # dist\ServerGlass-<version>-windows-x64-setup.exe
.\scripts\package-windows.ps1 -Install     # and install it on this machine
.\scripts\package-windows.ps1 -SkipBuild   # package the publish folder that is already there
```

Inno Setup compiles it (`winget install JRSoftware.InnoSetup`), from
`apps\windows\installer\ServerGlass.iss`. The version comes from `Cargo.toml` and is passed to both
the compiler and the build, so the installer, the Apps & Features entry and the executable's own
file version are the same number — nothing here has a second copy to forget. Left to itself MSBuild
stamps `1.0.0.0`, which is what a crash report would otherwise quote.

The install is **per-user**: `%LOCALAPPDATA%\Programs\ServerGlass`, a shortcut in your own Start
Menu, no administrator and no UAC prompt — the bargain `scripts/build-linux.sh --install` makes
with `~/.local`. Everything the app stores is per-user already, including DPAPI blobs no other
account can decrypt, so a machine-wide install would buy nothing.

Two things it deliberately does not do. It does not delete `%LOCALAPPDATA%\ServerGlass` on
uninstall — the host inventory, the pinned host keys and the encrypted credentials live there, and
someone upgrading should not have to re-approve every host key. And it is not signed, so SmartScreen
shows "Windows protected your PC" on the first run and needs *More info > Run anyway*; that is the
same missing certificate as the un-notarised macOS build, not a fault in the file.

This is not part of `scripts/release.sh`, which builds the macOS and Android artefacts, because an
Inno Setup installer has to be compiled on Windows and that script runs on a Mac. Nothing on the
releases page is a Windows build.

### The icon

`apps\windows\ServerGlass\Assets\ServerGlass.ico` is generated, by
`.\scripts\make-windows-icon.ps1`, from the 1024px master that `scripts/make-icons.swift` draws.
The mark exists in exactly one place — that Swift file — and this only reshapes its output into the
container Windows wants, because CoreGraphics does not run here. Regenerate it if the mark changes;
nothing in the build does it for you.

An unpackaged app has no manifest for the shell to read, so the executable's own icon resource is
the only one there is: `<ApplicationIcon>` in the csproj feeds the taskbar, Alt-Tab, the Start Menu
shortcut and the installer alike.

### How it reaches the core

Not through UniFFI. UniFFI has no C# backend, and `uniffi-bindgen-cs` — the third-party generator
this document used to say to evaluate — is a version behind: its latest release targets uniffi
0.31 while this workspace is on 0.32, and with no `.udl` in the tree there is no non-library mode
to fall back on. So the documented fallback is what ships: a hand-written `extern "C"` surface in
`crates/sg-ffi/src/cabi.rs`, with `csbindgen` generating the C# declarations from those signatures.

The payload crosses as **UTF-8 JSON in an `{ok|err}` envelope**, not as `#[repr(C)]` structs. A
`TargetSnapshot` nests vectors of records inside vectors of records, an `Option<f64>` and a fielded
enum; describing that layout by hand in two languages and keeping the copies in step is the failure
mode this project already has a list entry for. With JSON there is nothing to keep in step, and the
contract is testable rather than merely agreed-by-inspection —
`field_set_is_asserted_so_a_new_field_fails_here` pins the exact key set, the same guard
`crates/sg-sync` puts on the pairing wire format.

The generated `NativeMethods.g.cs` is committed, exactly as the Swift bindings are, so a checkout
builds without running a generator first. `scripts\check-bindings.ps1` proves it is current.

### What the app layer is allowed to decide

Colour from a level string, and layout. That is all — the same rule as every other front-end.
`sg_format`, `sg_format_duration` and `sg_sparkline_points` are exported precisely so this layer
cannot re-derive a number, a unit or a chart's scale. Two of those were found the hard way here:
uptime rendered as `11324 s` where the phone says `3h 8m`, because the Apple and Android layers
special-case `metric == "uptime"` and this one initially did not.

### Storage

`HostStore` keeps the host records as JSON under `%LOCALAPPDATA%\ServerGlass`, and the secrets —
passwords, key passphrases and pasted private keys — under **DPAPI**, scoped to the current user.
`SavedHost` has no field for a secret, which is the point: it cannot reach disk in the clear by
accident.

DPAPI rather than the Credential Manager, which is the closer analogue to the Keychain and was the
first implementation: `CredWriteW` caps a credential blob at 2560 bytes, and a pasted private key —
the whole reason the `key_text` sign-in method exists — is larger than that for anything but a short
ed25519 key. It failed for exactly the secret the store exists to hold. A test covers a
multi-kilobyte key so that cannot come back.

### What it does not do

It cannot scan a QR: a desktop has no camera. It renders one to receive an inventory, and takes a
pasted pairing code to send one. Nothing else from the other platforms is missing.


## The fixtures will not start: reserved port ranges

`./fixtures/up.sh` can fail with `ports are not available: exposing port TCP 127.0.0.1:2223`, with
nothing listening on the port and nothing in `netstat`. Windows reserves blocks of TCP ports for
Hyper-V, and the fixture ports 2222 and 2223 sit inside one of them on some machines:

```powershell
netsh interface ipv4 show excludedportrange protocol=tcp
```

A range such as `2180-2279` covers both. The reservation is made by the Host Network Service when
it starts, so it moves between reboots; freeing it needs an elevated `net stop winnat`, a restart,
or an explicit `netsh int ipv4 add excludedportrange ... store=persistent` claiming the ports back
before HNS takes them.

This is a machine condition, not a broken fixture — check it before concluding the containers are
at fault. The Linux app's live tests accept `SG_FIXTURE_PORT` so they can be pointed at a fixture
published somewhere outside the reserved range; the core's own live tests hardcode 2222 and 2223.
