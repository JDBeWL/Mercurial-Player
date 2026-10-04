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

// 正式发布签名配置放在 gen/android/keystore.properties
// 缺该文件时 release 会回退 debug 密钥——Play 拒收，且换正式密钥后同包名无法覆盖安装；
// 故下面的校验让打包 release 显式失败，仅本机测试可加 -PallowDebugSigning=true 放行。
val keystorePropertiesFile = rootProject.file("keystore.properties")
val keystoreProperties = Properties().apply {
    if (keystorePropertiesFile.exists()) {
        keystorePropertiesFile.inputStream().use { load(it) }
    }
}
val allowDebugSigning = (findProperty("allowDebugSigning") as String?)?.toBoolean() == true

android {
    compileSdk = 36
    namespace = "com.jdbewl.mercurial_player"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.jdbewl.mercurial_player"
        // 与 tauri.android.conf.json 的 minSdkVersion 保持一致（通知渠道自 API 26 起必需）
        minSdk = 26
        // AAudio 自 API 26 起才在 sysroot 提供，cargo-ndk 必须显式传 -P 26（默认 21 链接失败）
        // ndkVersion 必须是真正编译 Rust 的那个 NDK：tauri CLI 取 $ANDROID_HOME/ndk 下的最高版本
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
            // 三个规则文件里有两个是生成物（proguard-tauri.pro、wry 的 proguard-wry.pro，
            // 含 native/Ipc/WryActivity 的 `-keep`，都被 .gitignore 排除）：用 fileTree 在配置期
            // 快照成文件列表，既不假设生成物已存在，又收窄为 *.pro + src/**/*.pro 并排除 build/。
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                *fileTree(".") {
                    include("*.pro", "src/**/*.pro")
                    exclude("build/**")
                }.files.toTypedArray(),
            )
        }
    }
    compileOptions {
        // Java 与 Kotlin 统一到 17
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

// 打包 release 前的签名闸门
gradle.taskGraph.whenReady {
    val buildingRelease =
        allTasks.any { task ->
            task.project == project &&
                task.name.contains("Release") &&
                (task.name.startsWith("assemble") ||
                    task.name.startsWith("bundle") ||
                    task.name.startsWith("package"))
        }
    if (buildingRelease && !keystorePropertiesFile.exists() && !allowDebugSigning) {
        throw GradleException(
            "缺少 ${keystorePropertiesFile.path}：release 包只能用 AGP 的 debug 密钥签名，" +
                "Google Play 会拒收，且无法覆盖安装正式密钥签过的应用。" +
                "请在该文件写入 storeFile/storePassword/keyAlias/keyPassword 后重试；" +
                "仅本机安装测试可加 -PallowDebugSigning=true 跳过此检查。"
        )
    }
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("androidx.documentfile:documentfile:1.1.0")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    // MediaSessionCompat / MediaStyle 通知 / MediaButtonReceiver（耳机线控）
    implementation("androidx.media:media:1.7.0")
    // 刻意不声明 testImplementation / androidTestImplementation
}

apply(from = "tauri.build.gradle.kts")