//! 安全校验工具
//!
//! 提供路径与文件名的统一校验，防止路径穿越、任意文件读写等攻击。
//! 所有检查均为词法检查，可直接用于前端传入的参数。

use std::path::Path;

/// 判断路径是否恰好等于目录前缀，或位于其下（按分隔符边界匹配）
fn starts_with_dir(path: &str, dir: &str, sep: char) -> bool {
    path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with(sep))
}

/// 判断路径（词法检查）是否位于敏感系统目录
///
/// 覆盖 Windows 系统目录、启动项、凭据目录与 Unix 系统目录。
#[must_use]
pub fn is_sensitive_path(path: &str) -> bool {
    if path.is_empty() {
        return true;
    }

    // 统一分隔符后做小写匹配
    let win_style = path.replace('/', "\\").to_lowercase();
    let unix_style = path.replace('\\', "/").to_lowercase();

    let windows_prefixes = [
        "c:\\windows",
        "c:\\program files",
        "c:\\program files (x86)",
        "c:\\programdata",
        "c:\\users\\all users",
    ];
    if windows_prefixes
        .iter()
        .any(|p| starts_with_dir(&win_style, p, '\\'))
    {
        return true;
    }

    // Windows 启动项等敏感位置（AppData\Roaming\Microsoft 下含启动目录与凭据）
    if win_style.contains("\\appdata\\roaming\\microsoft\\") {
        return true;
    }

    // 凭据目录（按路径组件匹配，任意平台）
    if path
        .split(['/', '\\'])
        .any(|c| c.eq_ignore_ascii_case(".ssh") || c.eq_ignore_ascii_case(".gnupg"))
    {
        return true;
    }

    let unix_prefixes = [
        "/etc", "/usr", "/bin", "/sbin", "/var", "/system", "/boot", "/proc", "/sys", "/dev",
        "/lib", "/lib64",
    ];
    unix_prefixes
        .iter()
        .any(|p| starts_with_dir(&unix_style, p, '/'))
}

/// 判断是否为简单文件名：单个路径组件，不含分隔符、`..`、`.`、盘符或根。
///
/// 显式检查 `/`、`\`、`:` 而不依赖 `Path::components()`——后者按宿主平台
/// 解析分隔符，`\` 在 Unix 上是普通字符，会导致跨平台校验结果不一致。
#[must_use]
pub fn is_simple_filename(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 {
        return false;
    }
    if name == "." || name == ".." {
        return false;
    }
    !name.contains(['/', '\\', ':'])
}

/// 判断是否为安全的相对路径：非绝对路径、不含 `..`、`.`、空组件、盘符或根。
///
/// 与 `is_simple_filename` 同理，按两种分隔符切分做跨平台一致的校验。
#[must_use]
pub fn is_safe_relative_path(path: &str) -> bool {
    if path.is_empty() || path.len() > 1024 {
        return false;
    }
    path.split(['/', '\\'])
        .all(|c| !c.is_empty() && c != "." && c != ".." && !c.contains(':'))
}

/// 判断是否为合法的插件 ID：仅字母、数字、下划线、连字符，长度 1-64
#[must_use]
pub fn is_valid_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// 判断路径（canonicalize 后）是否仍位于指定目录内，防止符号链接逃逸。
/// 路径与目录都必须存在。
#[must_use]
pub fn is_within_dir(path: &Path, base: &Path) -> bool {
    match (path.canonicalize(), base.canonicalize()) {
        (Ok(canonical), Ok(canonical_base)) => canonical.starts_with(&canonical_base),
        _ => false,
    }
}

/// 判断路径的扩展名（不区分大小写）是否在白名单内
#[must_use]
pub fn has_allowed_extension(path: &str, allowed: &[&str]) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| allowed.iter().any(|a| e.eq_ignore_ascii_case(a)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensitive_paths() {
        assert!(is_sensitive_path("C:\\Windows\\System32\\cmd.exe"));
        assert!(is_sensitive_path("c:\\program files\\app\\x.txt"));
        assert!(is_sensitive_path("/etc/passwd"));
        assert!(is_sensitive_path("/usr/bin/ls"));
        assert!(is_sensitive_path("/etc"));
        assert!(is_sensitive_path("C:\\Users\\a\\.ssh\\id_rsa"));
        assert!(is_sensitive_path("/home/a/.gnupg/pubring.kbx"));
        assert!(is_sensitive_path(
            "C:\\Users\\a\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\x"
        ));
        assert!(is_sensitive_path(""));

        assert!(!is_sensitive_path("D:\\Music\\song.mp3"));
        assert!(!is_sensitive_path("/home/a/music/song.mp3"));
        assert!(!is_sensitive_path("C:\\Users\\a\\Music"));
        // 边界匹配：不应误伤 library 等相似前缀
        assert!(!is_sensitive_path("/library/books"));
        assert!(!is_sensitive_path("C:\\windows personal\\x"));
    }

    #[test]
    fn test_simple_filename() {
        assert!(is_simple_filename("cover.png"));
        assert!(is_simple_filename("my.plugin"));

        assert!(!is_simple_filename(""));
        assert!(!is_simple_filename(".."));
        assert!(!is_simple_filename("."));
        assert!(!is_simple_filename("../evil"));
        assert!(!is_simple_filename("a/b.png"));
        assert!(!is_simple_filename("a\\b.png"));
        assert!(!is_simple_filename("C:cover.png"));
        assert!(!is_simple_filename("/etc/passwd"));
    }

    #[test]
    fn test_safe_relative_path() {
        assert!(is_safe_relative_path("index.js"));
        assert!(is_safe_relative_path("src/main.js"));

        assert!(!is_safe_relative_path("../escape.js"));
        assert!(!is_safe_relative_path("a/../../escape.js"));
        assert!(!is_safe_relative_path("/etc/passwd"));
        assert!(!is_safe_relative_path("C:\\Windows\\evil.js"));
        assert!(!is_safe_relative_path(""));
    }

    #[test]
    fn test_valid_plugin_id() {
        assert!(is_valid_plugin_id("lyrics-share"));
        assert!(is_valid_plugin_id("My_Plugin_01"));

        assert!(!is_valid_plugin_id(""));
        assert!(!is_valid_plugin_id(".."));
        assert!(!is_valid_plugin_id("..\\..\\Windows"));
        assert!(!is_valid_plugin_id("a/b"));
        assert!(!is_valid_plugin_id("id with space"));
        assert!(!is_valid_plugin_id(&"x".repeat(65)));
    }

    #[test]
    fn test_allowed_extension() {
        assert!(has_allowed_extension("a.lrc", &["lrc", "ass", "srt"]));
        assert!(has_allowed_extension("a.LRC", &["lrc"]));
        assert!(!has_allowed_extension("a.txt", &["lrc"]));
        assert!(!has_allowed_extension("a", &["lrc"]));
    }
}
