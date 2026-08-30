//! 配置管理模块
//!
//! 提供应用程序配置的加载、保存和管理功能。
use crate::error::AppError;

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, RwLock};

/// 应用程序配置数据结构
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub struct AppConfig {
    /// 音乐目录列表
    pub music_directories: Vec<String>,
    /// 子目录扫描配置
    pub directory_scan: DirectoryScanConfig,
    /// 标题提取配置
    pub title_extraction: TitleExtractionConfig,
    /// 播放列表配置
    pub playlist: PlaylistConfig,
    /// 通用设置
    pub general: GeneralConfig,
    /// 音频设置
    pub audio: AudioConfig,
    /// 歌词设置
    #[serde(default)]
    pub lyrics: LyricsConfig,
    /// 上次播放会话 (用于启动恢复)
    #[serde(default)]
    pub last_session: Option<LastSession>,
}

/// 上次播放会话信息
///
/// 启动时通过 L1 (文件存在) + L2 (size+mtime 一致) 校验,
/// 通过则恢复到 position_secs;文件不存在则从播放列表移除并清除本字段
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LastSession {
    /// 曲目文件路径
    pub track_path: String,
    /// 曲目标题 (UI 显示)
    pub track_title: String,
    /// 曲目艺术家 (UI 显示)
    pub track_artist: String,
    /// 曲目时长 (秒, UI 显示)
    pub duration_secs: f32,
    /// 上次播放位置 (秒)
    pub position_secs: f32,
    /// 所在播放列表名 (用于上下首导航), None 表示无对应播放列表
    pub playlist_name: Option<String>,
    /// 在播放列表中的索引 (用于上下首导航)
    pub track_index_in_playlist: Option<usize>,
    /// 文件大小 (字节,L2 校验)
    pub file_size: u64,
    /// 文件最后修改时间 (Unix 秒,L2 校验)
    pub file_mtime: u64,
    /// 本记录保存时间 (Unix 秒,30 天过期)
    pub saved_at: u64,
    /// 播放队列快照 (用于恢复 player.playlist, 不依赖 musicLibrary 缓存)
    /// 保存所有曲元的元数据,恢复时直接构造 Track[]
    #[serde(default)]
    pub playlist_tracks: Vec<TrackSnapshot>,
}

/// 曲目元数据快照 (用于 last_session 恢复播放队列)
///
/// 只保存 UI 显示和导航需要的字段,不保存 coverPath (按需加载)
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackSnapshot {
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<f32>,
    pub bitrate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub bit_depth: Option<u8>,
    pub format: Option<String>,
}

/// last_session 过期时间 (30 天)
pub const LAST_SESSION_MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60;

/// 子目录扫描配置
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryScanConfig {
    pub enable_subdirectory_scan: bool,
    pub max_depth: u32,
    pub ignore_hidden_folders: bool,
    pub folder_blacklist: Vec<String>,
}

/// 标题提取配置
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TitleExtractionConfig {
    pub prefer_metadata: bool,
    pub separator: String,
    pub custom_separators: Vec<String>,
    pub hide_file_extension: bool,
    pub parse_artist_title: bool,
}

/// 播放列表配置
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistConfig {
    pub generate_all_songs_playlist: bool,
    pub folder_based_playlists: bool,
    pub playlist_name_format: String,
}

/// 通用设置
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)] // 配置结构体中多个布尔字段是合理的
pub struct GeneralConfig {
    pub language: String,
    pub theme: String,
    pub startup_load_last_config: bool,
    pub auto_save_config: bool,
    pub show_audio_info: bool,
    /// 是否启用自动更新（默认关闭）
    #[serde(default)]
    pub enable_auto_update: bool,
    /// 可打开的外部链接白名单主机（用于 open_external_url）
    #[serde(default = "default_external_url_allowed_hosts")]
    pub external_url_allowed_hosts: Vec<String>,
    /// 封面缓存大小（单位：MB），默认 1024MB (1GB)
    #[serde(default = "default_cover_cache_size_mb")]
    pub cover_cache_size_mb: u64,
    /// 封面缓存路径，默认为空表示使用系统临时目录
    #[serde(default)]
    pub cover_cache_path: Option<String>,
}

fn default_cover_cache_size_mb() -> u64 {
    1024 // 1GB
}

/// 音频设置
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AudioConfig {
    pub exclusive_mode: bool,
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// 是否启用淡入淡出(切歌平滑过渡 + pause/resume 消除爆音)
    #[serde(default = "default_true")]
    pub fade_enabled: bool,
}

/// 歌词设置
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LyricsConfig {
    #[serde(default)]
    pub enable_online_fetch: bool,
    #[serde(default = "default_true")]
    pub auto_save_online_lyrics: bool,
    #[serde(default = "default_true")]
    pub prefer_translation: bool,
    #[serde(default = "default_online_source")]
    pub online_source: String,
    #[serde(default = "default_lyrics_alignment")]
    pub lyrics_alignment: String,
    #[serde(default = "default_lyrics_font_family")]
    pub lyrics_font_family: String,
    #[serde(default = "default_lyrics_style")]
    pub lyrics_style: String,
}

const fn default_true() -> bool {
    true
}

fn default_online_source() -> String {
    "netease".to_string()
}

fn default_lyrics_alignment() -> String {
    "center".to_string()
}

fn default_lyrics_font_family() -> String {
    "Roboto".to_string()
}

fn default_lyrics_style() -> String {
    "modern".to_string()
}

fn default_external_url_allowed_hosts() -> Vec<String> {
    vec![
        "github.com".to_string(),
        "github.io".to_string(),
        "tauri.app".to_string(),
        "vuejs.org".to_string(),
        "intlify.dev".to_string(),
        "docs.rs".to_string(),
        "gnu.org".to_string(),
        "vitejs.dev".to_string(),
        "typescriptlang.org".to_string(),
        "vitest.dev".to_string(),
    ]
}

const fn default_volume() -> f32 {
    0.5
}

impl Default for DirectoryScanConfig {
    fn default() -> Self {
        Self {
            enable_subdirectory_scan: true,
            max_depth: 3,
            ignore_hidden_folders: true,
            folder_blacklist: vec![
                ".git".to_string(),
                "node_modules".to_string(),
                "temp".to_string(),
                "tmp".to_string(),
            ],
        }
    }
}

impl Default for TitleExtractionConfig {
    fn default() -> Self {
        Self {
            prefer_metadata: true,
            separator: "-".to_string(),
            custom_separators: vec![
                "-".to_string(),
                "_".to_string(),
                ".".to_string(),
                " ".to_string(),
            ],
            hide_file_extension: true,
            parse_artist_title: true,
        }
    }
}

impl Default for PlaylistConfig {
    fn default() -> Self {
        Self {
            generate_all_songs_playlist: true,
            folder_based_playlists: true,
            playlist_name_format: "{folderName}".to_string(),
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            language: "zh".to_string(),
            theme: "auto".to_string(),
            startup_load_last_config: true,
            auto_save_config: true,
            show_audio_info: true,
            enable_auto_update: false,
            external_url_allowed_hosts: default_external_url_allowed_hosts(),
            cover_cache_size_mb: default_cover_cache_size_mb(),
            cover_cache_path: None,
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            exclusive_mode: false,
            volume: default_volume(),
            fade_enabled: true,
        }
    }
}

impl Default for LyricsConfig {
    fn default() -> Self {
        Self {
            enable_online_fetch: false,
            auto_save_online_lyrics: true,
            prefer_translation: true,
            online_source: "netease".to_string(),
            lyrics_alignment: "center".to_string(),
            lyrics_font_family: "Roboto".to_string(),
            lyrics_style: "modern".to_string(),
        }
    }
}

/// 配置管理器
pub struct ConfigManager {
    config_dir: String,
    /// 进程内配置缓存（Arc 共享，读命中时免磁盘 IO + JSON 解析 + 目录扫描）。
    /// save_config 写穿更新，reset_config 清空；ConfigManager 在 AppState 中单例存在
    cache: RwLock<Option<Arc<AppConfig>>>,
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigManager {
    #[must_use]
    pub fn new() -> Self {
        let config_dir = Self::get_app_config_dir().unwrap_or_else(|_| "./data".to_string());
        if let Err(e) = std::fs::create_dir_all(&config_dir) {
            log::error!("Failed to create config directory: {e}");
        }
        Self {
            config_dir,
            cache: RwLock::new(None),
        }
    }

    /// 更新进程内缓存
    fn store_cache(&self, config: &AppConfig) {
        if let Ok(mut guard) = self.cache.write() {
            *guard = Some(Arc::new(config.clone()));
        }
    }

    /// 读取缓存命中时的克隆
    fn cached_config(&self) -> Option<AppConfig> {
        let guard = self.cache.read().ok()?;
        guard.as_deref().cloned()
    }

    fn get_app_config_dir() -> Result<String, Box<dyn std::error::Error>> {
        let exe_path = std::env::current_exe()?;
        let exe_dir = exe_path
            .parent()
            .ok_or("无法获取可执行文件目录")?
            .to_path_buf();
        let data_path = exe_dir.join("data");
        Ok(data_path.to_string_lossy().to_string())
    }

    /// 当前配置文件路径(data/config.json,唯一权威配置)
    fn get_config_path(&self) -> String {
        format!("{}/config.json", self.config_dir)
    }

    /// 旧版配置目录(<exe>/config,含 default.json/user.json,已废弃)
    fn get_legacy_config_dir() -> Option<String> {
        let exe_path = std::env::current_exe().ok()?;
        let dir = exe_path.parent()?.join("config");
        Some(dir.to_string_lossy().to_string())
    }

    /// 从文件读取配置,兼容两种历史格式:
    /// - 裸 AppConfig JSON(当前格式)
    /// - 旧版前端 plugin-store 包装格式 {"appConfig": {...}}
    ///
    /// 必须显式识别包装格式:serde 默认忽略未知字段,直接反序列化
    /// 包装文件会得到全默认值,导致用户配置被静默清空。
    fn read_config_file(file_path: &str) -> Option<AppConfig> {
        let text = std::fs::read_to_string(file_path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        let target = value.get("appConfig").unwrap_or(&value);
        serde_json::from_value::<AppConfig>(target.clone()).ok()
    }

    /// 判断文件是否为旧版 plugin-store 包装格式(顶层含 "appConfig" 键)
    fn is_plugin_store_wrapped(file_path: &str) -> bool {
        std::fs::read_to_string(file_path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .is_some_and(|value| value.get("appConfig").is_some())
    }

    /// 删除旧版 config/ 目录(default.json/user.json),目录非空时保留
    fn remove_legacy_config_dir(dir: Option<String>) {
        let Some(dir) = dir else { return };
        let _ = std::fs::remove_file(format!("{dir}/default.json"));
        let _ = std::fs::remove_file(format!("{dir}/user.json"));
        if std::fs::remove_dir(&dir).is_ok() {
            log::info!("已移除旧版配置目录: {dir}");
        }
    }

    /// 旧版布局一次性迁移(幂等):
    /// 1. data/config.json 有效(裸格式):直接复用;若是旧包装格式则重写为裸格式
    /// 2. 无有效配置:回退旧 <exe>/config/user.json(后端旧权威文件,含 lastSession)
    /// 3. 均无:清理残留旧目录,后续走 AppConfig::default()
    ///
    /// 只有新文件成功落盘后才删除旧目录,避免迁移中断丢失配置。
    fn migrate_legacy_config(&self) {
        let config_path = self.get_config_path();
        let legacy_dir = Self::get_legacy_config_dir();

        if let Some(config) = Self::read_config_file(&config_path) {
            if Self::is_plugin_store_wrapped(&config_path) {
                log::info!("检测到旧 plugin-store 包装格式,重写为裸格式: {config_path}");
                let _ = Self::save_config_to_file(&config, &config_path);
            }
            Self::remove_legacy_config_dir(legacy_dir);
            return;
        }

        if let Some(dir) = &legacy_dir {
            let legacy_user = format!("{dir}/user.json");
            if let Some(config) = Self::read_config_file(&legacy_user) {
                log::info!("迁移旧配置文件: {legacy_user} -> {config_path}");
                if Self::save_config_to_file(&config, &config_path).is_ok() {
                    Self::remove_legacy_config_dir(legacy_dir);
                }
                return;
            }
        }

        Self::remove_legacy_config_dir(legacy_dir);
    }

    /// 确保 data 目录存在、清理 .tmp 残留并执行旧版布局迁移
    pub fn initialize_config_files(&self) -> Result<(), AppError> {
        std::fs::create_dir_all(&self.config_dir)
            .map_err(|e| AppError::Config(format!("创建配置目录失败: {e}")))?;

        // 清理上次崩溃/断电可能残留的 .tmp 文件，避免需要手动清理
        self.cleanup_temp_files();

        self.migrate_legacy_config();

        Ok(())
    }

    /// 清理 config 目录下残留的 .tmp 文件
    ///
    /// save_config_to_file 使用「先写 .tmp 再 rename」的原子写入模式,
    /// 如果进程在 write 后、rename 前崩溃/断电/被 kill,
    /// .tmp 文件会残留。此方法在启动时扫描并清除这些残留,
    /// 避免用户需要手动清理。
    fn cleanup_temp_files(&self) {
        let Ok(entries) = std::fs::read_dir(&self.config_dir) else {
            return;
        };
        let mut cleaned = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "tmp") {
                if std::fs::remove_file(&path).is_ok() {
                    cleaned += 1;
                    log::debug!("清理残留临时文件: {}", path.display());
                }
            }
        }
        if cleaned > 0 {
            log::info!("启动时清理了 {cleaned} 个残留临时文件");
        }
    }

    /// 合并默认外链白名单:确保新增的域名在旧配置文件中也能生效
    fn with_default_allowed_hosts(mut config: AppConfig) -> AppConfig {
        let defaults = default_external_url_allowed_hosts();
        for d in defaults {
            if !config
                .general
                .external_url_allowed_hosts
                .iter()
                .any(|h| h.eq_ignore_ascii_case(&d))
            {
                config.general.external_url_allowed_hosts.push(d);
            }
        }
        config
    }

    pub fn load_config(&self) -> Result<AppConfig, AppError> {
        // 缓存命中直接返回，避免每个 command 都重复 读盘 + 解析 + read_dir 扫描
        if let Some(cached) = self.cached_config() {
            return Ok(cached);
        }

        self.initialize_config_files()?;

        let config_path = self.get_config_path();
        if let Some(config) = Self::read_config_file(&config_path) {
            log::info!("Loaded user configuration from: {config_path}");
            let config = Self::with_default_allowed_hosts(config);
            self.store_cache(&config);
            return Ok(config);
        }

        log::info!("No valid configuration file, using compiled-in defaults");
        let default_config = AppConfig::default();
        self.store_cache(&default_config);
        Ok(default_config)
    }

    fn load_config_from_file(file_path: &str) -> Result<AppConfig, AppError> {
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| AppError::Config(format!("Failed to read config file: {e}")))?;
        serde_json::from_str(&content)
            .map_err(|e| AppError::Config(format!("Failed to parse config file: {e}")))
    }

    pub fn save_config(&self, config: &AppConfig) -> Result<(), AppError> {
        Self::save_config_to_file(config, &self.get_config_path())?;
        // 写穿：保存成功后同步更新进程内缓存
        self.store_cache(config);
        Ok(())
    }

    fn save_config_to_file(config: &AppConfig, file_path: &str) -> Result<(), AppError> {
        use std::io::Write;
        let content = serde_json::to_string_pretty(config)
            .map_err(|e| AppError::Config(format!("Failed to serialize config: {e}")))?;
        // 原子写入:先写临时文件,sync_all 确保数据落盘,再 rename 替换。
        // 避免写入过程中崩溃/断电导致配置文件损坏;
        // sync_all 确保 rename 前数据已落盘,避免 rename 后断电导致内容丢失。
        // 即使进程在此期间崩溃,.tmp 残留也会在下次启动时被 cleanup_temp_files 清理。
        let tmp_path = format!("{file_path}.tmp");
        {
            let mut file = std::fs::File::create(&tmp_path)
                .map_err(|e| AppError::Config(format!("Failed to create temp config file: {e}")))?;
            file.write_all(content.as_bytes())
                .map_err(|e| AppError::Config(format!("Failed to write temp config file: {e}")))?;
            file.sync_all()
                .map_err(|e| AppError::Config(format!("Failed to sync temp config file: {e}")))?;
        }
        std::fs::rename(&tmp_path, file_path).map_err(|e| {
            // rename 失败时尝试清理临时文件,避免残留
            let _ = std::fs::remove_file(&tmp_path);
            AppError::Config(format!("Failed to rename config file: {e}"))
        })?;
        Ok(())
    }

    pub fn export_config(&self, config: &AppConfig, export_path: &str) -> Result<(), AppError> {
        Self::save_config_to_file(config, export_path)
    }

    pub fn import_config(&self, import_path: &str) -> Result<AppConfig, AppError> {
        let config = Self::load_config_from_file(import_path)?;
        // 合并默认外链白名单，防止导入的配置移除安全域名限制
        Ok(Self::with_default_allowed_hosts(config))
    }

    pub fn reset_config(&self) -> Result<AppConfig, AppError> {
        let default_config = AppConfig::default();
        let config_path = self.get_config_path();
        if Path::new(&config_path).exists() {
            if let Err(e) = std::fs::remove_file(&config_path) {
                log::error!("Failed to remove config file: {e}");
            }
        }
        // config.json 已删除，缓存作废，下次 load_config 重新初始化
        if let Ok(mut guard) = self.cache.write() {
            *guard = None;
        }
        Ok(default_config)
    }

    #[must_use]
    pub fn get_config_directory(&self) -> &str {
        &self.config_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn test_app_config_default_values() {
        let config = AppConfig::default();
        assert!(config.music_directories.is_empty());
        assert!(config.directory_scan.enable_subdirectory_scan);
        assert_eq!(config.directory_scan.max_depth, 3);
        assert_eq!(config.general.language, "zh");
        assert_eq!(config.general.theme, "auto");
        assert!(!config.audio.exclusive_mode);
        assert!(approx_eq(config.audio.volume, 0.5));
        assert!(config.audio.fade_enabled);
        assert_eq!(config.lyrics.online_source, "netease");
        assert_eq!(config.lyrics.lyrics_alignment, "center");
        assert!(config.last_session.is_none());
    }

    #[test]
    fn test_app_config_serde_roundtrip() {
        let mut config = AppConfig::default();
        config.music_directories.push("/test/music".to_string());
        config.audio.volume = 0.7;
        config.general.language = "en".to_string();
        config.last_session = Some(LastSession {
            track_path: "/test/track.mp3".to_string(),
            track_title: "Test".to_string(),
            track_artist: "Artist".to_string(),
            duration_secs: 180.0,
            position_secs: 60.0,
            playlist_name: Some("My List".to_string()),
            track_index_in_playlist: Some(3),
            file_size: 1024,
            file_mtime: 1_000_000,
            saved_at: 2_000_000,
            playlist_tracks: vec![],
        });

        let json = serde_json::to_string(&config).expect("serialize failed");
        let deserialized: AppConfig = serde_json::from_str(&json).expect("deserialize failed");
        let json2 = serde_json::to_string(&deserialized).expect("re-serialize failed");
        assert_eq!(json, json2, "round-trip JSON mismatch");

        // 关键字段恢复后值正确
        assert_eq!(deserialized.music_directories, config.music_directories);
        assert!(approx_eq(deserialized.audio.volume, 0.7));
        assert_eq!(deserialized.general.language, "en");
        assert!(deserialized.last_session.is_some());
    }

    #[test]
    fn test_app_config_camel_case_serialization() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).expect("serialize failed");
        // camelCase 字段应出现在 JSON 中
        assert!(
            json.contains("\"musicDirectories\""),
            "missing camelCase field musicDirectories"
        );
        assert!(json.contains("\"directoryScan\""));
        assert!(json.contains("\"titleExtraction\""));
        assert!(json.contains("\"exclusiveMode\""));
        assert!(json.contains("\"fadeEnabled\""));
        assert!(json.contains("\"onlineSource\""));
        assert!(json.contains("\"lyricsAlignment\""));
        assert!(json.contains("\"lastSession\""));
        // snake_case 不应出现
        assert!(!json.contains("music_directories"), "snake_case leaked");
        assert!(!json.contains("exclusive_mode"));
        assert!(!json.contains("fade_enabled"));
    }

    #[test]
    fn test_directory_scan_config_default() {
        let config = DirectoryScanConfig::default();
        assert!(config.enable_subdirectory_scan);
        assert_eq!(config.max_depth, 3);
        assert!(config.ignore_hidden_folders);
        assert_eq!(config.folder_blacklist.len(), 4);
        assert!(config.folder_blacklist.contains(&".git".to_string()));
    }

    #[test]
    fn test_title_extraction_config_default() {
        let config = TitleExtractionConfig::default();
        assert!(config.prefer_metadata);
        assert_eq!(config.separator, "-");
        assert_eq!(config.custom_separators.len(), 4);
        assert!(config.hide_file_extension);
        assert!(config.parse_artist_title);
    }

    #[test]
    fn test_playlist_config_default() {
        let config = PlaylistConfig::default();
        assert!(config.generate_all_songs_playlist);
        assert!(config.folder_based_playlists);
        assert_eq!(config.playlist_name_format, "{folderName}");
    }

    #[test]
    fn test_audio_config_default() {
        let config = AudioConfig::default();
        assert!(!config.exclusive_mode);
        assert!(approx_eq(config.volume, 0.5));
        assert!(config.fade_enabled);
    }

    #[test]
    fn test_lyrics_config_default() {
        let config = LyricsConfig::default();
        assert!(!config.enable_online_fetch);
        assert!(config.auto_save_online_lyrics);
        assert!(config.prefer_translation);
        assert_eq!(config.online_source, "netease");
        assert_eq!(config.lyrics_alignment, "center");
        assert_eq!(config.lyrics_font_family, "Roboto");
        assert_eq!(config.lyrics_style, "modern");
    }

    #[test]
    fn test_general_config_default() {
        let config = GeneralConfig::default();
        assert_eq!(config.language, "zh");
        assert_eq!(config.theme, "auto");
        assert!(config.startup_load_last_config);
        assert!(config.auto_save_config);
        assert!(config.show_audio_info);
        assert!(!config.enable_auto_update);
        assert!(!config.external_url_allowed_hosts.is_empty());
        assert_eq!(config.cover_cache_size_mb, 1024);
        assert!(config.cover_cache_path.is_none());
    }

    #[test]
    fn test_last_session_max_age_constant() {
        assert_eq!(LAST_SESSION_MAX_AGE_SECS, 30 * 24 * 60 * 60);
        assert_eq!(LAST_SESSION_MAX_AGE_SECS, 2_592_000);
    }

    #[test]
    fn test_last_session_serde_roundtrip() {
        let session = LastSession {
            track_path: "/music/song.mp3".to_string(),
            track_title: "Song".to_string(),
            track_artist: "Artist".to_string(),
            duration_secs: 240.0,
            position_secs: 120.5,
            playlist_name: None,
            track_index_in_playlist: None,
            file_size: 5_000_000,
            file_mtime: 1_700_000_000,
            saved_at: 1_700_000_100,
            playlist_tracks: vec![TrackSnapshot {
                path: "/music/song.mp3".to_string(),
                title: Some("Song".to_string()),
                artist: None,
                album: None,
                duration: Some(240.0),
                bitrate: Some(320),
                sample_rate: Some(44100),
                channels: Some(2),
                bit_depth: None,
                format: Some("mp3".to_string()),
            }],
        };

        let json = serde_json::to_string(&session).expect("serialize failed");
        let deserialized: LastSession = serde_json::from_str(&json).expect("deserialize failed");
        let json2 = serde_json::to_string(&deserialized).expect("re-serialize failed");
        assert_eq!(json, json2);
        assert_eq!(deserialized.track_path, session.track_path);
        assert!(approx_eq(deserialized.position_secs, session.position_secs));
        assert_eq!(deserialized.playlist_tracks.len(), 1);
    }

    #[test]
    fn test_track_snapshot_default_all_none_or_empty() {
        let snapshot = TrackSnapshot::default();
        assert!(snapshot.path.is_empty());
        assert!(snapshot.title.is_none());
        assert!(snapshot.artist.is_none());
        assert!(snapshot.album.is_none());
        assert!(snapshot.duration.is_none());
        assert!(snapshot.bitrate.is_none());
        assert!(snapshot.sample_rate.is_none());
        assert!(snapshot.channels.is_none());
        assert!(snapshot.bit_depth.is_none());
        assert!(snapshot.format.is_none());
    }
}
