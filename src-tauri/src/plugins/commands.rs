//! 插件系统 Tauri 命令
use std::path::{Path, PathBuf};

use crate::error::AppError;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

use super::manager::{self, PluginManifest};
use crate::security::{has_allowed_extension, is_simple_filename};
use tauri::command;

/// 列出所有插件
#[command]
pub fn list_plugins() -> Result<Vec<String>, AppError> {
    manager::list_plugin_dirs()
}

/// 允许的截图文件扩展名
const SCREENSHOT_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];
/// 单张截图大小上限(插件可经 file.saveImage 无限制写入,防止刷爆磁盘)
const MAX_SCREENSHOT_BYTES: usize = 32 * 1024 * 1024;
/// base64 编码后的对应上限（4 字符表示 3 字节，再留一行换行的余量）
const MAX_ENCODED_SCREENSHOT_BYTES: usize = MAX_SCREENSHOT_BYTES.div_ceil(3) * 4 + 4;
/// 截图目录总配额(达到后拒绝新增,提示用户清理)
const SCREENSHOTS_DIR_QUOTA_BYTES: u64 = 512 * 1024 * 1024;

/// 冲突自动改名:同名文件已存在时追加 `-1`/`-2`… 序号,避免覆盖用户已有截图
fn unique_screenshot_path(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("screenshot");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("png");
    for i in 1..10_000 {
        let alt = dir.join(format!("{stem}-{i}.{ext}"));
        if !alt.exists() {
            return alt;
        }
    }
    // 极端情况(同名前缀占满 9999 个):附加时间戳保证唯一
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    dir.join(format!("{stem}-{nanos}.{ext}"))
}

/// 统计目录内现有文件总大小(截图层级只有一层,直接 read_dir 即可)
fn screenshots_dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// 解码插件传来的截图字节。
///
/// 用 base64 而不是 raw IPC body：Tauri 在 Android 上不支持 `InvokeBody::Raw`
/// （`tauri::ipc::InvokeBody` 文档明写，枚举永远是 Json），而 base64 仍远好于数字数组
/// ——后者每个字节要 ~4 个 JSON 字符，2MB 图片会被放大到 ~8MB，base64 是 ~2.7MB。
fn decode_screenshot_data(data_b64: &str) -> Result<Vec<u8>, AppError> {
    // 先按编码长度拦一道，免得为超大输入白跑一次解码
    if data_b64.len() > MAX_ENCODED_SCREENSHOT_BYTES {
        return Err(AppError::Plugin(format!(
            "截图数据过大 (编码后 {} 字符, 上限 {MAX_SCREENSHOT_BYTES} 字节)",
            data_b64.len()
        )));
    }
    BASE64_STANDARD
        .decode(data_b64)
        .map_err(|e| AppError::Plugin(format!("截图数据不是合法 base64: {e}")))
}

/// 保存截图到程序目录下的 screenshots 文件夹
#[command]
pub fn save_screenshot(filename: &str, data_b64: String) -> Result<String, AppError> {
    // 校验文件名：必须为简单文件名且为图片扩展名，防止路径穿越任意写
    if !is_simple_filename(filename) || !has_allowed_extension(filename, &SCREENSHOT_EXTENSIONS) {
        return Err(AppError::Plugin(
            "非法的截图文件名（仅允许 png/jpg/jpeg/webp）".to_string(),
        ));
    }

    let data = decode_screenshot_data(&data_b64)?;

    // 单张大小上限
    if data.len() > MAX_SCREENSHOT_BYTES {
        return Err(AppError::Plugin(format!(
            "截图数据过大 ({} 字节, 上限 {} 字节)",
            data.len(),
            MAX_SCREENSHOT_BYTES
        )));
    }

    let exe_path = std::env::current_exe().map_err(|e| format!("无法获取可执行文件路径: {e}"))?;
    let exe_dir = exe_path.parent().ok_or("无法获取可执行文件目录")?;

    let screenshots_dir = exe_dir.join("screenshots");

    // 创建目录（如果不存在）
    if !screenshots_dir.exists() {
        std::fs::create_dir_all(&screenshots_dir).map_err(|e| format!("无法创建截图目录: {e}"))?;
    }

    // 目录配额
    let existing = screenshots_dir_size(&screenshots_dir);
    if existing.saturating_add(data.len() as u64) > SCREENSHOTS_DIR_QUOTA_BYTES {
        return Err(AppError::Plugin(format!(
            "截图目录已满 ({existing} 字节, 配额 {SCREENSHOTS_DIR_QUOTA_BYTES} 字节)，请清理后重试"
        )));
    }

    // 冲突自动改名,不覆盖已有截图
    let file_path = unique_screenshot_path(&screenshots_dir, filename);

    std::fs::write(&file_path, &data).map_err(|e| format!("无法保存截图: {e}"))?;

    Ok(file_path.to_string_lossy().to_string())
}

/// 读取插件清单
#[command]
pub fn read_plugin_manifest(path: &str) -> Result<PluginManifest, AppError> {
    manager::read_manifest(path)
}

/// 读取插件主文件
#[command]
pub fn read_plugin_main(path: &str, main: &str) -> Result<String, AppError> {
    let main_file = if main.is_empty() { "index.js" } else { main };
    manager::read_main_file(path, main_file)
}

/// 卸载插件
#[command]
pub fn uninstall_plugin(plugin_id: &str) -> Result<(), AppError> {
    manager::uninstall_plugin(plugin_id)
}

/// 在文件管理器中打开插件目录
#[command]
pub fn open_plugins_directory() -> Result<(), AppError> {
    let plugins_dir = manager::get_plugins_dir()?;

    tauri_plugin_opener::reveal_item_in_dir(&plugins_dir)
        .map_err(|e| format!("无法打开目录: {e}"))?;

    Ok(())
}

/// 打开截图目录
#[command]
pub fn open_screenshots_directory() -> Result<(), AppError> {
    let exe_path = std::env::current_exe().map_err(|e| format!("无法获取可执行文件路径: {e}"))?;
    let exe_dir = exe_path.parent().ok_or("无法获取可执行文件目录")?;

    let screenshots_dir = exe_dir.join("screenshots");

    // 创建目录（如果不存在）
    if !screenshots_dir.exists() {
        std::fs::create_dir_all(&screenshots_dir).map_err(|e| format!("无法创建截图目录: {e}"))?;
    }

    tauri_plugin_opener::reveal_item_in_dir(&screenshots_dir)
        .map_err(|e| format!("无法打开目录: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_screenshot_data_round_trips() {
        let raw: Vec<u8> = (0..512).map(|i| (i % 256) as u8).collect();
        let encoded = BASE64_STANDARD.encode(&raw);
        assert_eq!(decode_screenshot_data(&encoded).unwrap(), raw);
    }

    #[test]
    fn decode_screenshot_data_rejects_garbage() {
        assert!(matches!(
            decode_screenshot_data("这不是 base64!!"),
            Err(AppError::Plugin(_))
        ));
    }

    #[test]
    fn decode_screenshot_data_rejects_oversized_input() {
        let huge = "A".repeat(MAX_ENCODED_SCREENSHOT_BYTES + 1);
        assert!(matches!(
            decode_screenshot_data(&huge),
            Err(AppError::Plugin(_))
        ));
    }
}
