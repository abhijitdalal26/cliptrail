plugins {
    id("com.android.application")
}

android {
    namespace = "dev.cliptrail.spike"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.cliptrail.spike"
        minSdk = 29
        // Stay below 37: Android 17's local-network permission (docs/android-plan.md 4.8).
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0-spike"
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

    lint {
        lintConfig = file("lint.xml")
        abortOnError = true
        checkDependencies = false
    }
}

dependencies {
    // No runtime dependencies on purpose: framework Views only, smallest APK, nothing to vet.
    testImplementation("junit:junit:4.13.2")
}
