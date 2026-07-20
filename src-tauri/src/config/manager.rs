//! 配置管理模块
//!
//! 提供应用程序配置的加载、保存和管理功能。

use serde::{Deserialize, Serialize};
use std::path::Path;

/// 应用程序配置数据结构
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
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
        "www.github.com".to_string(),
        "api.github.com".to_string(),
        "tauri.app".to_string(),
        "www.tauri.app".to_string(),
        "vuejs.org".to_string(),
        "www.vuejs.org".to_string(),
        "docs.rs".to_string(),
        "gnu.org".to_string(),
        "www.gnu.org".to_string(),
    ]
}

const fn default_volume() -> f32 {
    0.5
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            music_directories: Vec::new(),
            directory_scan: DirectoryScanConfig::default(),
            title_extraction: TitleExtractionConfig::default(),
            playlist: PlaylistConfig::default(),
            general: GeneralConfig::default(),
            audio: AudioConfig::default(),
            lyrics: LyricsConfig::default(),
            last_session: None,
        }
    }
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
            custom_separators: vec!["-".to_string(), "_".to_string(), ".".to_string(), " ".to_string()],
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
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigManager {
    #[must_use]
    pub fn new() -> Self {
        let config_dir = Self::get_app_config_dir().unwrap_or_else(|_| "./config".to_string());
        if let Err(e) = std::fs::create_dir_all(&config_dir) {
            log::error!("Failed to create config directory: {e}");
        }
        Self { config_dir }
    }

    fn get_app_config_dir() -> Result<String, Box<dyn std::error::Error>> {
        let exe_path = std::env::current_exe()?;
        let exe_dir = exe_path.parent().ok_or("无法获取可执行文件目录")?.to_path_buf();
        let config_path = exe_dir.join("config");
        Ok(config_path.to_string_lossy().to_string())
    }

    fn get_default_config_path(&self) -> String {
        format!("{}/default.json", self.config_dir)
    }

    fn get_user_config_path(&self) -> String {
        format!("{}/user.json", self.config_dir)
    }

    pub fn initialize_config_files(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.config_dir).map_err(|e| format!("创建配置目录失败: {e}"))?;

        let default_config_path = self.get_default_config_path();
        let user_config_path = self.get_user_config_path();

        if !Path::new(&default_config_path).exists() {
            log::info!("创建默认配置文件: {default_config_path}");
            self.save_config_to_file(&AppConfig::default(), &default_config_path)?;
        }

        if !Path::new(&user_config_path).exists() {
            log::info!("创建用户配置文件: {user_config_path}");
            self.save_config_to_file(&AppConfig::default(), &user_config_path)?;
        }

        Ok(())
    }

    pub fn load_config(&self) -> Result<AppConfig, String> {
        self.initialize_config_files()?;

        let user_config_path = self.get_user_config_path();
        if Path::new(&user_config_path).exists() {
            if let Ok(config) = self.load_config_from_file(&user_config_path) {
                log::info!("Loaded user configuration from: {user_config_path}");
                return Ok(config);
            }
        }

        let default_config_path = self.get_default_config_path();
        self.load_config_from_file(&default_config_path).or_else(|_| {
            log::info!("Creating default configuration");
            let default_config = AppConfig::default();
            let _ = self.save_default_config(&default_config);
            Ok(default_config)
        })
    }

    fn load_config_from_file(&self, file_path: &str) -> Result<AppConfig, String> {
        let content = std::fs::read_to_string(file_path).map_err(|e| format!("Failed to read config file: {e}"))?;
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse config file: {e}"))
    }

    pub fn save_config(&self, config: &AppConfig) -> Result<(), String> {
        self.save_config_to_file(config, &self.get_user_config_path())
    }

    pub fn save_default_config(&self, config: &AppConfig) -> Result<(), String> {
        self.save_config_to_file(config, &self.get_default_config_path())
    }

    fn save_config_to_file(&self, config: &AppConfig, file_path: &str) -> Result<(), String> {
        let content = serde_json::to_string_pretty(config).map_err(|e| format!("Failed to serialize config: {e}"))?;
        std::fs::write(file_path, content).map_err(|e| format!("Failed to write config file: {e}"))
    }

    pub fn export_config(&self, config: &AppConfig, export_path: &str) -> Result<(), String> {
        self.save_config_to_file(config, export_path)
    }

    pub fn import_config(&self, import_path: &str) -> Result<AppConfig, String> {
        self.load_config_from_file(import_path)
    }

    pub fn reset_config(&self) -> Result<AppConfig, String> {
        let default_config = AppConfig::default();
        let user_config_path = self.get_user_config_path();
        if Path::new(&user_config_path).exists() {
            if let Err(e) = std::fs::remove_file(&user_config_path) {
                log::error!("Failed to remove user config file: {e}");
            }
        }
        Ok(default_config)
    }

    #[must_use]
    pub fn get_config_directory(&self) -> &str {
        &self.config_dir
    }
}
