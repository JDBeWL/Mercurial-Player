//! Android SAF（Storage Access Framework）适配层。
//! 桌面端媒体路径为绝对路径，Android 分区存储下为 `content://` URI；本模块经 JNI 调 Kotlin
//! `SafBridge` 把 URI 转成 native fd 再包成 `File`，解码/元数据/封面因此复用桌面代码。

use crate::error::AppError;

/// 判断路径是否为 Android SAF 的 content URI
#[cfg(target_os = "android")]
pub fn is_content_uri(path: &str) -> bool {
    path.starts_with("content://")
}

/// 非 Android 平台恒为 false
#[cfg(not(target_os = "android"))]
pub fn is_content_uri(_path: &str) -> bool {
    false
}

#[cfg(target_os = "android")]
#[allow(unsafe_code)] // JNI/fd 桥接必须使用 unsafe，见各调用点 SAFETY 注释
mod android_impl {
    use super::SafEntry;
    use super::SafPickState;
    use super::is_content_uri;
    use crate::error::AppError;
    use std::fs::File;
    use std::os::fd::FromRawFd;

    /// 获取当前保存的(content URI)音乐目录树
    pub fn get_saved_tree() -> Result<Option<String>, AppError> {
        let _ = is_content_uri; // 占位避免未用警告，实际逻辑在 JNI 侧
        let s = jni_call_string("getSavedTreeUri", &[])?;
        Ok(if s.is_empty() { None } else { Some(s) })
    }

    /// 授权状态快照（URI + 版本号 + 显示名）
    pub fn get_pick_state() -> Result<SafPickState, AppError> {
        let json = jni_call_string("getPickState", &[])?;
        let v: serde_json::Value = serde_json::from_str(&json)
            .map_err(|e| AppError::msg(format!("SAF 状态 JSON 无效: {e}")))?;
        let uri = v.get("uri").and_then(|u| u.as_str()).unwrap_or_default();
        let display = v
            .get("displayName")
            .and_then(|u| u.as_str())
            .unwrap_or_default();
        Ok(SafPickState {
            uri: if uri.is_empty() {
                None
            } else {
                Some(uri.to_string())
            },
            version: v.get("version").and_then(|u| u.as_i64()).unwrap_or(0),
            display_name: if display.is_empty() {
                None
            } else {
                Some(display.to_string())
            },
        })
    }

    /// 清除已保存的树 URI
    pub fn clear_saved_tree() -> Result<(), AppError> {
        jni_call_static_noop("com/jdbewl/mercurial_player/SafBridge", "clearSavedTree")
    }

    /// 获取应用数据目录（files 父级 data 目录，写配置/缓存用）
    pub fn get_app_data_dir() -> Result<String, AppError> {
        jni_call_string("getAppDataDir", &[])
    }

    /// 调起系统目录选择器（异步；结果写入 Kotlin 侧持久化存储）
    pub fn request_pick() -> Result<(), AppError> {
        // 通过 MainActivity 静态方法转发，确保在主线程触发 Activity Result API
        jni_call_static_noop("com/jdbewl/mercurial_player/MainActivity", "safRequestPick")
    }

    /// 枚举已授权树下的音频文件
    pub fn list_audio_files(tree_uri: &str) -> Result<Vec<SafEntry>, AppError> {
        let json = jni_call_string("listAudioFiles", &[tree_uri])?;
        parse_audio_entry_list(&json)
    }

    fn parse_audio_entry_list(json: &str) -> Result<Vec<SafEntry>, AppError> {
        let v: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| AppError::msg(format!("SAF 音频列表 JSON 无效: {e}")))?;
        let arr = v
            .as_array()
            .ok_or_else(|| AppError::msg("SAF 音频列表不是数组"))?;
        arr.iter()
            .map(|item| {
                let uri = item
                    .get("uri")
                    .and_then(|u| u.as_str())
                    .ok_or_else(|| AppError::msg("SAF 条目缺少 uri"))?
                    .to_string();
                let name = item
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or_default()
                    .to_string();
                let folder = item
                    .get("folder")
                    .and_then(|f| f.as_str())
                    .unwrap_or_default()
                    .to_string();
                let folder_path = item
                    .get("folderPath")
                    .and_then(|f| f.as_str())
                    .unwrap_or_default()
                    .to_string();
                Ok(SafEntry {
                    uri,
                    name,
                    folder,
                    folder_path,
                })
            })
            .collect()
    }

    /// 打开 content URI，返回包装好的 `File`（fd 所有权在 Rust 侧，随 File drop 关闭）
    pub fn open_media_file(path: &str) -> Result<File, AppError> {
        debug_assert!(is_content_uri(path));
        let fd = jni_call_int("openFileFd", &[path])?;
        if fd < 0 {
            return Err(AppError::msg(format!("无法打开 SAF 文件: {path}")));
        }
        // SAFETY: detachFd 已经把 fd 所有权移交给本进程；此处包装进 File 负责关闭
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    /// 打开 content URI 用于写入，返回包装好的 `File`（fd 所有权在 Rust 侧）
    ///
    /// Kotlin 侧用 `"wt"` 打开（写 + 截断），见 `SafBridge.openOutputFd`。
    pub fn open_output_file(path: &str) -> Result<File, AppError> {
        debug_assert!(is_content_uri(path));
        let fd = jni_call_int("openOutputFd", &[path])?;
        if fd < 0 {
            return Err(AppError::msg(format!("无法写入 SAF 文件: {path}")));
        }
        // SAFETY: detachFd 已经把 fd 所有权移交给本进程；此处包装进 File 负责关闭
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    // JNI 封装

    // 共享 JNI 封装：with_jni / app_class 见 crate::android::java_bridge
    use crate::android::java_bridge::{app_class, with_jni};

    /// 调用 SafBridge 返回 String 的静态方法
    pub fn jni_call_string(method: &str, args: &[&str]) -> Result<String, AppError> {
        use jni::objects::{JObject, JString, JValue};
        with_jni(|env| {
            let class = app_class(env, "com/jdbewl/mercurial_player/SafBridge")?;
            // JObject 需在调用期间存活：先收集到 Vec，再借用
            let mut objs: Vec<JObject> = Vec::with_capacity(args.len());
            for a in args {
                let js = env
                    .new_string(a)
                    .map_err(|e| AppError::msg(format!("new_string 失败: {e}")))?;
                objs.push(js.into());
            }
            let jargs: Vec<JValue> = objs.iter().map(JValue::Object).collect();
            let sig = format!(
                "({})Ljava/lang/String;",
                "Ljava/lang/String;".repeat(args.len())
            );
            let ret = env
                .call_static_method(&class, method, &sig, &jargs)
                .map_err(|e| AppError::msg(format!("调用 {method} 失败: {e}")))?;
            let obj: JObject = ret
                .l()
                .map_err(|e| AppError::msg(format!("{method} 返回类型不符: {e}")))?;
            // Kotlin 可能返回 null（如无已保存的树 URI），此时按空字符串处理
            if obj.is_null() {
                return Ok(String::new());
            }
            let s: JString = obj.into();
            let s = env
                .get_string(&s)
                .map_err(|e| AppError::msg(format!("{method} 读取字符串失败: {e}")))?;
            Ok(s.to_string_lossy().to_string())
        })
    }

    /// 调用 SafBridge 返回 Int 的静态方法
    fn jni_call_int(method: &str, args: &[&str]) -> Result<i32, AppError> {
        use jni::objects::{JObject, JValue};
        with_jni(|env| {
            let class = app_class(env, "com/jdbewl/mercurial_player/SafBridge")?;
            // JObject 需在调用期间存活：先收集到 Vec，再借用
            let mut objs: Vec<JObject> = Vec::with_capacity(args.len());
            for a in args {
                let js = env
                    .new_string(a)
                    .map_err(|e| AppError::msg(format!("new_string 失败: {e}")))?;
                objs.push(js.into());
            }
            let jargs: Vec<JValue> = objs.iter().map(JValue::Object).collect();
            let sig = format!("({})I", "Ljava/lang/String;".repeat(args.len()));
            let ret = env
                .call_static_method(&class, method, &sig, &jargs)
                .map_err(|e| AppError::msg(format!("调用 {method} 失败: {e}")))?;
            ret.i()
                .map_err(|e| AppError::msg(format!("{method} 返回类型不符: {e}")))
        })
    }

    /// 调用任意类无参 void 静态方法（用于触发 UI 动作）
    fn jni_call_static_noop(class_name: &str, method: &str) -> Result<(), AppError> {
        with_jni(|env| {
            let class = app_class(env, class_name)?;
            env.call_static_method(&class, method, "()V", &[])
                .map_err(|e| AppError::msg(format!("调用 {method} 失败: {e}")))?;
            Ok(())
        })
    }
}

/// Android 下的统一媒体文件打开入口：content URI 走 JNI fd 桥，其余走本地路径
#[cfg(target_os = "android")]
pub fn open_media_file(path: &str) -> Result<std::fs::File, AppError> {
    if is_content_uri(path) {
        android_impl::open_media_file(path)
    } else {
        std::fs::File::open(path).map_err(|e| e.to_string().into())
    }
}

/// 非 Android 直接走本地路径（桌面零改动）
#[cfg(not(target_os = "android"))]
pub fn open_media_file(path: &str) -> Result<std::fs::File, AppError> {
    std::fs::File::open(path).map_err(|e| e.to_string().into())
}

/// Android 下的统一**写文件**入口：content URI 走 JNI fd 桥，其余走本地路径。
/// 存在的理由：安卓端「提取封面」拿到的是 `content://`，按路径 `fs::write` 必然失败且用户无感知。
#[cfg(target_os = "android")]
pub fn open_write_file(path: &str) -> Result<std::fs::File, AppError> {
    if is_content_uri(path) {
        android_impl::open_output_file(path)
    } else {
        std::fs::File::create(path).map_err(|e| e.to_string().into())
    }
}

/// 非 Android 直接走本地路径（桌面零改动）
#[cfg(not(target_os = "android"))]
pub fn open_write_file(path: &str) -> Result<std::fs::File, AppError> {
    std::fs::File::create(path).map_err(|e| e.to_string().into())
}

/// content URI 的显示名（非 Android 或无名字时返回 `None`）。
/// URI 本身看不出文件名（document id 可能是 `msf:1000000021` 这种），只能回查 provider。
#[cfg(target_os = "android")]
pub fn content_uri_display_name(uri: &str) -> Option<String> {
    android_impl::jni_call_string("getDisplayName", &[uri])
        .ok()
        .filter(|name| !name.is_empty())
}

/// 非 Android 没有 content URI
#[cfg(not(target_os = "android"))]
pub fn content_uri_display_name(_uri: &str) -> Option<String> {
    None
}

/// 调起系统目录选择器（仅 Android 生效；桌面端返回 Ok 无操作）
pub fn request_pick_directory() -> Result<(), AppError> {
    #[cfg(target_os = "android")]
    {
        android_impl::request_pick()
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(())
    }
}

/// 获取当前保存的音乐目录树 URI（仅 Android；桌面端返回 None）
pub fn get_saved_tree_uri() -> Result<Option<String>, AppError> {
    #[cfg(target_os = "android")]
    {
        android_impl::get_saved_tree()
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(None)
    }
}

/// 获取应用数据目录（仅 Android；桌面端返回 None）
pub fn get_app_data_dir() -> Result<Option<String>, AppError> {
    #[cfg(target_os = "android")]
    {
        let s = android_impl::get_app_data_dir()?;
        Ok(if s.is_empty() { None } else { Some(s) })
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(None)
    }
}

/// SAF 树枚举出的单个音频条目
///
/// `folder` 是直接父目录的显示名，`folder_path` 是相对树根的目录路径（可多级）。
#[derive(Debug, Clone)]
pub struct SafEntry {
    /// content:// 文档 URI（播放/元数据读取都以它为准）
    pub uri: String,
    /// Kotlin 给出的显示文件名
    pub name: String,
    /// 直接父目录显示名（根目录下的文件为树根可读名）
    pub folder: String,
    /// 相对树根的目录路径，空串表示根目录
    pub folder_path: String,
}

/// 枚举已授权树下的音频文件（仅 Android；桌面端返回空）
pub fn list_audio_files(uri: &str) -> Result<Vec<SafEntry>, AppError> {
    #[cfg(target_os = "android")]
    {
        android_impl::list_audio_files(uri)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = uri;
        Ok(Vec::new())
    }
}

/// SAF 授权状态快照
///
/// `version` 每次成功授权都会递增：重新授权**同一个**目录时 URI 不变，
/// 前端只能靠 version 判定"选择器已返回"。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafPickState {
    /// 已保存的树 URI（未授权则 None）
    pub uri: Option<String>,
    /// 授权版本号
    pub version: i64,
    /// 树的可读显示名（如 Music）
    pub display_name: Option<String>,
}

/// 获取 SAF 授权状态（仅 Android）
pub fn get_pick_state() -> Result<SafPickState, AppError> {
    #[cfg(target_os = "android")]
    {
        android_impl::get_pick_state()
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(SafPickState::default())
    }
}

/// 清除已保存的树 URI（移除音乐目录时同步清理，避免残留状态）
pub fn clear_saved_tree() -> Result<(), AppError> {
    #[cfg(target_os = "android")]
    {
        android_impl::clear_saved_tree()
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(())
    }
}

/// 已保存树的可读显示名（用于 UI 展示，避免把 content:// URI 直接显示给用户）
pub fn get_saved_tree_display_name() -> Result<Option<String>, AppError> {
    #[cfg(target_os = "android")]
    {
        let s = android_impl::jni_call_string("getSavedTreeDisplayName", &[])?;
        Ok(if s.is_empty() { None } else { Some(s) })
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(None)
    }
}

/// 把 URL 编码的百分号序列解成原始字符。
/// `document id` 常把分隔符编码（`primary%3AMusic%2FSong.mp3`），不解码就会显示成一串编码。
#[must_use]
pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// 由 content URI 推出可读文件名（含扩展名）
///
/// `content://.../document/primary%3AMusic%2FAlbum%2FSong.mp3` → `Song.mp3`
#[must_use]
pub fn display_name_from_document_uri(uri: &str) -> String {
    let last = uri.rsplit('/').next().unwrap_or("");
    let decoded = percent_decode(last);
    let file_name = decoded.rsplit(['/', '\\']).next().unwrap_or(&decoded);
    file_name.to_string()
}

/// 从树 URI 直接推导可读名（`.../tree/primary%3AMusic` → `Music`）
///
/// 不经过 JNI，任何树 URI 都能用；仅在 Kotlin 侧取不到显示名时兜底。
#[must_use]
pub fn display_name_from_tree_uri(uri: &str) -> String {
    let tail = uri.rsplit("/tree/").next().unwrap_or("");
    // document id 可能含编码后的分隔符（primary%3ADownload%2FMusic），先解码；
    // 再依次取"最后一个 / 之后"和"最后一个 : 之后"，两者都要考虑
    // （SD 卡树形如 "1A1B-2C3D:Music"，外置目录形如 "primary:Download/Music"）
    let decoded = percent_decode(tail);
    let after_slash = decoded.rsplit(['/', '\\']).next().unwrap_or(&decoded);
    let name = after_slash.rsplit(':').next().unwrap_or(after_slash);
    if name.is_empty() {
        decoded
    } else {
        name.to_string()
    }
}
#[cfg(test)]
mod tests {
    use super::{display_name_from_document_uri, display_name_from_tree_uri, percent_decode};

    /// document id 把分隔符编码了：直接取 URI 末段会得到一串编码，必须先解码
    #[test]
    fn document_uri_解码为可读文件名() {
        let uri = "content://com.android.externalstorage.documents/document/primary%3AMusic%2FAlbum%2FSong.mp3";
        assert_eq!(display_name_from_document_uri(uri), "Song.mp3");
    }

    #[test]
    fn document_uri_根目录文件() {
        let uri =
            "content://com.android.externalstorage.documents/document/primary%3AMusic%2FSong.flac";
        assert_eq!(display_name_from_document_uri(uri), "Song.flac");
    }

    #[test]
    fn 百分号解码处理非法序列与加号() {
        assert_eq!(percent_decode("primary%3AMusic"), "primary:Music");
        assert_eq!(percent_decode("a%2Fb%20c"), "a/b c");
        // 非法/截断的转义序列保留原样，不能吞掉字符
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("abc%zz"), "abc%zz");
    }

    #[test]
    fn 树_uri推导可读名() {
        assert_eq!(
            display_name_from_tree_uri(
                "content://com.android.externalstorage.documents/tree/primary%3AMusic"
            ),
            "Music"
        );
        assert_eq!(
            display_name_from_tree_uri(
                "content://com.android.externalstorage.documents/tree/primary%3ADownload%2FMusic"
            ),
            "Music"
        );
    }
}
