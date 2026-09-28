# Reliability and UI audit — 28 September 2026

Work is on `codex/reliability-and-ui`. All eight pull-request heads were fetched. PRs 1–4 were
already merged; the four open dependency PRs (5–8) were integrated locally. This audit does not
merge or close those PRs on GitHub.

## Reported macOS crash

The supplied report traps in Swift's `ServerGlass.addTarget(config:)` while restoring saved hosts.
The installed executable loads `libsg_ffi.dylib` from an absolute path in a development checkout.
SwiftPM's `-lsg_ffi` selected that dylib even though the build intended to embed the static core.
A rebuilt library can therefore change the native record layout underneath an installed app.

The package now links the archive explicitly. Packaging rejects any remaining `libsg_ffi` dynamic
dependency, verifies its signature, and takes the bundle version from the workspace. The rebuilt
app launches and collects live readings. A Swift regression test sends a complete saved-host
configuration through the native bridge. The external-library dependency is confirmed; the exact
historical library/bindings mismatch cannot be reconstructed from the crash report alone.

## Confirmed defects addressed

| Area | Failure | Change |
| --- | --- | --- |
| Dependency integration | The russh update no longer implemented the required host-key callback. | Support the new key/certificate argument while preserving leaf-key pinning. |
| Android build | AGP 9 conflicts with the old Kotlin Android plugin; current UI dependencies need a newer compile SDK. | Use AGP's built-in Kotlin support, register generated Kotlin sources correctly, and compile with SDK 37. Minimum and target SDK remain unchanged. |
| Linux build | Direct GLib 0.22 types conflict with GTK 0.9 and exceed the pinned Rust toolchain's minimum. | Align GLib with GTK's 0.20 dependency and regenerate the separate lockfile. |
| SSH trust | A failed pin write still permitted authentication; unreadable pins could look like first use. | Fail before authentication, report storage errors, and serialize pin checks/writes within the process. Preserve permissions on existing directories. |
| SSH framing | A command reading stdin could consume the rest of the batch script. | Give each command `/dev/null` as stdin. Test on Debian and Alpine. |
| SSH bounds | Streaming output repeatedly renewed its timeout and could grow memory indefinitely. | Use an absolute deadline, a 16 MiB output bound, and bounded authentication/channel startup. |
| Poll lifecycle | Stopped tasks could publish old state; queued commands could survive a restart. | Give each run its own command queue and generation, reject disconnected commands, and discard abandoned requests. |
| Reconnection | A command transport failure broke only the inner command loop. | Leave the refresh loop and rebuild the failed session. |
| Health | A healthy verdict could survive disconnection; detail gauges were omitted from assessment. | Recompute health on state transitions and assess all host gauges. |
| Metrics | Counter state and vanished metric histories accumulated as processes/devices changed. | Prune against each complete tick; rebaseline reused IDs and ignore non-finite samples. |
| Credential storage | Apple deleted a credential before knowing its replacement could be saved. Android silently fell back to plaintext storage. | Update Keychain items in place; require encrypted Android storage and migrate legacy values only after a successful encrypted write. |
| Inventory | Unreadable saved inventories could become an empty list and then be overwritten. | Report unreadable data and prevent normal save flows from replacing it on all four front-ends. |
| Windows lifetime | Disposing the native core could race with a background native call. | Use SafeHandle with a lease for every call, including blocking commands and pairing. |
| Windows polling | A background thread enumerated an ObservableCollection owned by the UI. | Copy hosts on the dispatcher, skip obsolete updates, and report unexpected polling failures. |
| Pairing | A TCP listener that never answered could block later addresses; transfers had no deadline. | Bound the full handshake and payload, verify the QR's public key, enforce offer expiry and payload limits. |
| IPv6 pairing | Offers advertised IPv6 addresses while the listener bound only IPv4. | Listen on both requested address families and advertise only sockets that were opened. |
| Apple interaction | Deleting several rows could use shifted indices; compact fleet cards did not navigate. | Capture IDs before deletion and use NavigationLink for phone fleet cards. |
| Presentation | Failed connections could show an endless spinner or stale fleet metrics; some proportions were drawn without a maximum. | Make empty states reflect connection health, hide stale fleet readings, and draw capacity widgets only for real fractions. |
| UI consistency | Process severity was calculated again in some front-ends; supporting text was faint. | Use the core's severity and improve text contrast across platforms, plus native text sizes for Apple host rows. |

The fixes retain read-only collection, one network round trip per refresh, in-memory samples,
device-local credentials, and shared Rust formatting/severity decisions.

## Verification

| Surface | Evidence |
| --- | --- |
| Rust workspace | `SG_REQUIRE_FIXTURES=1 ./scripts/check.sh`: format, Clippy, build, 285 tests including live Debian/Alpine tests, Linux lockfile resolution and version checks. The final pairing expiry adjustment also passed targeted tests and workspace Clippy. |
| macOS | Built and signed the bundle; inspected its linked libraries; 10 Swift tests; launched against the Debian fixture and inspected live technical/summary views. |
| iOS | Built for the arm64 simulator; launched on iPhone 17 Pro; verified live readings, back navigation and Add Host validation (port 0 disabled, valid port enabled). |
| Android | Built the arm64 debug APK; all 9 JVM tests passed; installed and launched on the foldable emulator with no recorded crash. This was a fixture launch, not a complete interactive Android UI test. |
| Linux | Built in a Rust 1.89 Debian container with GTK/libadwaita; format and Clippy passed with warnings denied; 24 unit tests and 4 live engine tests passed. |
| Windows bridge | Built the .NET projects and passed 24 tests against the real native library on macOS, including concurrent disposal. DPAPI tests were excluded because they require Windows. |
| CI configuration | Added the macOS packaging/bridge job and required live fixtures for Linux jobs; YAML parses locally. The updated workflow has not yet run on GitHub. |

## Limits and release follow-up

- WinUI build/launch, Windows installation and DPAPI must still be verified on Windows.
- Linux desktop rendering and complete Android interaction need native UI testing. An iPad and
  physical iOS device were not exercised.
- This pass is not proof that the repository contains no remaining bugs. It does not establish
  accessibility conformance, long-duration stability or an exhaustive security audit.
- Build artifacts remain local. The existing app in `/Applications` was not replaced,
  and no release was published. Review and CI status are tracked in the branch's pull request.

Useful local outputs are `target/ServerGlass.app`, the iOS simulator app under
`target/ios/DerivedData/Build/Products/Debug-iphonesimulator`, and
`apps/android/app/build/outputs/apk/debug/app-debug.apk`.
