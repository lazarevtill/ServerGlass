plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "cloud.lazarev.serverglass"
    compileSdk = 37

    defaultConfig {
        applicationId = "cloud.lazarev.serverglass"
        minSdk = 26
        targetSdk = 35
        versionCode = 6
        versionName = "0.4.0"
    }

    // Android will not install an unsigned APK at all — unlike macOS, there is no unsigned
    // option. `scripts/release.sh` generates a local, gitignored key on first run; without it the
    // release build still succeeds and produces an unsigned APK for the script to sign itself.
    val keystorePath = System.getenv("SG_KEYSTORE")
    signingConfigs {
        if (keystorePath != null && file(keystorePath).exists()) {
            create("release") {
                storeFile = file(keystorePath)
                storePassword = System.getenv("SG_KEYSTORE_PASSWORD") ?: "serverglass"
                keyAlias = System.getenv("SG_KEY_ALIAS") ?: "serverglass"
                keyPassword = System.getenv("SG_KEY_PASSWORD") ?: "serverglass"
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures {
        compose = true
    }

    sourceSets["main"].apply {
        // The Rust .so, placed here by cargo-ndk via scripts/build-android.sh.
        jniLibs.directories.add("src/main/jniLibs")
        // The generated UniFFI bindings, kept out of the hand-written source tree.
        kotlin.directories.add("build/generated/uniffi")
    }

    packaging {
        jniLibs {
            // The core is a static Rust archive with no compression benefit worth the unpack cost.
            useLegacyPackaging = false
        }
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2026.09.00")
    implementation(composeBom)

    implementation("androidx.core:core-ktx:1.19.1")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")

    // Foldables. `window` exposes the hinge as a FoldingFeature; `material3-window-size-class`
    // turns the current width into the compact/medium/expanded buckets the layout switches on.
    implementation("androidx.window:window:1.3.0")
    implementation("androidx.compose.material3:material3-window-size-class")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")

    // The record format is pure Kotlin plus org.json, so it is tested on the JVM rather than on a
    // device: a test that needs an emulator is a test that does not run.
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20260814")

    // Passwords and passphrases, encrypted with a key held in the Android Keystore.
    implementation("androidx.security:security-crypto:1.1.0")

    // UniFFI's Kotlin bindings call into the .so through JNA; the `@aar` classifier is the
    // Android-native build, and the plain jar will not load at runtime.
    implementation("net.java.dev.jna:jna:5.19.1@aar")
}
