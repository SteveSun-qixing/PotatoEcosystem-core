//! 内核配置
//!
//! 定义内核的配置结构和加载逻辑。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    pub async fn from_file(path: impl Into<PathBuf>) -> crate::utils::Result<Self> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let override_config = CoreConfig::builder()
            .log_level("debug")
            .dev_mode()
            .build();

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
}
