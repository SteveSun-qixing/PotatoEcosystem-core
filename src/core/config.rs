//! 配置管理系统
//!
//! 本模块实现薯片微内核的配置管理功能，包括：
//!
//! - 配置文件加载（支持 YAML、JSON 格式）
//! - 配置层级覆盖（默认 < 系统 < 用户 < 环境变量）
//! - 配置读写 API（支持嵌套路径访问）
//! - 配置变更监听
//! - 配置热更新（骨架实现）
//!
//! # 示例
//!
//! ```rust,no_run
//! use chips_core::core::config::{ConfigManager, ConfigSource};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut manager = ConfigManager::new();
//!     
//!     // 加载配置文件
//!     manager.load_from_file("config.yaml").await?;
//!     
//!     // 读取配置值
//!     let worker_count: usize = manager.get("router.worker_count").await.unwrap_or(4);
//!     
//!     // 设置配置值
//!     manager.set("router.worker_count", 8).await?;
//!     
//!     // 监听配置变更
//!     let watcher_id = manager.on_change("router", |old, new| {
//!         println!("router config changed");
//!     }).await;
//!     
//!     Ok(())
//! }
//! ```

use crate::utils::{generate_id, CoreError, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

// ============================================================================
// 配置结构体定义
// ============================================================================

/// 路由器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    /// 工作线程数（默认 CPU 核心数 × 2）
    #[serde(default = "default_worker_count")]
    pub worker_count: usize,

    /// 最大并发请求数
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,

    /// 请求队列大小
    #[serde(default = "default_queue_size")]
    pub queue_size: usize,

    /// 默认超时时间（毫秒）
    #[serde(default = "default_timeout_ms")]
    pub default_timeout_ms: u64,
}

fn default_worker_count() -> usize {
    num_cpus::get() * 2
}

fn default_max_concurrent() -> usize {
    1000
}

fn default_queue_size() -> usize {
    10000
}

fn default_timeout_ms() -> u64 {
    30000
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            worker_count: default_worker_count(),
            max_concurrent: default_max_concurrent(),
            queue_size: default_queue_size(),
            default_timeout_ms: default_timeout_ms(),
        }
    }
}

/// 日志配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// 日志级别
    #[serde(default = "default_log_level")]
    pub level: String,

    /// 是否输出到文件
    #[serde(default)]
    pub file_output: bool,

    /// 日志文件目录
    #[serde(default)]
    pub log_dir: Option<PathBuf>,

    /// 是否输出 JSON 格式
    #[serde(default)]
    pub json_format: bool,

    /// 日志轮转策略
    #[serde(default = "default_rotation")]
    pub rotation: String,

    /// 保留日志文件数
    #[serde(default = "default_max_files")]
    pub max_files: usize,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_rotation() -> String {
    "daily".to_string()
}

fn default_max_files() -> usize {
    7
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file_output: false,
            log_dir: None,
            json_format: false,
            rotation: default_rotation(),
            max_files: default_max_files(),
        }
    }
}

/// 模块管理配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleConfig {
    /// 模块目录列表
    #[serde(default)]
    pub module_dirs: Vec<PathBuf>,

    /// 是否启用热插拔
    #[serde(default = "default_true")]
    pub hot_reload: bool,

    /// 健康检查间隔（秒）
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval_secs: u64,

    /// 自动加载的模块列表
    #[serde(default)]
    pub auto_load: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_health_check_interval() -> u64 {
    30
}

impl Default for ModuleConfig {
    fn default() -> Self {
        Self {
            module_dirs: vec![],
            hot_reload: true,
            health_check_interval_secs: default_health_check_interval(),
            auto_load: vec![],
        }
    }
}

/// 内核配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfig {
    /// 配置文件路径
    #[serde(skip)]
    pub config_path: Option<PathBuf>,

    /// 路由器配置
    #[serde(default)]
    pub router: RouterConfig,

    /// 日志配置
    #[serde(default)]
    pub logging: LogConfig,

    /// 模块管理配置
    #[serde(default)]
    pub modules: ModuleConfig,

    /// 是否为开发模式
    #[serde(default)]
    pub dev_mode: bool,

    /// 数据目录
    #[serde(default)]
    pub data_dir: Option<PathBuf>,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            config_path: None,
            router: RouterConfig::default(),
            logging: LogConfig::default(),
            modules: ModuleConfig::default(),
            dev_mode: false,
            data_dir: None,
        }
    }
}

impl CoreConfig {
    /// 创建配置构建器
    pub fn builder() -> CoreConfigBuilder {
        CoreConfigBuilder::new()
    }

    /// 从文件加载配置
    pub async fn from_file(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let content = tokio::fs::read_to_string(&path).await?;

        let mut config: CoreConfig = if path.extension().map(|e| e == "json").unwrap_or(false) {
            serde_json::from_str(&content)?
        } else {
            serde_yaml::from_str(&content)?
        };

        config.config_path = Some(path);
        Ok(config)
    }

    /// 合并另一个配置（用于覆盖）
    pub fn merge(&mut self, other: CoreConfig) {
        // 只覆盖非默认值的配置
        if other.router.worker_count != default_worker_count() {
            self.router.worker_count = other.router.worker_count;
        }
        if other.router.max_concurrent != default_max_concurrent() {
            self.router.max_concurrent = other.router.max_concurrent;
        }
        if other.logging.level != default_log_level() {
            self.logging.level = other.logging.level;
        }
        if other.logging.file_output {
            self.logging.file_output = true;
            self.logging.log_dir = other.logging.log_dir;
        }
        if !other.modules.module_dirs.is_empty() {
            self.modules.module_dirs.extend(other.modules.module_dirs);
        }
        if other.dev_mode {
            self.dev_mode = true;
        }
        if other.data_dir.is_some() {
            self.data_dir = other.data_dir;
        }
    }
}

/// 配置构建器
#[derive(Debug, Default)]
pub struct CoreConfigBuilder {
    config: CoreConfig,
}

impl CoreConfigBuilder {
    /// 创建新的构建器
    pub fn new() -> Self {
        Self {
            config: CoreConfig::default(),
        }
    }

    /// 设置配置文件路径
    pub fn config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.config_path = Some(path.into());
        self
    }

    /// 设置工作线程数
    pub fn worker_count(mut self, count: usize) -> Self {
        self.config.router.worker_count = count;
        self
    }

    /// 设置最大并发数
    pub fn max_concurrent(mut self, count: usize) -> Self {
        self.config.router.max_concurrent = count;
        self
    }

    /// 设置日志级别
    pub fn log_level(mut self, level: impl Into<String>) -> Self {
        self.config.logging.level = level.into();
        self
    }

    /// 启用文件日志
    pub fn file_logging(mut self, log_dir: impl Into<PathBuf>) -> Self {
        self.config.logging.file_output = true;
        self.config.logging.log_dir = Some(log_dir.into());
        self
    }

    /// 启用 JSON 格式日志
    pub fn json_logging(mut self) -> Self {
        self.config.logging.json_format = true;
        self
    }

    /// 添加模块目录
    pub fn module_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config.modules.module_dirs.push(dir.into());
        self
    }

    /// 启用开发模式
    pub fn dev_mode(mut self) -> Self {
        self.config.dev_mode = true;
        self
    }

    /// 设置数据目录
    pub fn data_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config.data_dir = Some(dir.into());
        self
    }

    /// 构建配置
    pub fn build(self) -> CoreConfig {
        self.config
    }
}

// num_cpus 是一个可选依赖，如果不可用则使用默认值
mod num_cpus {
    pub fn get() -> usize {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    }
}

// ============================================================================
// 配置管理器
// ============================================================================

/// 配置源类型
///
/// 定义配置的来源，用于实现层级覆盖
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigSource {
    /// 默认配置（最低优先级）
    Default = 0,
    /// 系统配置
    System = 1,
    /// 用户配置
    User = 2,
    /// 环境变量配置（最高优先级）
    Environment = 3,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigSource::Default => write!(f, "default"),
            ConfigSource::System => write!(f, "system"),
            ConfigSource::User => write!(f, "user"),
            ConfigSource::Environment => write!(f, "environment"),
        }
    }
}

/// 配置变更回调函数类型
pub type ConfigChangeCallback = Box<dyn Fn(&serde_json::Value, &serde_json::Value) + Send + Sync>;

/// 配置监听器
struct ConfigWatcher {
    /// 监听器 ID
    id: String,
    /// 监听的配置路径（空字符串表示监听所有）
    path: String,
    /// 回调函数
    callback: ConfigChangeCallback,
}

/// 配置验证器类型
pub type ConfigValidator = Box<dyn Fn(&serde_json::Value) -> Result<()> + Send + Sync>;

/// 配置管理器
///
/// 提供统一的配置管理能力，包括：
/// - 多源配置加载（文件、环境变量）
/// - 配置层级覆盖
/// - 配置读写 API
/// - 配置变更监听
/// - 配置热更新（骨架）
pub struct ConfigManager {
    /// 配置数据（JSON 格式存储，便于动态访问）
    config: Arc<RwLock<serde_json::Value>>,
    /// 配置路径列表（按加载顺序）
    config_paths: Arc<RwLock<Vec<(PathBuf, ConfigSource)>>>,
    /// 配置监听器
    watchers: Arc<RwLock<Vec<ConfigWatcher>>>,
    /// 配置验证器
    validators: Arc<RwLock<HashMap<String, ConfigValidator>>>,
    /// 环境变量前缀
    env_prefix: String,
    /// 是否启用热更新
    hot_reload_enabled: Arc<RwLock<bool>>,
    /// 保存路径（用于 save() 方法）
    save_path: Arc<RwLock<Option<PathBuf>>>,
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigManager {
    /// 创建新的配置管理器
    ///
    /// # Example
    ///
    /// ```
    /// use chips_core::core::config::ConfigManager;
    ///
    /// let manager = ConfigManager::new();
    /// ```
    pub fn new() -> Self {
        Self {
            config: Arc::new(RwLock::new(serde_json::json!({}))),
            config_paths: Arc::new(RwLock::new(Vec::new())),
            watchers: Arc::new(RwLock::new(Vec::new())),
            validators: Arc::new(RwLock::new(HashMap::new())),
            env_prefix: "CHIPS".to_string(),
            hot_reload_enabled: Arc::new(RwLock::new(false)),
            save_path: Arc::new(RwLock::new(None)),
        }
    }

    /// 使用自定义环境变量前缀创建配置管理器
    ///
    /// # Arguments
    ///
    /// * `env_prefix` - 环境变量前缀
    ///
    /// # Example
    ///
    /// ```
    /// use chips_core::core::config::ConfigManager;
    ///
    /// let manager = ConfigManager::with_env_prefix("MY_APP");
    /// // 将读取 MY_APP_* 环境变量
    /// ```
    pub fn with_env_prefix(env_prefix: impl Into<String>) -> Self {
        Self {
            env_prefix: env_prefix.into(),
            ..Self::new()
        }
    }

    /// 使用构建器模式创建配置管理器
    pub fn builder() -> ConfigManagerBuilder {
        ConfigManagerBuilder::new()
    }

    // ========================================================================
    // Task 4.5: 配置文件加载
    // ========================================================================

    /// 从文件加载配置
    ///
    /// 支持 YAML 和 JSON 格式，根据文件扩展名自动检测
    ///
    /// # Arguments
    ///
    /// * `path` - 配置文件路径
    ///
    /// # Returns
    ///
    /// 成功返回 `Ok(())`，失败返回错误
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::core::config::ConfigManager;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let mut manager = ConfigManager::new();
    ///     manager.load_from_file("config.yaml").await.unwrap();
    /// }
    /// ```
    pub async fn load_from_file(&self, path: impl AsRef<Path>) -> Result<()> {
        self.load_from_file_with_source(path, ConfigSource::User)
            .await
    }

    /// 从文件加载配置（指定来源）
    ///
    /// # Arguments
    ///
    /// * `path` - 配置文件路径
    /// * `source` - 配置来源（用于层级覆盖）
    pub async fn load_from_file_with_source(
        &self,
        path: impl AsRef<Path>,
        source: ConfigSource,
    ) -> Result<()> {
        let path = path.as_ref();
        let path_buf = path.to_path_buf();

        // 读取文件内容
        let content = tokio::fs::read_to_string(path).await.map_err(|e| {
            CoreError::ConfigLoadFailed(format!("无法读取配置文件 '{}': {}", path.display(), e))
        })?;

        // 根据扩展名解析
        let new_config: serde_json::Value = match path.extension().and_then(|e| e.to_str()) {
            Some("json") => serde_json::from_str(&content).map_err(|e| {
                CoreError::ConfigLoadFailed(format!("JSON 解析失败 '{}': {}", path.display(), e))
            })?,
            Some("yaml") | Some("yml") => serde_yaml::from_str(&content).map_err(|e| {
                CoreError::ConfigLoadFailed(format!("YAML 解析失败 '{}': {}", path.display(), e))
            })?,
            _ => {
                // 尝试 YAML，因为 YAML 是 JSON 的超集
                serde_yaml::from_str(&content).map_err(|e| {
                    CoreError::ConfigLoadFailed(format!(
                        "配置解析失败 '{}': {}",
                        path.display(),
                        e
                    ))
                })?
            }
        };

        // 验证配置
        self.validate_config(&new_config).await?;

        // 合并配置
        self.merge_config(new_config, source).await?;

        // 记录配置路径
        let mut paths = self.config_paths.write().await;
        paths.push((path_buf.clone(), source));

        // 设置保存路径（如果是用户配置）
        if source == ConfigSource::User {
            let mut save_path = self.save_path.write().await;
            *save_path = Some(path_buf);
        }

        Ok(())
    }

    /// 从多个路径加载配置
    ///
    /// 按顺序加载，后面的配置覆盖前面的
    ///
    /// # Arguments
    ///
    /// * `paths` - 配置文件路径列表（按优先级从低到高）
    pub async fn load_from_paths<P: AsRef<Path>>(&self, paths: &[P]) -> Result<()> {
        for path in paths {
            if path.as_ref().exists() {
                self.load_from_file(path).await?;
            }
        }
        Ok(())
    }

    /// 从字符串加载配置
    ///
    /// # Arguments
    ///
    /// * `content` - 配置内容（YAML 或 JSON 格式）
    /// * `format` - 格式（"yaml" 或 "json"）
    pub async fn load_from_str(&self, content: &str, format: &str) -> Result<()> {
        let config: serde_json::Value = match format {
            "json" => serde_json::from_str(content).map_err(|e| {
                CoreError::ConfigLoadFailed(format!("JSON 解析失败: {}", e))
            })?,
            "yaml" | "yml" => serde_yaml::from_str(content).map_err(|e| {
                CoreError::ConfigLoadFailed(format!("YAML 解析失败: {}", e))
            })?,
            _ => {
                return Err(CoreError::ConfigLoadFailed(format!(
                    "不支持的配置格式: {}",
                    format
                )));
            }
        };

        self.validate_config(&config).await?;
        self.merge_config(config, ConfigSource::User).await
    }

    // ========================================================================
    // Task 4.6: 配置层级覆盖
    // ========================================================================

    /// 加载完整的配置层级
    ///
    /// 按以下顺序加载配置（后者覆盖前者）：
    /// 1. 默认配置
    /// 2. 系统配置
    /// 3. 用户配置
    /// 4. 环境变量
    ///
    /// # Arguments
    ///
    /// * `default_config` - 默认配置（可选）
    /// * `system_path` - 系统配置路径（可选）
    /// * `user_path` - 用户配置路径（可选）
    pub async fn load_layered(
        &self,
        default_config: Option<serde_json::Value>,
        system_path: Option<&Path>,
        user_path: Option<&Path>,
    ) -> Result<()> {
        // 1. 加载默认配置
        if let Some(config) = default_config {
            self.merge_config(config, ConfigSource::Default).await?;
        }

        // 2. 加载系统配置
        if let Some(path) = system_path {
            if path.exists() {
                self.load_from_file_with_source(path, ConfigSource::System)
                    .await?;
            }
        }

        // 3. 加载用户配置
        if let Some(path) = user_path {
            if path.exists() {
                self.load_from_file_with_source(path, ConfigSource::User)
                    .await?;
            }
        }

        // 4. 加载环境变量
        self.load_from_environment().await?;

        Ok(())
    }

    /// 从环境变量加载配置
    ///
    /// 环境变量格式：`{PREFIX}_{PATH}`，其中 PATH 使用下划线分隔层级
    /// 例如：`CHIPS_ROUTER_WORKER_COUNT=8` 对应 `router.worker_count`
    pub async fn load_from_environment(&self) -> Result<()> {
        let prefix = format!("{}_", self.env_prefix);
        let mut env_config = serde_json::json!({});

        for (key, value) in std::env::vars() {
            if let Some(config_key) = key.strip_prefix(&prefix) {
                // 转换环境变量名为配置路径
                // ROUTER_WORKER_COUNT -> router.worker_count
                let path = config_key.to_lowercase().replace('_', ".");

                // 尝试解析值
                let parsed_value = Self::parse_env_value(&value);

                // 设置到配置中
                Self::set_nested_value(&mut env_config, &path, parsed_value);
            }
        }

        if env_config != serde_json::json!({}) {
            self.merge_config(env_config, ConfigSource::Environment)
                .await?;
        }

        Ok(())
    }

    /// 解析环境变量值
    fn parse_env_value(value: &str) -> serde_json::Value {
        // 尝试解析为布尔值
        if value.eq_ignore_ascii_case("true") {
            return serde_json::Value::Bool(true);
        }
        if value.eq_ignore_ascii_case("false") {
            return serde_json::Value::Bool(false);
        }

        // 尝试解析为整数
        if let Ok(n) = value.parse::<i64>() {
            return serde_json::Value::Number(n.into());
        }

        // 尝试解析为浮点数
        if let Ok(n) = value.parse::<f64>() {
            if let Some(n) = serde_json::Number::from_f64(n) {
                return serde_json::Value::Number(n);
            }
        }

        // 尝试解析为 JSON 数组或对象
        if (value.starts_with('[') && value.ends_with(']'))
            || (value.starts_with('{') && value.ends_with('}'))
        {
            if let Ok(v) = serde_json::from_str(value) {
                return v;
            }
        }

        // 默认为字符串
        serde_json::Value::String(value.to_string())
    }

    /// 合并配置（深度合并）
    async fn merge_config(&self, new_config: serde_json::Value, source: ConfigSource) -> Result<()> {
        let mut config = self.config.write().await;
        let old_config = config.clone();

        // 深度合并
        Self::deep_merge(&mut config, new_config);

        // 通知变更
        drop(config);
        self.notify_changes("", &old_config, source).await;

        Ok(())
    }

    /// 深度合并两个 JSON 值
    fn deep_merge(target: &mut serde_json::Value, source: serde_json::Value) {
        match (target, source) {
            (serde_json::Value::Object(target_map), serde_json::Value::Object(source_map)) => {
                for (key, value) in source_map {
                    if let Some(target_value) = target_map.get_mut(&key) {
                        Self::deep_merge(target_value, value);
                    } else {
                        target_map.insert(key, value);
                    }
                }
            }
            (target, source) => {
                *target = source;
            }
        }
    }

    /// 设置嵌套值
    fn set_nested_value(config: &mut serde_json::Value, path: &str, value: serde_json::Value) {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = config;

        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                // 最后一个部分，设置值
                if let serde_json::Value::Object(map) = current {
                    map.insert((*part).to_string(), value.clone());
                }
            } else {
                // 中间部分，确保是对象
                if let serde_json::Value::Object(map) = current {
                    if !map.contains_key(*part) {
                        map.insert((*part).to_string(), serde_json::json!({}));
                    }
                    current = map.get_mut(*part).unwrap();
                }
            }
        }
    }

    // ========================================================================
    // Task 4.7: 配置读写 API
    // ========================================================================

    /// 读取配置值
    ///
    /// 支持嵌套路径访问，如 `router.worker_count`
    ///
    /// # Type Parameters
    ///
    /// * `T` - 目标类型，需要实现 `DeserializeOwned`
    ///
    /// # Arguments
    ///
    /// * `key` - 配置键（支持点号分隔的嵌套路径）
    ///
    /// # Returns
    ///
    /// 成功返回 `Some(T)`，未找到或类型不匹配返回 `None`
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::core::config::ConfigManager;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let manager = ConfigManager::new();
    ///     let count: Option<usize> = manager.get("router.worker_count").await;
    /// }
    /// ```
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let config = self.config.read().await;
        let value = self.get_nested_value(&config, key)?;
        serde_json::from_value(value.clone()).ok()
    }

    /// 读取配置值（带默认值）
    ///
    /// # Type Parameters
    ///
    /// * `T` - 目标类型
    ///
    /// # Arguments
    ///
    /// * `key` - 配置键
    /// * `default` - 默认值
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::core::config::ConfigManager;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let manager = ConfigManager::new();
    ///     let count: usize = manager.get_or("router.worker_count", 4).await;
    /// }
    /// ```
    pub async fn get_or<T: DeserializeOwned>(&self, key: &str, default: T) -> T {
        self.get(key).await.unwrap_or(default)
    }

    /// 读取配置值（返回 Result）
    ///
    /// # Arguments
    ///
    /// * `key` - 配置键
    ///
    /// # Returns
    ///
    /// 成功返回值，失败返回 `ConfigNotFound` 错误
    pub async fn get_required<T: DeserializeOwned>(&self, key: &str) -> Result<T> {
        self.get(key).await.ok_or_else(|| CoreError::ConfigNotFound(key.to_string()))
    }

    /// 获取嵌套值
    fn get_nested_value<'a>(
        &self,
        config: &'a serde_json::Value,
        path: &str,
    ) -> Option<&'a serde_json::Value> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = config;

        for part in parts {
            match current {
                serde_json::Value::Object(map) => {
                    current = map.get(part)?;
                }
                _ => return None,
            }
        }

        Some(current)
    }

    /// 设置配置值
    ///
    /// 支持嵌套路径，会自动创建中间对象
    ///
    /// # Type Parameters
    ///
    /// * `T` - 值类型，需要实现 `Serialize`
    ///
    /// # Arguments
    ///
    /// * `key` - 配置键
    /// * `value` - 配置值
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::core::config::ConfigManager;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let manager = ConfigManager::new();
    ///     manager.set("router.worker_count", 8).await.unwrap();
    /// }
    /// ```
    pub async fn set<T: Serialize>(&self, key: &str, value: T) -> Result<()> {
        let json_value = serde_json::to_value(value).map_err(|e| {
            CoreError::InvalidConfigValue {
                key: key.to_string(),
                reason: e.to_string(),
            }
        })?;

        let mut config = self.config.write().await;
        let old_config = config.clone();

        Self::set_nested_value(&mut config, key, json_value);

        // 通知变更
        drop(config);
        self.notify_changes(key, &old_config, ConfigSource::User).await;

        Ok(())
    }

    /// 删除配置值
    ///
    /// # Arguments
    ///
    /// * `key` - 配置键
    pub async fn remove(&self, key: &str) -> Result<()> {
        let parts: Vec<&str> = key.split('.').collect();
        if parts.is_empty() {
            return Ok(());
        }

        let mut config = self.config.write().await;
        let old_config = config.clone();

        // 找到父对象并删除键
        if parts.len() == 1 {
            if let serde_json::Value::Object(map) = &mut *config {
                map.remove(parts[0]);
            }
        } else {
            let parent_path = parts[..parts.len() - 1].join(".");
            let last_key = parts[parts.len() - 1];

            if let Some(parent) = self.get_nested_value_mut(&mut config, &parent_path) {
                if let serde_json::Value::Object(map) = parent {
                    map.remove(last_key);
                }
            }
        }

        drop(config);
        self.notify_changes(key, &old_config, ConfigSource::User).await;

        Ok(())
    }

    /// 获取嵌套值（可变引用）
    fn get_nested_value_mut<'a>(
        &self,
        config: &'a mut serde_json::Value,
        path: &str,
    ) -> Option<&'a mut serde_json::Value> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = config;

        for part in parts {
            match current {
                serde_json::Value::Object(map) => {
                    current = map.get_mut(part)?;
                }
                _ => return None,
            }
        }

        Some(current)
    }

    /// 保存配置到文件
    ///
    /// 将当前配置保存到指定路径或上次加载的用户配置路径
    ///
    /// # Arguments
    ///
    /// * `path` - 保存路径（可选，不指定则使用上次加载的路径）
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::core::config::ConfigManager;
    /// use std::path::Path;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let manager = ConfigManager::new();
    ///     // 保存到默认路径
    ///     manager.save(None::<&Path>).await.unwrap();
    ///     // 保存到指定路径
    ///     manager.save(Some("config.yaml")).await.unwrap();
    /// }
    /// ```
    pub async fn save<P: AsRef<Path>>(&self, path: Option<P>) -> Result<()> {
        let save_path = match path {
            Some(p) => p.as_ref().to_path_buf(),
            None => {
                let guard = self.save_path.read().await;
                guard.clone().ok_or_else(|| {
                    CoreError::ConfigLoadFailed("没有指定保存路径".to_string())
                })?
            }
        };

        let config = self.config.read().await;
        let content = match save_path.extension().and_then(|e| e.to_str()) {
            Some("json") => serde_json::to_string_pretty(&*config).map_err(|e| {
                CoreError::ConfigLoadFailed(format!("序列化失败: {}", e))
            })?,
            _ => serde_yaml::to_string(&*config).map_err(|e| {
                CoreError::ConfigLoadFailed(format!("序列化失败: {}", e))
            })?,
        };

        tokio::fs::write(&save_path, content).await.map_err(|e| {
            CoreError::ConfigLoadFailed(format!(
                "保存配置失败 '{}': {}",
                save_path.display(),
                e
            ))
        })?;

        Ok(())
    }

    /// 检查配置键是否存在
    pub async fn contains(&self, key: &str) -> bool {
        let config = self.config.read().await;
        self.get_nested_value(&config, key).is_some()
    }

    /// 获取所有顶级配置键
    pub async fn keys(&self) -> Vec<String> {
        let config = self.config.read().await;
        match &*config {
            serde_json::Value::Object(map) => map.keys().cloned().collect(),
            _ => vec![],
        }
    }

    /// 获取完整配置（只读）
    pub async fn get_all(&self) -> serde_json::Value {
        self.config.read().await.clone()
    }

    /// 将配置反序列化为指定类型
    pub async fn as_typed<T: DeserializeOwned>(&self) -> Result<T> {
        let config = self.config.read().await;
        serde_json::from_value(config.clone()).map_err(|e| {
            CoreError::ConfigLoadFailed(format!("类型转换失败: {}", e))
        })
    }

    // ========================================================================
    // Task 4.8: 配置变更监听
    // ========================================================================

    /// 注册配置变更监听器
    ///
    /// 当指定路径的配置发生变更时，回调函数会被调用
    ///
    /// # Arguments
    ///
    /// * `path` - 监听的配置路径（空字符串表示监听所有变更）
    /// * `callback` - 回调函数，接收旧值和新值
    ///
    /// # Returns
    ///
    /// 返回监听器 ID，用于后续取消监听
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::core::config::ConfigManager;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let manager = ConfigManager::new();
    ///     let watcher_id = manager.on_change("router", |old, new| {
    ///         println!("router config changed from {:?} to {:?}", old, new);
    ///     }).await;
    /// }
    /// ```
    pub async fn on_change<F>(&self, path: &str, callback: F) -> String
    where
        F: Fn(&serde_json::Value, &serde_json::Value) + Send + Sync + 'static,
    {
        let watcher_id = generate_id();
        let watcher = ConfigWatcher {
            id: watcher_id.clone(),
            path: path.to_string(),
            callback: Box::new(callback),
        };

        let mut watchers = self.watchers.write().await;
        watchers.push(watcher);

        watcher_id
    }

    /// 取消配置变更监听
    ///
    /// # Arguments
    ///
    /// * `watcher_id` - 监听器 ID（由 `on_change` 返回）
    ///
    /// # Returns
    ///
    /// 成功取消返回 `true`，未找到返回 `false`
    pub async fn off_change(&self, watcher_id: &str) -> bool {
        let mut watchers = self.watchers.write().await;
        let len_before = watchers.len();
        watchers.retain(|w| w.id != watcher_id);
        watchers.len() < len_before
    }

    /// 取消指定路径的所有监听
    pub async fn off_change_all(&self, path: &str) {
        let mut watchers = self.watchers.write().await;
        watchers.retain(|w| w.path != path);
    }

    /// 通知配置变更
    async fn notify_changes(&self, changed_path: &str, old_config: &serde_json::Value, _source: ConfigSource) {
        let watchers = self.watchers.read().await;
        let new_config = self.config.read().await;

        for watcher in watchers.iter() {
            // 检查路径是否匹配
            // 空路径监听所有变更
            // 否则检查变更路径是否以监听路径开头（或反过来）
            let should_notify = watcher.path.is_empty()
                || changed_path.starts_with(&watcher.path)
                || watcher.path.starts_with(changed_path);

            if should_notify {
                // 获取监听路径对应的旧值和新值
                let old_value = if watcher.path.is_empty() {
                    old_config.clone()
                } else {
                    self.get_nested_value(old_config, &watcher.path)
                        .cloned()
                        .unwrap_or(serde_json::Value::Null)
                };

                let new_value = if watcher.path.is_empty() {
                    new_config.clone()
                } else {
                    self.get_nested_value(&new_config, &watcher.path)
                        .cloned()
                        .unwrap_or(serde_json::Value::Null)
                };

                // 只有值实际变化时才通知
                if old_value != new_value {
                    (watcher.callback)(&old_value, &new_value);
                }
            }
        }
    }

    // ========================================================================
    // Task 4.9: 配置热更新（骨架实现）
    // ========================================================================

    /// 启用配置热更新
    ///
    /// 启用后，配置文件变化时会自动重新加载
    ///
    /// # Note
    ///
    /// 这是一个骨架实现，实际的文件监控需要 `notify` crate
    /// 完整实现将在后续版本中添加
    pub async fn enable_hot_reload(&self) -> Result<()> {
        let mut enabled = self.hot_reload_enabled.write().await;
        *enabled = true;

        // TODO: 使用 notify crate 实现文件监控
        // 骨架实现：记录状态，实际监控逻辑待实现
        tracing::info!("配置热更新已启用（骨架实现）");

        Ok(())
    }

    /// 禁用配置热更新
    pub async fn disable_hot_reload(&self) -> Result<()> {
        let mut enabled = self.hot_reload_enabled.write().await;
        *enabled = false;

        tracing::info!("配置热更新已禁用");

        Ok(())
    }

    /// 检查热更新是否启用
    pub async fn is_hot_reload_enabled(&self) -> bool {
        *self.hot_reload_enabled.read().await
    }

    /// 手动触发重新加载
    ///
    /// 重新从已加载的配置文件中读取配置
    pub async fn reload(&self) -> Result<()> {
        let paths = self.config_paths.read().await.clone();

        // 清空当前配置
        {
            let mut config = self.config.write().await;
            *config = serde_json::json!({});
        }

        // 按顺序重新加载
        for (path, source) in paths {
            if path.exists() {
                self.load_from_file_with_source(&path, source).await?;
            }
        }

        // 重新加载环境变量
        self.load_from_environment().await?;

        tracing::info!("配置已重新加载");

        Ok(())
    }

    // ========================================================================
    // 配置验证
    // ========================================================================

    /// 注册配置验证器
    ///
    /// # Arguments
    ///
    /// * `path` - 配置路径
    /// * `validator` - 验证函数
    pub async fn add_validator<F>(&self, path: &str, validator: F)
    where
        F: Fn(&serde_json::Value) -> Result<()> + Send + Sync + 'static,
    {
        let mut validators = self.validators.write().await;
        validators.insert(path.to_string(), Box::new(validator));
    }

    /// 移除配置验证器
    pub async fn remove_validator(&self, path: &str) {
        let mut validators = self.validators.write().await;
        validators.remove(path);
    }

    /// 验证配置
    async fn validate_config(&self, config: &serde_json::Value) -> Result<()> {
        let validators = self.validators.read().await;

        for (path, validator) in validators.iter() {
            if let Some(value) = self.get_nested_value(config, path) {
                validator(value)?;
            }
        }

        Ok(())
    }

    /// 验证完整配置
    pub async fn validate(&self) -> Result<()> {
        let config = self.config.read().await;
        drop(config);

        let config = self.config.read().await;
        self.validate_config(&config).await
    }
}

/// 配置管理器构建器
pub struct ConfigManagerBuilder {
    env_prefix: String,
    default_config: Option<serde_json::Value>,
    system_path: Option<PathBuf>,
    user_path: Option<PathBuf>,
}

impl Default for ConfigManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigManagerBuilder {
    /// 创建新的构建器
    pub fn new() -> Self {
        Self {
            env_prefix: "CHIPS".to_string(),
            default_config: None,
            system_path: None,
            user_path: None,
        }
    }

    /// 设置环境变量前缀
    pub fn env_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.env_prefix = prefix.into();
        self
    }

    /// 设置默认配置
    pub fn default_config(mut self, config: serde_json::Value) -> Self {
        self.default_config = Some(config);
        self
    }

    /// 设置系统配置路径
    pub fn system_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.system_path = Some(path.into());
        self
    }

    /// 设置用户配置路径
    pub fn user_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.user_path = Some(path.into());
        self
    }

    /// 构建并加载配置
    pub async fn build(self) -> Result<ConfigManager> {
        let manager = ConfigManager::with_env_prefix(self.env_prefix);

        manager
            .load_layered(
                self.default_config,
                self.system_path.as_deref(),
                self.user_path.as_deref(),
            )
            .await?;

        Ok(manager)
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    // ------------------------------------------------------------------------
    // CoreConfig 测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_default_config() {
        let config = CoreConfig::default();
        assert!(!config.dev_mode);
        assert_eq!(config.logging.level, "info");
        assert_eq!(config.router.default_timeout_ms, 30000);
    }

    #[test]
    fn test_config_builder() {
        let config = CoreConfig::builder()
            .worker_count(8)
            .log_level("debug")
            .dev_mode()
            .build();

        assert_eq!(config.router.worker_count, 8);
        assert_eq!(config.logging.level, "debug");
        assert!(config.dev_mode);
    }

    #[test]
    fn test_config_merge() {
        let mut base = CoreConfig::default();
        let override_config = CoreConfig::builder().log_level("debug").dev_mode().build();

        base.merge(override_config);

        assert_eq!(base.logging.level, "debug");
        assert!(base.dev_mode);
    }

    #[test]
    fn test_config_serialization() {
        let config = CoreConfig::builder()
            .worker_count(4)
            .log_level("warn")
            .build();

        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: CoreConfig = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(parsed.router.worker_count, 4);
        assert_eq!(parsed.logging.level, "warn");
    }

    // ------------------------------------------------------------------------
    // ConfigManager 基础测试
    // ------------------------------------------------------------------------

    #[tokio::test]
    async fn test_config_manager_new() {
        let manager = ConfigManager::new();
        assert!(manager.keys().await.is_empty());
    }

    #[tokio::test]
    async fn test_config_manager_with_env_prefix() {
        let manager = ConfigManager::with_env_prefix("TEST_APP");
        assert_eq!(manager.env_prefix, "TEST_APP");
    }

    // ------------------------------------------------------------------------
    // Task 4.5: 配置文件加载测试
    // ------------------------------------------------------------------------

    #[tokio::test]
    async fn test_load_yaml_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");

        let yaml_content = r#"
router:
  worker_count: 16
  max_concurrent: 2000
logging:
  level: debug
dev_mode: true
"#;
        tokio::fs::write(&config_path, yaml_content).await.unwrap();

        let manager = ConfigManager::new();
        manager.load_from_file(&config_path).await.unwrap();

        assert_eq!(
            manager.get::<usize>("router.worker_count").await,
            Some(16)
        );
        assert_eq!(
            manager.get::<String>("logging.level").await,
            Some("debug".to_string())
        );
        assert_eq!(manager.get::<bool>("dev_mode").await, Some(true));
    }

    #[tokio::test]
    async fn test_load_json_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");

        let json_content = r#"{
            "router": {
                "worker_count": 8,
                "queue_size": 5000
            },
            "logging": {
                "level": "warn",
                "json_format": true
            }
        }"#;
        tokio::fs::write(&config_path, json_content).await.unwrap();

        let manager = ConfigManager::new();
        manager.load_from_file(&config_path).await.unwrap();

        assert_eq!(manager.get::<usize>("router.worker_count").await, Some(8));
        assert_eq!(manager.get::<usize>("router.queue_size").await, Some(5000));
        assert_eq!(
            manager.get::<String>("logging.level").await,
            Some("warn".to_string())
        );
        assert_eq!(manager.get::<bool>("logging.json_format").await, Some(true));
    }

    #[tokio::test]
    async fn test_load_from_str() {
        let manager = ConfigManager::new();

        let yaml_content = r#"
server:
  host: localhost
  port: 8080
"#;
        manager.load_from_str(yaml_content, "yaml").await.unwrap();

        assert_eq!(
            manager.get::<String>("server.host").await,
            Some("localhost".to_string())
        );
        assert_eq!(manager.get::<u16>("server.port").await, Some(8080));
    }

    #[tokio::test]
    async fn test_load_multiple_paths() {
        let temp_dir = TempDir::new().unwrap();

        let config1_path = temp_dir.path().join("base.yaml");
        let config2_path = temp_dir.path().join("override.yaml");

        tokio::fs::write(&config1_path, "value: 1\nbase_only: true")
            .await
            .unwrap();
        tokio::fs::write(&config2_path, "value: 2\noverride_only: true")
            .await
            .unwrap();

        let manager = ConfigManager::new();
        manager
            .load_from_paths(&[&config1_path, &config2_path])
            .await
            .unwrap();

        // 后面的配置覆盖前面的
        assert_eq!(manager.get::<i32>("value").await, Some(2));
        // 两个配置的独有值都存在
        assert_eq!(manager.get::<bool>("base_only").await, Some(true));
        assert_eq!(manager.get::<bool>("override_only").await, Some(true));
    }

    #[tokio::test]
    async fn test_load_invalid_file() {
        let manager = ConfigManager::new();
        let result = manager.load_from_file("/nonexistent/config.yaml").await;
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------------
    // Task 4.6: 配置层级覆盖测试
    // ------------------------------------------------------------------------

    #[tokio::test]
    async fn test_layered_config() {
        let temp_dir = TempDir::new().unwrap();
        let system_path = temp_dir.path().join("system.yaml");
        let user_path = temp_dir.path().join("user.yaml");

        // 系统配置
        tokio::fs::write(&system_path, "level: system\nsystem_value: true")
            .await
            .unwrap();

        // 用户配置（覆盖系统配置的 level）
        tokio::fs::write(&user_path, "level: user\nuser_value: true")
            .await
            .unwrap();

        // 默认配置
        let default_config = serde_json::json!({
            "level": "default",
            "default_value": true
        });

        let manager = ConfigManager::new();
        manager
            .load_layered(
                Some(default_config),
                Some(system_path.as_path()),
                Some(user_path.as_path()),
            )
            .await
            .unwrap();

        // 用户配置优先级最高
        assert_eq!(
            manager.get::<String>("level").await,
            Some("user".to_string())
        );

        // 各层级的独有值都存在
        assert_eq!(manager.get::<bool>("default_value").await, Some(true));
        assert_eq!(manager.get::<bool>("system_value").await, Some(true));
        assert_eq!(manager.get::<bool>("user_value").await, Some(true));
    }

    #[tokio::test]
    async fn test_deep_merge() {
        let mut target = serde_json::json!({
            "a": {
                "b": 1,
                "c": 2
            },
            "d": 3
        });

        let source = serde_json::json!({
            "a": {
                "b": 10,
                "e": 5
            },
            "f": 6
        });

        ConfigManager::deep_merge(&mut target, source);

        assert_eq!(target["a"]["b"], 10); // 覆盖
        assert_eq!(target["a"]["c"], 2); // 保留
        assert_eq!(target["a"]["e"], 5); // 新增
        assert_eq!(target["d"], 3); // 保留
        assert_eq!(target["f"], 6); // 新增
    }

    #[tokio::test]
    async fn test_env_value_parsing() {
        // 布尔值
        assert_eq!(
            ConfigManager::parse_env_value("true"),
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            ConfigManager::parse_env_value("FALSE"),
            serde_json::Value::Bool(false)
        );

        // 整数
        assert_eq!(
            ConfigManager::parse_env_value("42"),
            serde_json::Value::Number(42.into())
        );

        // 浮点数
        let float_val = ConfigManager::parse_env_value("3.14");
        assert!(float_val.as_f64().unwrap() - 3.14 < 0.001);

        // JSON 数组
        assert_eq!(
            ConfigManager::parse_env_value("[1,2,3]"),
            serde_json::json!([1, 2, 3])
        );

        // 字符串
        assert_eq!(
            ConfigManager::parse_env_value("hello"),
            serde_json::Value::String("hello".to_string())
        );
    }

    // ------------------------------------------------------------------------
    // Task 4.7: 配置读写 API 测试
    // ------------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_set() {
        let manager = ConfigManager::new();

        // 设置值
        manager.set("key1", "value1").await.unwrap();
        manager.set("nested.key2", 42).await.unwrap();
        manager.set("nested.deep.key3", true).await.unwrap();

        // 读取值
        assert_eq!(
            manager.get::<String>("key1").await,
            Some("value1".to_string())
        );
        assert_eq!(manager.get::<i32>("nested.key2").await, Some(42));
        assert_eq!(manager.get::<bool>("nested.deep.key3").await, Some(true));

        // 不存在的键
        assert_eq!(manager.get::<String>("nonexistent").await, None);
    }

    #[tokio::test]
    async fn test_get_or() {
        let manager = ConfigManager::new();
        manager.set("existing", 10).await.unwrap();

        assert_eq!(manager.get_or("existing", 0).await, 10);
        assert_eq!(manager.get_or("nonexistent", 99).await, 99);
    }

    #[tokio::test]
    async fn test_get_required() {
        let manager = ConfigManager::new();
        manager.set("existing", "value").await.unwrap();

        assert!(manager.get_required::<String>("existing").await.is_ok());
        assert!(manager.get_required::<String>("nonexistent").await.is_err());
    }

    #[tokio::test]
    async fn test_contains() {
        let manager = ConfigManager::new();
        manager.set("key", "value").await.unwrap();

        assert!(manager.contains("key").await);
        assert!(!manager.contains("nonexistent").await);
    }

    #[tokio::test]
    async fn test_keys() {
        let manager = ConfigManager::new();
        manager.set("a", 1).await.unwrap();
        manager.set("b", 2).await.unwrap();
        manager.set("c.d", 3).await.unwrap();

        let keys = manager.keys().await;
        assert!(keys.contains(&"a".to_string()));
        assert!(keys.contains(&"b".to_string()));
        assert!(keys.contains(&"c".to_string()));
    }

    #[tokio::test]
    async fn test_remove() {
        let manager = ConfigManager::new();
        manager.set("key", "value").await.unwrap();
        assert!(manager.contains("key").await);

        manager.remove("key").await.unwrap();
        assert!(!manager.contains("key").await);
    }

    #[tokio::test]
    async fn test_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("saved.yaml");

        let manager = ConfigManager::new();
        manager.set("key1", "value1").await.unwrap();
        manager.set("nested.key2", 42).await.unwrap();

        // 保存
        manager.save(Some(&config_path)).await.unwrap();

        // 重新加载
        let manager2 = ConfigManager::new();
        manager2.load_from_file(&config_path).await.unwrap();

        assert_eq!(
            manager2.get::<String>("key1").await,
            Some("value1".to_string())
        );
        assert_eq!(manager2.get::<i32>("nested.key2").await, Some(42));
    }

    #[tokio::test]
    async fn test_as_typed() {
        let manager = ConfigManager::new();
        manager.set("router.worker_count", 8).await.unwrap();
        manager.set("router.max_concurrent", 1000).await.unwrap();
        manager.set("router.queue_size", 10000).await.unwrap();
        manager.set("router.default_timeout_ms", 30000u64).await.unwrap();

        // 转换为 RouterConfig
        let router: RouterConfig = manager.get_required("router").await.unwrap();
        assert_eq!(router.worker_count, 8);
        assert_eq!(router.max_concurrent, 1000);
    }

    // ------------------------------------------------------------------------
    // Task 4.8: 配置变更监听测试
    // ------------------------------------------------------------------------

    #[tokio::test]
    async fn test_on_change() {
        let manager = ConfigManager::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // 注册监听器
        let watcher_id = manager
            .on_change("test", move |_old, _new| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            })
            .await;

        // 触发变更
        manager.set("test.value", 1).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        manager.set("test.value", 2).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 2);

        // 取消监听
        assert!(manager.off_change(&watcher_id).await);

        // 不再触发
        manager.set("test.value", 3).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_path_specific_change() {
        let manager = ConfigManager::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // 只监听 router 路径
        manager
            .on_change("router", move |_old, _new| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            })
            .await;

        // router 路径变更应该触发
        manager.set("router.worker_count", 8).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // 其他路径不触发
        manager.set("logging.level", "debug").await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_global_change_listener() {
        let manager = ConfigManager::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // 监听所有变更（空路径）
        manager
            .on_change("", move |_old, _new| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            })
            .await;

        manager.set("a", 1).await.unwrap();
        manager.set("b.c", 2).await.unwrap();
        manager.set("d.e.f", 3).await.unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_off_change_all() {
        let manager = ConfigManager::new();
        let counter = Arc::new(AtomicUsize::new(0));

        // 注册多个相同路径的监听器
        for _ in 0..3 {
            let c = counter.clone();
            manager
                .on_change("test", move |_old, _new| {
                    c.fetch_add(1, Ordering::SeqCst);
                })
                .await;
        }

        manager.set("test.value", 1).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 3);

        // 取消所有 test 路径的监听
        manager.off_change_all("test").await;

        manager.set("test.value", 2).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    // ------------------------------------------------------------------------
    // Task 4.9: 配置热更新测试
    // ------------------------------------------------------------------------

    #[tokio::test]
    async fn test_hot_reload_enable_disable() {
        let manager = ConfigManager::new();

        assert!(!manager.is_hot_reload_enabled().await);

        manager.enable_hot_reload().await.unwrap();
        assert!(manager.is_hot_reload_enabled().await);

        manager.disable_hot_reload().await.unwrap();
        assert!(!manager.is_hot_reload_enabled().await);
    }

    #[tokio::test]
    async fn test_reload() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");

        // 初始配置
        tokio::fs::write(&config_path, "value: 1").await.unwrap();

        let manager = ConfigManager::new();
        manager.load_from_file(&config_path).await.unwrap();
        assert_eq!(manager.get::<i32>("value").await, Some(1));

        // 修改配置文件
        tokio::fs::write(&config_path, "value: 2").await.unwrap();

        // 手动重新加载
        manager.reload().await.unwrap();
        assert_eq!(manager.get::<i32>("value").await, Some(2));
    }

    // ------------------------------------------------------------------------
    // 配置验证测试
    // ------------------------------------------------------------------------

    #[tokio::test]
    async fn test_validator() {
        let manager = ConfigManager::new();

        // 添加验证器：worker_count 必须 > 0
        manager
            .add_validator("router.worker_count", |value| {
                if let Some(count) = value.as_u64() {
                    if count > 0 {
                        return Ok(());
                    }
                }
                Err(CoreError::InvalidConfigValue {
                    key: "router.worker_count".to_string(),
                    reason: "必须大于 0".to_string(),
                })
            })
            .await;

        // 有效值
        let valid_config = serde_json::json!({
            "router": {
                "worker_count": 8
            }
        });
        assert!(manager.validate_config(&valid_config).await.is_ok());

        // 无效值
        let invalid_config = serde_json::json!({
            "router": {
                "worker_count": 0
            }
        });
        assert!(manager.validate_config(&invalid_config).await.is_err());
    }

    // ------------------------------------------------------------------------
    // ConfigManagerBuilder 测试
    // ------------------------------------------------------------------------

    #[tokio::test]
    async fn test_config_manager_builder() {
        let temp_dir = TempDir::new().unwrap();
        let user_path = temp_dir.path().join("user.yaml");

        tokio::fs::write(&user_path, "user_key: user_value")
            .await
            .unwrap();

        let default_config = serde_json::json!({
            "default_key": "default_value"
        });

        let manager = ConfigManagerBuilder::new()
            .env_prefix("TEST")
            .default_config(default_config)
            .user_path(&user_path)
            .build()
            .await
            .unwrap();

        assert_eq!(
            manager.get::<String>("default_key").await,
            Some("default_value".to_string())
        );
        assert_eq!(
            manager.get::<String>("user_key").await,
            Some("user_value".to_string())
        );
    }

    // ------------------------------------------------------------------------
    // 边界情况测试
    // ------------------------------------------------------------------------

    #[tokio::test]
    async fn test_empty_path() {
        let manager = ConfigManager::new();
        manager.set("key", "value").await.unwrap();

        // 空路径获取整个配置
        let all: serde_json::Value = manager.get_all().await;
        assert!(all.get("key").is_some());
    }

    #[tokio::test]
    async fn test_special_characters_in_value() {
        let manager = ConfigManager::new();

        // 包含特殊字符的值
        manager
            .set("key", "value with spaces and 中文")
            .await
            .unwrap();
        assert_eq!(
            manager.get::<String>("key").await,
            Some("value with spaces and 中文".to_string())
        );
    }

    #[tokio::test]
    async fn test_complex_nested_structure() {
        let manager = ConfigManager::new();

        manager.set("a.b.c.d.e", 1).await.unwrap();
        manager.set("a.b.c.d.f", 2).await.unwrap();
        manager.set("a.b.g", 3).await.unwrap();

        assert_eq!(manager.get::<i32>("a.b.c.d.e").await, Some(1));
        assert_eq!(manager.get::<i32>("a.b.c.d.f").await, Some(2));
        assert_eq!(manager.get::<i32>("a.b.g").await, Some(3));
    }

    #[tokio::test]
    async fn test_array_config() {
        let manager = ConfigManager::new();

        let arr = vec!["a", "b", "c"];
        manager.set("list", arr).await.unwrap();

        let result: Vec<String> = manager.get("list").await.unwrap();
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let manager = Arc::new(ConfigManager::new());
        let mut handles = vec![];

        // 并发写入
        for i in 0..10 {
            let m = manager.clone();
            handles.push(tokio::spawn(async move {
                m.set(&format!("key{}", i), i).await.unwrap();
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // 验证所有值都正确写入
        for i in 0..10 {
            assert_eq!(
                manager.get::<i32>(&format!("key{}", i)).await,
                Some(i as i32)
            );
        }
    }
}
