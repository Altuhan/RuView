import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.kite.pos"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.kite.pos"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk {
            // Phones/tablets and the x86_64 emulator.
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets["main"].jniLibs.srcDir(layout.buildDirectory.dir("rustJniLibs"))
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

dependencies {
    androidTestImplementation("androidx.test:core:1.6.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("junit:junit:4.13.2")
}

// Builds libkitepos.so for each ABI with cargo-ndk (`cargo install cargo-ndk`).
// The NDK is found through ANDROID_NDK_HOME or the SDK's ndk/ directory.
// Pass -PskipCargo to package prebuilt libraries instead.
val cargoBuild by tasks.registering(Exec::class) {
    val outDir = layout.buildDirectory.dir("rustJniLibs").get().asFile
    workingDir = rootDir.resolve("../..")
    commandLine(
        "cargo", "ndk",
        "-t", "arm64-v8a", "-t", "x86_64",
        "--platform", "24",
        "-o", outDir.absolutePath,
        "build", "--release", "-p", "kite-example-android-pos",
    )
}

if (!project.hasProperty("skipCargo")) {
    tasks.named("preBuild") { dependsOn(cargoBuild) }
}
