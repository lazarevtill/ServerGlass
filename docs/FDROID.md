# Publishing to F-Droid

What is ready, what still has to be done by hand, and what would get the submission rejected.

## What F-Droid requires, and where this repository stands

| Requirement | Status |
|---|---|
| Public source repository | Ready once the GitHub mirror is live — see [MIRRORING.md](MIRRORING.md) |
| FOSS licence file in the repo | [LICENSE-MIT](../LICENSE-MIT) and [LICENSE-APACHE](../LICENSE-APACHE) |
| Only FOSS dependencies | androidx (Apache-2.0) and JNA (Apache-2.0 / LGPL). No Google Play Services, no Firebase |
| No prebuilt binaries or blobs in the repo | Verified: `git ls-files` matches no `.so`, `.jar` or `.aar` |
| A git tag per release, matching the version | `v0.3.0` = versionName 0.3.0, versionCode 5 |
| `fastlane/metadata/android/en-US/` | Present: descriptions, icon, three phone screenshots, changelog |
| Build recipe | [`docs/fdroid/cloud.lazarev.serverglass.yml`](fdroid/cloud.lazarev.serverglass.yml) — **written but not yet tested** |

## The part that needs doing: test the recipe

The recipe has not been through `fdroid build`. It almost certainly needs adjusting, because the
build is not a plain Gradle build — the core is Rust, cross-compiled to the NDK, and the Kotlin
bindings are generated from a host build of the same library. Everything is built from source in
the repository, which is what F-Droid requires; but the toolchain has to be installed inside their
buildserver first, and the exact incantation depends on their image.

```bash
git clone https://gitlab.com/fdroid/fdroidserver.git
git clone https://gitlab.com/fdroid/fdroiddata.git
cd fdroiddata
cp /path/to/serverglass/docs/fdroid/cloud.lazarev.serverglass.yml metadata/
../fdroidserver/fdroid build -v -l cloud.lazarev.serverglass
```

Expect to iterate on:

- **Rust installation.** The recipe installs rustup in `sudo:`, which runs as root, and then uses
  `$HOME/.cargo/bin` in `prebuild:`, which does not. If the paths do not line up, install to a
  fixed location such as `/opt/rust` with `RUSTUP_HOME` and `CARGO_HOME` set explicitly.
- **The NDK.** `cargo-ndk` needs `ANDROID_NDK_HOME`. The buildserver provides an NDK; the version
  may not be the 27.3.13750724 this project develops against, and `-P 26` must keep matching
  `minSdk` in `app/build.gradle.kts`.
- **The host library extension.** The bindings step reads `target/debug/libsg_ffi.so` — on Linux.
  The local script says `.dylib` because it runs on macOS. That difference is already handled in
  the recipe and is the kind of thing to check first if the step fails.
- **`scanignore`.** The generated `.so` lands in `jniLibs`, which F-Droid's scanner flags as a
  binary. It is produced by the prebuild step from source in the same repository, and the
  `scanignore` entry says so. If the scanner objects to anything else, do not widen this blindly —
  work out what it found.

## Submitting

1. Fork <https://gitlab.com/fdroid/fdroiddata>.
2. Add `metadata/cloud.lazarev.serverglass.yml`.
3. Open a merge request with the **New App** label.
4. Expect 24–48 hours from approval to the app appearing, once builds succeed.

## Keeping it updated

`UpdateCheckMode: Tags ^v[0-9.]+$` means F-Droid watches for new `vX.Y.Z` tags, which is what
`scripts/release.sh` already creates. `AutoUpdateMode: Version` then writes the next build entry
itself. In practice a new release needs:

- the tag, which the release script makes;
- a changelog at `fastlane/metadata/android/en-US/changelogs/<versionCode>.txt`, **under 500
  characters** — F-Droid truncates silently past that;
- nothing else, unless the build steps changed.

## The mirror rewrites history — what that means here

`scripts/mirror-to-github.sh` strips the GitLab CI config from every commit before publishing, so
the public tags are *not* the same objects as the private ones. That is fine for F-Droid, which
builds from the public repository, with one caveat worth knowing before it bites:

`git filter-branch` is deterministic — the same history through the same filter produces the same
hashes — so as long as commits are only ever appended, an already-published tag keeps pointing at
the same commit. **Changing the strip list rewrites every hash**, which moves every tag, and
F-Droid pins each build to the commit a tag resolved to. If the strip list has to change, expect to
tell F-Droid about it rather than assuming the rebuild is silent.

## What would get this rejected

- **Adding a non-free dependency.** Google Play Services and Firebase are named explicitly in
  F-Droid's guidance. Push notifications, crash reporting and analytics are the usual routes in;
  this app has none of the three and should not acquire them.
- **Committing a built `.so`.** The current build generates it. Committing one to make CI simpler
  would turn a from-source build into a blob.
- **Downloading anything at build time.** Gradle dependencies come from Maven, which F-Droid
  mirrors; anything fetched by a script during the build is a problem.
- **A tag that does not match the version in the manifest.** `versionName` in
  `app/build.gradle.kts` and the tag must agree.
