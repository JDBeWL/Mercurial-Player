//! 安全校验工具：路径与文件名的统一校验，防路径穿越、任意文件读写。
//!
//! 除 [`is_within_dir`] / [`is_within_music_dirs`] 需要 canonicalize 外，其余检查都是词法检查，
//! 可以直接作用在前端传入的参数上。

use std::path::Path;

/// 判断路径是否恰好等于目录前缀，或位于其下（按分隔符边界匹配）
fn starts_with_dir(path: &str, dir: &str, sep: char) -> bool {
    path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with(sep))
}

/// 判断路径（词法检查）是否位于敏感系统目录：Windows 系统目录、启动项、凭据目录与 Unix 系统目录
///
/// 空路径也判为敏感，调用方无需先排除。
#[must_use]
pub fn is_sensitive_path(path: &str) -> bool {
    if path.is_empty() {
        return true;
    }

    // 两种分隔符风格各归一化一遍，任一命中即拒绝
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
/// 显式检查 `/`、`\`、`:` 而不用 `Path::components()`：后者按宿主平台解析分隔符，
/// `\` 在 Unix 上是普通字符，跨平台校验结果会不一致。
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
/// 跨平台分隔符处理的理由与 [`is_simple_filename`] 相同。
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
///
/// 路径与目录都必须存在，任一 canonicalize 失败即判为不在内。
#[must_use]
pub fn is_within_dir(path: &Path, base: &Path) -> bool {
    match (path.canonicalize(), base.canonicalize()) {
        (Ok(canonical), Ok(canonical_base)) => canonical.starts_with(&canonical_base),
        _ => false,
    }
}

/// 目录扫描类命令的白名单门禁：路径必须是已登记的 `music_directories` 之一或其子目录。
///
/// `is_sensitive_path` 只是黑名单，挡不住"传一个用户从没添加过的目录"，而整树枚举加元数据
/// 回传正是被攻破的渲染进程最想要的能力。Android 的 SAF 树 URI 本身就代表用户授权，放行；
/// 目录不存在时 `is_within_dir` 返回 false，因此卸载的外置盘会自然被拒。
#[must_use]
pub fn is_within_music_dirs(path: &str, music_dirs: &[String]) -> bool {
    if crate::android::saf::is_content_uri(path) {
        return true;
    }
    let target = Path::new(path);
    music_dirs
        .iter()
        .any(|dir| is_within_dir(target, Path::new(dir)))
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
    use std::fs;

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
    fn test_is_within_music_dirs() {
        let tag = std::process::id();
        let base = std::env::temp_dir().join(format!("mp_music_base_{tag}"));
        let inside = base.join("rock");
        let outside = std::env::temp_dir().join(format!("mp_music_outside_{tag}"));
        fs::create_dir_all(&inside).unwrap();
        fs::create_dir_all(&outside).unwrap();

        let dirs = vec![base.to_string_lossy().into_owned()];
        assert!(is_within_music_dirs(&base.to_string_lossy(), &dirs));
        assert!(is_within_music_dirs(&inside.to_string_lossy(), &dirs));
        assert!(!is_within_music_dirs(&outside.to_string_lossy(), &dirs));
        // 相似前缀不算在册：/tmp/mp_music_base 不能覆盖 /tmp/mp_music_basedir
        let lookalike = std::env::temp_dir().join(format!("mp_music_basedir_{tag}"));
        fs::create_dir_all(&lookalike).unwrap();
        assert!(!is_within_music_dirs(&lookalike.to_string_lossy(), &dirs));
        // 一个目录都没登记时，任何路径都不可扫描
        assert!(!is_within_music_dirs(&inside.to_string_lossy(), &[]));

        for dir in [&base, &outside, &lookalike] {
            let _ = fs::remove_dir_all(dir);
        }
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
