//! 系统字体查询模块
//!
//! 提供获取系统已安装字体列表的功能

use std::collections::HashSet;

#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

/// 获取系统已安装的字体列表
pub fn get_system_fonts() -> Result<Vec<String>, String> {
    #[cfg(target_os = "windows")]
    {
        get_windows_fonts()
    }
    
    #[cfg(target_os = "macos")]
    {
        get_macos_fonts()
    }
    
    #[cfg(target_os = "linux")]
    {
        get_linux_fonts()
    }
}

#[cfg(target_os = "windows")]
fn get_windows_fonts() -> Result<Vec<String>, String> {
    let mut fonts = HashSet::new();
    
    // 读取注册表中的字体信息
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    
    // Windows 字体注册表路径
    let font_key = hklm
        .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts")
        .map_err(|e| format!("Failed to open fonts registry key: {e}"))?;
    
    // 遍历所有字体条目
    for (name, _) in font_key.enum_values().filter_map(Result::ok) {
        // 字体名称格式通常是 "Font Name (TrueType)" 或 "Font Name & Font Name Bold (TrueType)"
        // 提取实际的字体名称
        if let Some(font_name) = extract_font_name(&name) {
            fonts.insert(font_name);
        }
    }
    
    // 转换为排序的 Vec
    let mut font_list: Vec<String> = fonts.into_iter().collect();
    font_list.sort();
    
    Ok(font_list)
}

#[cfg(target_os = "macos")]
fn get_macos_fonts() -> Result<Vec<String>, String> {
    use std::process::Command;
    
    // 使用 system_profiler 获取字体列表
    let output = Command::new("system_profiler")
        .args(["SPFontsDataType", "-json"])
        .output()
        .map_err(|e| format!("Failed to execute system_profiler: {e}"))?;
    
    if !output.status.success() {
        return Err("system_profiler command failed".to_string());
    }
    
    let json_str = String::from_utf8_lossy(&output.stdout);
    
    // 简单解析 JSON（实际项目中应使用 serde_json）
    let mut fonts = HashSet::new();
    
    // 这里需要根据实际的 JSON 结构解析
    // 暂时返回一些常见的 macOS 字体
    fonts.insert("SF Pro".to_string());
    fonts.insert("Helvetica Neue".to_string());
    fonts.insert("Arial".to_string());
    fonts.insert("Times New Roman".to_string());
    fonts.insert("Courier New".to_string());
    
    let mut font_list: Vec<String> = fonts.into_iter().collect();
    font_list.sort();
    
    Ok(font_list)
}

#[cfg(target_os = "linux")]
fn get_linux_fonts() -> Result<Vec<String>, String> {
    use std::process::Command;
    
    // 使用 fc-list 命令获取字体列表
    let output = Command::new("fc-list")
        .args([":", "family"])
        .output()
        .map_err(|e| format!("Failed to execute fc-list: {e}"))?;
    
    if !output.status.success() {
        return Err("fc-list command failed".to_string());
    }
    
    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut fonts = HashSet::new();
    
    // fc-list 输出格式：每行一个字体族名称
    for line in output_str.lines() {
        let font_name = line.trim();
        if !font_name.is_empty() {
            // 有些字体名称包含多个变体，用逗号分隔
            for name in font_name.split(',') {
                fonts.insert(name.trim().to_string());
            }
        }
    }
    
    let mut font_list: Vec<String> = fonts.into_iter().collect();
    font_list.sort();
    
    Ok(font_list)
}

/// 从注册表字体名称中提取实际的字体名称
/// 例如：
/// - "Arial (TrueType)" -> "Arial"
/// - "Microsoft YaHei & Microsoft YaHei UI (TrueType)" -> "Microsoft YaHei"
/// - "Segoe UI Bold (TrueType)" -> "Segoe UI"
#[cfg(target_os = "windows")]
fn extract_font_name(registry_name: &str) -> Option<String> {
    // 移除括号及其内容
    let name = registry_name
        .split('(')
        .next()?
        .trim();
    
    // 如果包含 &，取第一个名称
    let name = name
        .split('&')
        .next()?
        .trim();
    
    // 移除字体样式后缀（Bold, Italic, Light 等）
    let name = remove_font_style_suffix(name);
    
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// 移除字体样式后缀
#[cfg(target_os = "windows")]
fn remove_font_style_suffix(name: &str) -> &str {
    let suffixes = [
        " Bold",
        " Italic",
        " Light",
        " Regular",
        " Medium",
        " Semibold",
        " Black",
        " Thin",
        " ExtraLight",
        " ExtraBold",
        " Heavy",
    ];
    
    for suffix in &suffixes {
        if let Some(pos) = name.rfind(suffix) {
            // 确保后缀在末尾
            if pos + suffix.len() == name.len() {
                return name[..pos].trim();
            }
        }
    }
    
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    #[cfg(target_os = "windows")]
    fn test_extract_font_name() {
        assert_eq!(
            extract_font_name("Arial (TrueType)"),
            Some("Arial".to_string())
        );
        
        assert_eq!(
            extract_font_name("Microsoft YaHei & Microsoft YaHei UI (TrueType)"),
            Some("Microsoft YaHei".to_string())
        );
        
        assert_eq!(
            extract_font_name("Segoe UI Bold (TrueType)"),
            Some("Segoe UI".to_string())
        );
        
        assert_eq!(
            extract_font_name("Consolas (TrueType)"),
            Some("Consolas".to_string())
        );
    }
}
