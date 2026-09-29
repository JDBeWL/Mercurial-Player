import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// 正式发布用的签名配置，放在 gen/android/keystore.properties（已 gitignore，绝不入库）：
//   storeFile=/绝对路径/xx.jks
//   storePassword=...
//   keyAlias=...
//   keyPassword=...
// 没有这个文件时（本地测试/模拟器），release 包回退用 AGP 内置的 debug 密钥签名。
// 这样 release 包也能直接安装；代价是 debug 密钥签出的包不能上架，
// 且无法覆盖安装用正式密钥签过的同包名应用。
val keystorePropertiesFile = rootProject.file("keystore.properties")
val keystoreProperties = Properties().apply {
    if (keystorePropertiesFile.exists()) {
        keystorePropertiesFile.inputStream().use { load(it) }
    }
}

android {
    compileSdk = 36
    namespace = "com.jdbewl.mercurial_player"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.jdbewl.mercurial_player"
        // 与 tauri.android.conf.json 的 minSdkVersion 保持一致（通知渠道自 API 26 起必需）
        minSdk = 26
        // 固定 NDK：cpal 的 AAudio 后端链接 -laaudio，该库自 API 26 起才在 sysroot 提供；
        // cargo-ndk 默认 -P 21，必须显式传 -P 26（或 CARGO_NDK_PLATFORM=26），否则链接失败。
        // 版本要和「真正编译 Rust 的那个 NDK」一致：tauri CLI 自己挑 NDK，规则是取
        // $ANDROID_HOME/ndk 下已安装的最高版本（本机为 30.0.14904198），故此处同步写 30。
        // 若想改回 27：要么卸掉更高版本，要么设环境变量 NDK_HOME 指向它（tauri CLI 认这个变量）。
        ndkVersion = "30.0.14904198"
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    signingConfigs {
        // 只有存在 keystore.properties 时才创建 release 签名配置，避免空配置导致构建报错
        if (keystorePropertiesFile.exists()) {
            create("release") {
                storeFile = rootProject.file(keystoreProperties.getProperty("storeFile"))
                storePassword = keystoreProperties.getProperty("storePassword")
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("keyPassword")
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            // 有正式密钥就用它；没有就让本地 release 包也能装（回退 debug 密钥）
            signingConfig = if (keystorePropertiesFile.exists()) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("androidx.documentfile:documentfile:1.1.0")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    // 阶段 3：MediaSessionCompat / MediaStyle 通知 / MediaButtonReceiver（线控）
    implementation("androidx.media:media:1.7.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")