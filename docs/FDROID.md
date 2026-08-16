# Publishing to F-Droid

What is ready, what still has to be done by hand, and what would get the submission rejected.

## What F-Droid requires, and where this repository stands

| Requirement | Status |
|---|---|
| Public source repository | [github.com/lazarevtill/ServerGlass](https://github.com/lazarevtill/ServerGlass) |
| FOSS licence file in the repo | [LICENSE-MIT](../LICENSE-MIT) and [LICENSE-APACHE](../LICENSE-APACHE) |
| Only FOSS dependencies | androidx (Apache-2.0) and JNA (Apache-2.0 / LGPL). No Google Play Services, no Firebase |
| No prebuilt binaries or blobs in the repo | Verified: `git ls-files` matches no `.so`, `.jar`, `.aar` or `.apk` |
| A git tag per release, matching the version | `v0.4.0` = versionName 0.4.0, versionCode 6 |
| `fastlane/metadata/android/en-US/` | Title, both descriptions, icon, three phone screenshots, changelogs |
| Build recipe | [`docs/fdroid/cloud.lazarev.serverglass.yml`](fdroid/cloud.lazarev.serverglass.yml) — dry-run locally, not yet through `fdroid build` |

### Why the recipe targets v0.4.0 and not v0.3.0

v0.3.0 was the obvious candidate and is the wrong one: **it predates the submission material.** The
fastlane metadata, this document, the recipe itself and the GitHub Actions workflow all landed
afterwards, so a checkout of that tag contains no description, no screenshots and no changelog for
F-Droid to read. Pinning a build to it would have meant submitting an app whose store listing does
not exist in the commit being built.

## The dry run, and what it does and does not prove

The recipe's build steps have been run against a **clean clone** outside the development tree, which
is the failure mode a recipe most often has — steps that only work because of state left lying
around by a previous local build:

```bash
git clone --no-local . /tmp/fdroid-test && cd /tmp/fdroid-test
git checkout v0.4.0
cargo ndk -t arm64-v8a -P 26 -o apps/android/app/src/main/jniLibs build --release -p sg-ffi
cargo build -p sg-ffi
cargo run -q -p sg-bindgen --bin uniffi-bindgen -- generate \
    --library target/debug/libsg_ffi.so --language kotlin \
    --out-dir apps/android/app/build/generated/uniffi
(cd apps/android && gradle :app:assembleRelease)
```

That proves the Rust → NDK → bindgen → Gradle chain is self-contained. It does **not** prove the
recipe works in F-Droid's buildserver, because two things there are outside this test:

- **Where Rust ends up.** `sudo:` runs as root and `prebuild:` does not, so a toolchain installed
  into root's `$HOME` is invisible by the time it is used. The recipe therefore installs to
  `/opt/rust` and puts that on `PATH`, leaving `CARGO_HOME` alone so cargo can still write its
  registry cache as the build user. This was a real defect in the first draft of the recipe.
- **The NDK.** The recipe asks for `ndk: r27d` — 27.3.13750724, the version
  `scripts/build-android.sh` names — and passes `$$NDK$$` through as `ANDROID_NDK_HOME`. The point
  release is not load-bearing; `-P 26` matching `minSdk` is. If the buildserver image does not
  carry r27d, r27c is a safe substitution and this is the line to change.

To go further, run it in their buildserver:

```bash
git clone https://gitlab.com/fdroid/fdroidserver.git
git clone https://gitlab.com/fdroid/fdroiddata.git
cd fdroiddata
cp /path/to/serverglass/docs/fdroid/cloud.lazarev.serverglass.yml metadata/
../fdroidserver/fdroid build -v -l cloud.lazarev.serverglass
```

## Two things about the recipe that are easy to break

**`--bin uniffi-bindgen` is not optional.** `sg-bindgen` also ships a C# generator for the Windows
app, so an unqualified `cargo run -p sg-bindgen` is ambiguous and fails. This has already broken the
macOS, iOS and Android build scripts once. It matters most here because `AutoUpdateMode: Version`
copies the build block forward to the next release verbatim — an unqualified command would have sat
in the recipe working fine against a tag that had one binary, and failed on the first release that
had two, long after anyone was looking at it.

**`-P 26` must keep matching `minSdk`** in `apps/android/app/build.gradle.kts`. The NDK links
against that API level's libc, and a mismatch is a runtime loader failure rather than a build error.

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
- `versionCode` incremented in `app/build.gradle.kts` — F-Droid will not accept a release that
  reuses one;
- a changelog at `fastlane/metadata/android/en-US/changelogs/<versionCode>.txt`, **under 500
  characters** — F-Droid truncates silently past that, so count them rather than eyeballing it;
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
  mirrors; anything fetched by a script during the build is a problem. The Rust toolchain install
  in `sudo:` is the one exception, and is the normal arrangement for Rust apps in fdroiddata.
- **A tag that does not match the version in the manifest.** `versionName` in
  `app/build.gradle.kts` and the tag must agree.

### One dependency worth knowing about

`org.json:json` is a **`testImplementation`** dependency, used to test the record format on the JVM.
Its licence carries the "shall be used for Good, not Evil" clause, which the FSF and Debian both
treat as non-free. It is not a problem as things stand — F-Droid builds `assembleRelease`, which
never resolves test dependencies, and nothing from it reaches the APK. It is written down here so
that if it ever moves to `implementation`, the consequence is known in advance rather than
discovered in a rejected merge request.
