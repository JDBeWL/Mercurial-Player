//! 配置管理：`AppConfig` 定义与 config.json 的加载、保存与旧版布局迁移。
use crate::error::AppError;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

/// 落盘格式与前端负载的字段全集。
///
/// serde 忽略未知字段，后端没声明的键会在下一次整包保存时静默丢失（表现为"设置每次都被重置"），
/// 故新增设置项必须在这里补字段并带 `#[serde(default)]`，旧配置文件才能继续解析。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub struct AppConfig {
    pub music_directories: Vec<String>,
    pub directory_scan: DirectoryScanConfig,
    pub title_extraction: TitleExtractionConfig,
    pub playlist: PlaylistConfig,
    pub general: GeneralConfig,
    pub audio: AudioConfig,
    #[serde(default)]
    pub lyrics: LyricsConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub visualizer: VisualizerConfig,
    #[serde(default)]
    pub last_session: Option<LastSession>,
}

/// UI 设置:只持久化非临时状态(如迷你模式),面板开关是临时态、不落盘
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UiConfig {
    #[serde(default)]
    pub mini_mode: bool,
    /// 界面字号倍率（1.0 = 设计稿原始大小），仅 Android 生效，机制见
    /// [`crate::android::font_scale`]；必须留字段的理由见 [`AppConfig`]
    #[serde(default = "default_font_scale")]
    pub font_scale: f32,
}

impl Default for UiConfig {
    /// 不能 derive：`f32` 的默认值是 0，界面字号会变成"0 倍"。
    fn default() -> Self {
        Self {
            mini_mode: false,
            font_scale: default_font_scale(),
        }
    }
}

/// 可视化(频谱)设置
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VisualizerConfig {
    #[serde(default = "default_target_fps")]
    pub target_fps: u32,
    /// 是否把目标帧率 cap 到屏幕刷新率(字段名是历史遗留的"垂直同步",仅作持久化键使用)
    #[serde(default)]
    pub enable_vertical_sync: bool,
    /// 最近一次检测到的屏幕刷新率(仅作显示/限制用参考值)
    #[serde(default)]
    pub detected_refresh_rate: Option<u32>,
}

fn default_target_fps() -> u32 {
    60
}

const fn default_font_scale() -> f32 {
    1.0
}

impl Default for VisualizerConfig {
    fn default() -> Self {
        Self {
            target_fps: default_target_fps(),
            enable_vertical_sync: false,
            detected_refresh_rate: None,
        }
    }
}

/// 上次播放会话信息
///
/// 启动时通过 L1(文件存在) + L2(size+mtime 一致) 校验，通过则恢复到 position_secs；
/// 不通过则清除本字段，是否从播放列表移除由前端决定
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LastSession {
    pub track_path: String,
    pub track_title: String,
    pub track_artist: String,
    /// 曲目时长 (秒, 仅供 UI 显示)
    pub duration_secs: f32,
    /// 上次播放位置 (秒)
    pub position_secs: f32,
    /// 所在播放列表名 (用于上下首导航), None 表示无对应播放列表
    pub playlist_name: Option<String>,
    /// 在播放列表中的索引 (与 `playlist_name` 一起用于上下首导航)
    pub track_index_in_playlist: Option<usize>,
    /// 文件大小 (字节,L2 校验)
    pub file_size: u64,
    /// 文件最后修改时间 (Unix 秒,L2 校验)
    pub file_mtime: u64,
    /// 本记录保存时间 (Unix 秒,[`LAST_SESSION_MAX_AGE_SECS`] 后过期)
    pub saved_at: u64,
    /// 播放队列快照:恢复 player.playlist 用,存全量元数据以直接构造 Track[],不依赖 musicLibrary 缓存
    #[serde(default)]
    pub playlist_tracks: Vec<TrackSnapshot>,
}

/// 曲目元数据快照：只存 UI 显示和导航需要的字段，不存 coverPath (按需加载)
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryScanConfig {
    pub enable_subdirectory_scan: bool,
    pub max_depth: u32,
    pub ignore_hidden_folders: bool,
    pub folder_blacklist: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TitleExtractionConfig {
    pub prefer_metadata: bool,
    pub separator: String,
    pub custom_separators: Vec<String>,
    pub hide_file_extension: bool,
    pub parse_artist_title: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistConfig {
    pub generate_all_songs_playlist: bool,
    pub folder_based_playlists: bool,
    pub playlist_name_format: String,
    /// 曲目排序: "asc" | "desc" (前端 SortOrder)
    #[serde(default = "default_sort_order")]
    pub sort_order: String,
}

fn default_sort_order() -> String {
    "asc".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)] // 配置结构体中多个布尔字段是合理的
pub struct GeneralConfig {
    pub language: String,
    pub theme: String,
    pub startup_load_last_config: bool,
    pub auto_save_config: bool,
    pub show_audio_info: bool,
    #[serde(default = "default_true")]
    pub show_queue_info: bool,
    /// 沉浸式封面取色风格：album = 整张封面的代表色；fusion = 取封面右缘条带
    #[serde(default = "default_immersive_color_scheme")]
    pub immersive_color_scheme: String,
    /// 沉浸式模式下是否根据封面主色亮度自动切换深/浅主题
    #[serde(default = "default_true")]
    pub immersive_auto_theme: bool,
    #[serde(default)]
    pub enable_auto_update: bool,
    /// 可打开的外部链接白名单主机（用于 open_external_url）
    #[serde(default = "default_external_url_allowed_hosts")]
    pub external_url_allowed_hosts: Vec<String>,
    /// 封面缓存大小上限（单位：MB）
    #[serde(default = "default_cover_cache_size_mb")]
    pub cover_cache_size_mb: u64,
    /// 封面缓存路径，默认为空表示使用系统临时目录
    #[serde(default)]
    pub cover_cache_path: Option<String>,
}

fn default_immersive_color_scheme() -> String {
    "album".to_string()
}

fn default_cover_cache_size_mb() -> u64 {
    1024
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AudioConfig {
    pub exclusive_mode: bool,
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// 是否启用淡入淡出(切歌平滑过渡 + pause/resume 消除爆音)
    #[serde(default = "default_true")]
    pub fade_enabled: bool,
    /// 用户手动选择的输出设备标识,取自 cpal `DeviceTrait::id()`(各平台原生稳定标识),落盘为 `"host:id"`。
    /// 标识机器绑定,失效时启动静默回退系统默认设备;只有设置页主动选择才写入,自动回退不覆盖用户选择。
    #[serde(default)]
    pub preferred_device_id: Option<String>,
    /// Android:USB DAC 独占(位完美)输出,绕过系统混音/重采样按曲目原生采样率直连 USB 声卡。
    /// 只有系统里确实存在 USB 音频输出设备时才可能生效,拔出后自动回落共享模式并暂停。
    #[serde(default)]
    pub usb_dac_exclusive: bool,
}

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
    /// 译文字体(空字符串 = 跟随原文歌词字体)
    #[serde(default)]
    pub translation_font_family: String,
    #[serde(default = "default_true")]
    pub show_no_lyrics_hint: bool,
    #[serde(default = "default_true")]
    pub show_fetch_lyrics_button: bool,
    #[serde(default = "default_lyrics_style")]
    pub lyrics_style: String,
    /// 主歌词面板字号倍率，1.0 = 样式表原始大小；桌面歌词窗口用 `desktop_lyrics.font_size`
    #[serde(default = "default_font_scale")]
    pub font_scale: f32,
    /// 点击"获取歌词"时是否自动择优(true = 无感自动写盘,false = 打开候选挑选弹窗)
    #[serde(default = "default_true")]
    pub auto_select_best_lyrics: bool,
    /// 启用的歌词来源 id 有序列表(= 顺延优先级),须包含 onlineSource
    #[serde(default = "default_lyric_provider_order")]
    pub lyric_provider_order: Vec<String>,
    /// 键为歌词来源 id，值见 [`ProviderLyricSetting`]
    #[serde(default)]
    pub lyric_provider_settings: HashMap<String, ProviderLyricSetting>,
    #[serde(default)]
    pub desktop_lyrics: DesktopLyricsConfig,
}

/// 单个歌词来源的偏好(与前端 `ProviderLyricSetting` 对应)；字段为空表示跟随来源/全局默认
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLyricSetting {
    /// 该来源选用的算法 id;None = 用来源默认算法
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// 该来源的最终文本类型(original / translation / roman / auto);None = 跟随全局 preferTranslation
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefer_kind: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLyricsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_desktop_lyrics_locked")]
    pub locked: bool,
    #[serde(default = "default_desktop_lyrics_font_size")]
    pub font_size: i32,
    /// auto按背景亮度自动切换深/浅文字; 可选 dark/light/blue/pink/orange/green
    #[serde(default = "default_desktop_lyrics_color_preset")]
    pub color_preset: String,
}

fn default_desktop_lyrics_locked() -> bool {
    true
}

fn default_desktop_lyrics_font_size() -> i32 {
    28
}

fn default_desktop_lyrics_color_preset() -> String {
    "auto".to_string()
}

const fn default_true() -> bool {
    true
}

fn default_online_source() -> String {
    "netease".to_string()
}

/// 歌词来源顺序的默认值:与 `default_online_source` 保持一致
fn default_lyric_provider_order() -> Vec<String> {
    vec!["netease".to_string()]
}

fn default_lyrics_alignment() -> String {
    "center".to_string()
}

fn default_lyrics_font_family() -> String {
    "Noto Sans SC".to_string()
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
            sort_order: default_sort_order(),
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
            show_queue_info: true,
            immersive_color_scheme: default_immersive_color_scheme(),
            immersive_auto_theme: true,
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
            preferred_device_id: None,
            usb_dac_exclusive: false,
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
            lyrics_font_family: default_lyrics_font_family(),
            translation_font_family: String::new(),
            show_no_lyrics_hint: true,
            show_fetch_lyrics_button: true,
            lyrics_style: default_lyrics_style(),
            font_scale: default_font_scale(),
            auto_select_best_lyrics: true,
            lyric_provider_order: default_lyric_provider_order(),
            lyric_provider_settings: HashMap::new(),
            desktop_lyrics: DesktopLyricsConfig::default(),
        }
    }
}

impl Default for DesktopLyricsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            locked: true,
            font_size: 28,
            color_preset: default_desktop_lyrics_color_preset(),
        }
    }
}

pub struct ConfigManager {
    config_dir: String,
    /// 进程内配置缓存（Arc 共享，读命中时免磁盘 IO + JSON 解析 + 目录扫描）。
    /// save_config 写穿更新，reset_config 清空；ConfigManager 在 AppState 中单例存在
    cache: RwLock<Option<Arc<AppConfig>>>,
    /// 写互斥锁:串行化「读-改-写」序列,所有这类修改都必须走 [`Self::update_config`]
    ///
    /// 不加锁时并发的 load_config() -> 修改 -> save_config() 会读到同一份旧值，
    /// 后写覆盖先写 (TOCTOU 丢失更新)。
    write_lock: Mutex<()>,
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
            write_lock: Mutex::new(()),
        }
    }

    fn store_cache(&self, config: &AppConfig) {
        if let Ok(mut guard) = self.cache.write() {
            *guard = Some(Arc::new(config.clone()));
        }
    }

    fn cached_config(&self) -> Option<AppConfig> {
        let guard = self.cache.read().ok()?;
        guard.as_deref().cloned()
    }

    fn get_app_config_dir() -> Result<String, Box<dyn std::error::Error>> {
        // Android 改写数据目录，见 crate::config::DATA_DIR_OVERRIDE
        if let Some(dir) = crate::config::data_dir_override() {
            return Ok(dir.join("data").to_string_lossy().to_string());
        }
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

    /// 从文件读取配置,兼容旧版前端 plugin-store 包装格式 `{"appConfig": {...}}`
    ///
    /// 必须显式识别包装格式:serde 会忽略未知字段，反序列化包装文件只会得到全默认值、清空用户配置。
    /// `Err` 也不能折叠成 `Ok(None)`(那是"文件不存在/首次运行"),否则损坏文件会被默认值覆盖落盘。
    fn read_config_file(file_path: &str) -> Result<Option<AppConfig>, AppError> {
        if !Path::new(file_path).exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(file_path)
            .map_err(|e| AppError::Config(format!("读取配置文件失败: {e}")))?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| AppError::Config(format!("配置文件不是合法 JSON(可能已损坏): {e}")))?;
        let target = value.get("appConfig").unwrap_or(&value);
        serde_json::from_value::<AppConfig>(target.clone())
            .map(Some)
            .map_err(|e| AppError::Config(format!("配置文件字段不符合预期(可能已损坏): {e}")))
    }

    /// 把无法解析的配置文件改名为 `<config>.corrupt-<时间戳>` 备份
    ///
    /// 备份要空出原路径,后续 `save_config` 才不会覆盖用户配置;备份失败时必须提示用户手工移走。
    fn backup_corrupt_config(&self) {
        let config_path = self.get_config_path();
        // 毫秒时间戳 + 同名避让,避免反复启动时因目标名已存在而 rename 失败
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let mut backup_path = format!("{config_path}.corrupt-{timestamp}");
        let mut suffix = 1;
        while Path::new(&backup_path).exists() {
            backup_path = format!("{config_path}.corrupt-{timestamp}-{suffix}");
            suffix += 1;
        }
        match std::fs::rename(&config_path, &backup_path) {
            Ok(()) => log::error!("配置文件无法解析,已备份为 {backup_path},将以默认配置启动"),
            Err(e) => log::error!(
                "配置文件无法解析,且备份失败({e});原文件仍在 {config_path},\
                 请手工移走后重启,否则它会被后续保存覆盖"
            ),
        }
    }

    /// 是否为旧版 plugin-store 包装格式,键名见 [`Self::read_config_file`]
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

    /// 旧版布局一次性迁移(幂等):新文件有效就复用(旧包装格式重写为裸格式);
    /// 无有效配置时回退旧 <exe>/config/user.json(后端旧权威文件,含 lastSession);
    /// 两者皆无则只清理残留旧目录,后续走 AppConfig::default()
    ///
    /// 只有新文件成功落盘后才删除旧目录,避免迁移中断丢失配置。
    fn migrate_legacy_config(&self) {
        let config_path = self.get_config_path();
        let legacy_dir = Self::get_legacy_config_dir();

        match Self::read_config_file(&config_path) {
            Ok(Some(config)) => {
                if Self::is_plugin_store_wrapped(&config_path) {
                    log::info!("检测到旧 plugin-store 包装格式,重写为裸格式: {config_path}");
                    if let Err(e) = Self::save_config_to_file(&config, &config_path) {
                        log::error!("重写裸格式配置失败，保留旧配置目录以便回退: {e}");
                        return;
                    }
                }
                Self::remove_legacy_config_dir(legacy_dir);
                return;
            }
            Ok(None) => {}
            Err(e) => {
                // 旧目录是最后一份可用副本:损坏时既不覆盖也不删,备份与回退交给 load_config
                log::warn!("配置文件已损坏,跳过旧版迁移以免覆盖: {e}");
                return;
            }
        }

        if let Some(dir) = &legacy_dir {
            let legacy_user = format!("{dir}/user.json");
            if let Ok(Some(config)) = Self::read_config_file(&legacy_user) {
                log::info!("迁移旧配置文件: {legacy_user} -> {config_path}");
                if Self::save_config_to_file(&config, &config_path).is_ok() {
                    Self::remove_legacy_config_dir(legacy_dir);
                }
                return;
            }
        }

        Self::remove_legacy_config_dir(legacy_dir);
    }

    pub fn initialize_config_files(&self) -> Result<(), AppError> {
        std::fs::create_dir_all(&self.config_dir)
            .map_err(|e| AppError::Config(format!("创建配置目录失败: {e}")))?;

        self.cleanup_temp_files();

        self.migrate_legacy_config();

        Ok(())
    }

    /// 清除 .tmp 残留：原子写入中途崩溃留下的，成因见 [`Self::save_config_to_file`]
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

    /// 并回默认外链白名单:新增域名在旧配置文件里也要生效,导入的配置不能靠删域名绕开白名单限制
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
        // 命中即返回,省掉的开销见 Self::cache
        if let Some(cached) = self.cached_config() {
            return Ok(cached);
        }

        self.initialize_config_files()?;

        let config_path = self.get_config_path();
        match Self::read_config_file(&config_path) {
            Ok(Some(config)) => {
                log::info!("Loaded user configuration from: {config_path}");
                let config = Self::with_default_allowed_hosts(config);
                self.store_cache(&config);
                return Ok(config);
            }
            Ok(None) => {
                log::info!("No configuration file yet, using compiled-in defaults");
            }
            Err(e) => {
                // 备份优先于回退默认值,顺序理由见 backup_corrupt_config
                log::error!("配置文件损坏,无法加载: {e}");
                self.backup_corrupt_config();
            }
        }

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
        self.store_cache(config);
        Ok(())
    }

    /// 在写锁保护下原子地「读-改-写」配置:`f` 拿到的是最新落盘值,写回也不会被并发修改插队
    ///
    /// 凡是「先 load 再 save」的地方都应改用本方法,锁的必要性见字段 write_lock。
    pub fn update_config<F, T>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut AppConfig) -> T,
    {
        // 锁中毒时不能沿用可能不一致的内存状态,直接报错
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| AppError::Config("配置写锁已中毒，无法安全修改配置".to_string()))?;
        let mut config = self.load_config()?;
        let result = f(&mut config);
        self.save_config(&config)?;
        Ok(result)
    }

    fn save_config_to_file(config: &AppConfig, file_path: &str) -> Result<(), AppError> {
        use std::io::Write;
        let content = serde_json::to_string_pretty(config)
            .map_err(|e| AppError::Config(format!("Failed to serialize config: {e}")))?;
        // 原子写入:写 .tmp -> sync_all(rename 前数据必须已落盘,否则断电会留下空文件) -> rename。
        // 中途崩溃留下的 .tmp 由启动时的 cleanup_temp_files 清理。
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
        // 导入的配置不能削弱白名单，见 Self::with_default_allowed_hosts
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
        // 文件已删就必须作废缓存,否则 load_config 还会返回旧值
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
        assert_eq!(config.music_directories, [] as [String; 0]);
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

        assert_eq!(deserialized.music_directories, config.music_directories);
        assert!(approx_eq(deserialized.audio.volume, 0.7));
        assert_eq!(deserialized.general.language, "en");
        assert!(deserialized.last_session.is_some());
    }

    #[test]
    fn test_app_config_camel_case_serialization() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).expect("serialize failed");
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
        assert!(json.contains("\"translationFontFamily\""));
        assert!(json.contains("\"showNoLyricsHint\""));
        assert!(json.contains("\"desktopLyrics\""));
        assert!(json.contains("\"miniMode\""));
        assert!(json.contains("\"targetFps\""));
        assert!(json.contains("\"sortOrder\""));
        assert!(json.contains("\"lastSession\""));
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
        assert_eq!(config.sort_order, "asc");
    }

    #[test]
    fn test_audio_config_default() {
        let config = AudioConfig::default();
        assert!(!config.exclusive_mode);
        assert!(approx_eq(config.volume, 0.5));
        assert!(config.fade_enabled);
        assert!(config.preferred_device_id.is_none());
    }

    #[test]
    fn test_lyrics_config_default() {
        let config = LyricsConfig::default();
        assert!(!config.enable_online_fetch);
        assert!(config.auto_save_online_lyrics);
        assert!(config.prefer_translation);
        assert_eq!(config.online_source, "netease");
        assert_eq!(config.lyrics_alignment, "center");
        assert_eq!(config.lyrics_font_family, "Noto Sans SC");
        assert_eq!(config.lyrics_style, "modern");
        assert_eq!(config.translation_font_family, "");
        assert!(config.show_no_lyrics_hint);
        assert!(config.show_fetch_lyrics_button);
        assert!(!config.desktop_lyrics.enabled);
        assert!(config.desktop_lyrics.locked);
        assert_eq!(config.desktop_lyrics.font_size, 28);
        assert_eq!(config.desktop_lyrics.color_preset, "auto");
    }

    #[test]
    fn test_ui_and_visualizer_defaults() {
        let config = AppConfig::default();
        assert!(!config.ui.mini_mode);
        assert_eq!(config.visualizer.target_fps, 60);
        assert!(!config.visualizer.enable_vertical_sync);
        assert!(config.visualizer.detected_refresh_rate.is_none());
    }

    #[test]
    fn test_new_fields_parse_old_json_fragment() {
        // 缺字段的旧配置必须能解析，serde default 的兼容规则见 [`AppConfig`]
        let mut v = serde_json::to_value(AppConfig::default()).expect("serialize");
        // 剥掉新增键,模拟旧版 config.json
        v["playlist"].as_object_mut().unwrap().remove("sortOrder");
        let lyrics = v["lyrics"].as_object_mut().unwrap();
        lyrics.remove("translationFontFamily");
        lyrics.remove("showNoLyricsHint");
        lyrics.remove("showFetchLyricsButton");
        lyrics.remove("desktopLyrics");
        v.as_object_mut().unwrap().remove("ui");
        v.as_object_mut().unwrap().remove("visualizer");

        let config: AppConfig = serde_json::from_value(v).expect("old config must parse");
        assert_eq!(config.playlist.sort_order, "asc");
        assert_eq!(config.lyrics.lyrics_style, "modern");
        assert_eq!(config.lyrics.desktop_lyrics.font_size, 28);
        assert_eq!(config.visualizer.target_fps, 60);
        assert!(!config.ui.mini_mode);
        assert!(config.last_session.is_none());
    }

    #[test]
    fn test_general_config_default() {
        let config = GeneralConfig::default();
        assert_eq!(config.language, "zh");
        assert_eq!(config.theme, "auto");
        assert!(config.startup_load_last_config);
        assert!(config.auto_save_config);
        assert!(config.show_audio_info);
        assert!(config.show_queue_info);
        assert_eq!(config.immersive_color_scheme, "album");
        assert!(config.immersive_auto_theme);
        assert!(!config.enable_auto_update);
        assert_ne!(config.external_url_allowed_hosts, [] as [String; 0]);
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
        assert_eq!(snapshot.path, "");
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

    /// 歌词来源字段必须能在后端往返，serde 静默丢字段的规则与后果见 [`AppConfig`]
    #[test]
    fn test_lyrics_provider_settings_roundtrip() {
        let raw = r#"{
            "enableOnlineFetch": false,
            "autoSaveOnlineLyrics": true,
            "preferTranslation": true,
            "onlineSource": "qq",
            "autoSelectBestLyrics": false,
            "lyricProviderOrder": ["qq", "netease", "lrclib"],
            "lyricProviderSettings": {
                "qq": { "method": "web", "preferKind": "translation" },
                "lrclib": { "method": "search" }
            }
        }"#;

        let parsed: LyricsConfig = serde_json::from_str(raw).expect("应能解析前端配置");
        assert!(!parsed.auto_select_best_lyrics);
        assert_eq!(
            parsed.lyric_provider_order,
            vec![
                "qq".to_string(),
                "netease".to_string(),
                "lrclib".to_string()
            ]
        );
        assert_eq!(
            parsed
                .lyric_provider_settings
                .get("qq")
                .and_then(|s| s.prefer_kind.as_deref()),
            Some("translation")
        );

        // 再序列化回去,三个字段都要在(不被丢弃),且空字段被省略
        let json = serde_json::to_string(&parsed).expect("应能序列化");
        assert!(json.contains("\"autoSelectBestLyrics\":false"));
        assert!(json.contains("\"lyricProviderOrder\":[\"qq\",\"netease\",\"lrclib\"]"));
        assert!(json.contains("\"lyricProviderSettings\""));
        assert!(json.contains("\"preferKind\":\"translation\""));
        // lrclib 只设置了 method,序列化不应冒出空的 preferKind
        let round_tripped: LyricsConfig = serde_json::from_str(&json).expect("应能往返");
        let lrclib = round_tripped
            .lyric_provider_settings
            .get("lrclib")
            .expect("lrclib 偏好应保留");
        assert_eq!(lrclib.method.as_deref(), Some("search"));
        assert!(lrclib.prefer_kind.is_none());
    }

    /// 老配置文件没有这几个字段时应落到默认值(自动择优 + netease)
    #[test]
    fn test_lyrics_provider_settings_defaults() {
        let legacy: LyricsConfig =
            serde_json::from_str(r#"{"onlineSource":"lrclib"}"#).expect("旧配置应能解析");
        assert!(legacy.auto_select_best_lyrics);
        assert_eq!(legacy.lyric_provider_order, vec!["netease".to_string()]);
        assert!(legacy.lyric_provider_settings.is_empty());
    }
}
