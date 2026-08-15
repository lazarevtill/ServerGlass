// ServerGlass for Android.
//
// Built by ./scripts/build-android.sh, which cross-compiles the Rust core with cargo-ndk,
// generates the Kotlin bindings from it, and then runs Gradle. Running Gradle on its own works
// only if those two steps have already happened.

pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "ServerGlass"
include(":app")
