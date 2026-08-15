plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "cloud.lazarev.serverglass"
    compileSdk = 35

    defaultConfig {
        applicationId = "cloud.lazarev.serverglass"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    buildFeatures {
        compose = true
    }

    sourceSets["main"].apply {
        // The Rust .so, placed here by cargo-ndk via scripts/build-android.sh.
        jniLibs.srcDir("src/main/jniLibs")
        // The generated UniFFI bindings, kept out of the hand-written source tree.
        java.srcDir("build/generated/uniffi")
    }

    packaging {
        jniLibs {
            // The core is a static Rust archive with no compression benefit worth the unpack cost.
            useLegacyPackaging = false
        }
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2025.01.00")
    implementation(composeBom)

    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("androidx.activity:activity-compose:1.9.3")
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

    // UniFFI's Kotlin bindings call into the .so through JNA; the `@aar` classifier is the
    // Android-native build, and the plain jar will not load at runtime.
    implementation("net.java.dev.jna:jna:5.15.0@aar")
}
