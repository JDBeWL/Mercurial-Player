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
}

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
pub struct GeneralConfig {
    pub language: String,
    pub theme: String,
    pub startup_load_last_config: bool,
    pub auto_save_config: bool,
    pub show_audio_info: bool,
    pub lyrics_alignment: String,
    #[serde(default = "default_lyrics_font_family")]
    pub lyrics_font_family: String,
}

/// 音频设置
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AudioConfig {
    pub exclusive_mode: bool,
    #[serde(default = "default_volume")]
    pub volume: f32,
}

const fn default_volume() -> f32 {
    0.5
}

fn default_lyrics_font_family() -> String {
    "Roboto".to_string()
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
            lyrics_alignment: "center".to_string(),
            lyrics_font_family: "Roboto".to_string(),
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
            eprintln!("Failed to create config directory: {e}");
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
            println!("创建默认配置文件: {default_config_path}");
            self.save_config_to_file(&AppConfig::default(), &default_config_path)?;
        }

        if !Path::new(&user_config_path).exists() {
            println!("创建用户配置文件: {user_config_path}");
            self.save_config_to_file(&AppConfig::default(), &user_config_path)?;
        }

        Ok(())
    }

    pub fn load_config(&self) -> Result<AppConfig, String> {
        self.initialize_config_files()?;

        let user_config_path = self.get_user_config_path();
        if Path::new(&user_config_path).exists() {
            if let Ok(config) = self.load_config_from_file(&user_config_path) {
                println!("Loaded user configuration from: {user_config_path}");
                return Ok(config);
            }
        }

        let default_config_path = self.get_default_config_path();
        self.load_config_from_file(&default_config_path).or_else(|_| {
            println!("Creating default configuration");
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
                eprintln!("Failed to remove user config file: {e}");
            }
        }
        Ok(default_config)
    }

    #[must_use]
    pub fn get_config_directory(&self) -> &str {
        &self.config_dir
    }
}
