//! 日志系统模块
//!
//! 本模块基于 tracing 生态实现完整的日志系统功能，包括：
//!
//! - 多级别日志支持（TRACE, DEBUG, INFO, WARN, ERROR）
//! - 结构化日志（JSON 格式输出）
//! - 文件日志输出（异步非阻塞）
//! - 日志轮转（按时间轮转：每天、每小时）
//! - 日志过滤（按模块、按级别）
//!
//! # 示例
//!
//! ```rust,no_run
//! use chips_core::utils::logger::{Logger, LoggerConfig, RotationStrategy};
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // 基础初始化（仅控制台输出）
//!     let _guard = Logger::init(LoggerConfig::default())?;
//!     
//!     // 完整配置初始化
//!     let config = LoggerConfig::builder()
//!         .level("debug")
//!         .json_format(true)
//!         .file_output(PathBuf::from("./logs"))
//!         .rotation(RotationStrategy::Daily)
//!         .max_files(7)
//!         .build();
//!     
//!     let _guard = Logger::init(config)?;
//!     
//!     tracing::info!(
//!         request_id = "req-123",
//!         sender = "editor",
//!         "Processing request"
//!     );
//!     
//!     Ok(())
//! }
//! ```

use crate::utils::{CoreError, Result};
use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;
use tracing::Level;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

// ============================================================================
// 日志轮转策略
// ============================================================================

/// 日志轮转策略
///
/// 定义日志文件的轮转方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RotationStrategy {
    /// 不轮转（单个日志文件）
    Never,
    /// 每分钟轮转（主要用于测试）
    Minutely,
    /// 每小时轮转
    Hourly,
    /// 每天轮转（默认）
    #[default]
    Daily,
}

impl RotationStrategy {
    /// 转换为 tracing-appender 的 Rotation 类型
    fn to_rotation(self) -> Rotation {
        match self {
            RotationStrategy::Never => Rotation::NEVER,
            RotationStrategy::Minutely => Rotation::MINUTELY,
            RotationStrategy::Hourly => Rotation::HOURLY,
            RotationStrategy::Daily => Rotation::DAILY,
        }
    }

    /// 从字符串解析轮转策略
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "never" | "none" => RotationStrategy::Never,
            "minutely" | "minute" => RotationStrategy::Minutely,
            "hourly" | "hour" => RotationStrategy::Hourly,
            "daily" | "day" => RotationStrategy::Daily,
            _ => RotationStrategy::Daily,
        }
    }
}

impl std::fmt::Display for RotationStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RotationStrategy::Never => write!(f, "never"),
            RotationStrategy::Minutely => write!(f, "minutely"),
            RotationStrategy::Hourly => write!(f, "hourly"),
            RotationStrategy::Daily => write!(f, "daily"),
        }
    }
}

// ============================================================================
// 日志配置
// ============================================================================

/// 日志系统配置
///
/// 用于配置日志系统的各项参数
#[derive(Debug, Clone)]
pub struct LoggerConfig {
    /// 默认日志级别（例如 "trace", "debug", "info", "warn", "error"）
    pub level: String,

    /// 是否使用 JSON 格式输出
    pub json_format: bool,

    /// 是否输出到控制台
    pub console_output: bool,

    /// 文件输出目录（None 表示不输出到文件）
    pub file_output: Option<PathBuf>,

    /// 日志文件名前缀
    pub file_prefix: String,

    /// 日志轮转策略
    pub rotation: RotationStrategy,

    /// 保留的最大日志文件数（仅供参考，tracing-appender 不直接支持清理）
    pub max_files: usize,

    /// 是否显示目标模块
    pub show_target: bool,

    /// 是否显示线程 ID
    pub show_thread_ids: bool,

    /// 是否显示文件名和行号
    pub show_file_line: bool,

    /// 是否显示时间戳
    pub show_timestamp: bool,

    /// 是否显示日志级别
    pub show_level: bool,

    /// 自定义过滤指令（EnvFilter 格式）
    /// 例如："chips_core=debug,chips_core::router=trace"
    pub filter_directives: Option<String>,

    /// 是否启用 ANSI 颜色（控制台输出）
    pub ansi_colors: bool,

    /// Span 事件配置
    pub span_events: SpanEvents,
}

/// Span 事件配置
#[derive(Debug, Clone, Copy, Default)]
pub struct SpanEvents {
    /// 记录 span 进入事件
    pub enter: bool,
    /// 记录 span 退出事件
    pub exit: bool,
    /// 记录 span 关闭事件
    pub close: bool,
}

impl SpanEvents {
    /// 转换为 FmtSpan
    fn to_fmt_span(self) -> FmtSpan {
        let mut span = FmtSpan::NONE;
        if self.enter {
            span |= FmtSpan::ENTER;
        }
        if self.exit {
            span |= FmtSpan::EXIT;
        }
        if self.close {
            span |= FmtSpan::CLOSE;
        }
        span
    }
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            json_format: false,
            console_output: true,
            file_output: None,
            file_prefix: "chips-core".to_string(),
            rotation: RotationStrategy::Daily,
            max_files: 7,
            show_target: true,
            show_thread_ids: false,
            show_file_line: true,
            show_timestamp: true,
            show_level: true,
            filter_directives: None,
            ansi_colors: true,
            span_events: SpanEvents::default(),
        }
    }
}

impl LoggerConfig {
    /// 创建配置构建器
    pub fn builder() -> LoggerConfigBuilder {
        LoggerConfigBuilder::new()
    }

    /// 从 CoreConfig 的 LogConfig 创建
    pub fn from_log_config(log_config: &crate::core::config::LogConfig) -> Self {
        Self {
            level: log_config.level.clone(),
            json_format: log_config.json_format,
            console_output: true,
            file_output: if log_config.file_output {
                log_config.log_dir.clone()
            } else {
                None
            },
            file_prefix: "chips-core".to_string(),
            rotation: RotationStrategy::from_str(&log_config.rotation),
            max_files: log_config.max_files,
            ..Default::default()
        }
    }

    /// 解析日志级别字符串
    #[allow(dead_code)]
    fn parse_level(&self) -> Level {
        match self.level.to_lowercase().as_str() {
            "trace" => Level::TRACE,
            "debug" => Level::DEBUG,
            "info" => Level::INFO,
            "warn" | "warning" => Level::WARN,
            "error" => Level::ERROR,
            _ => Level::INFO,
        }
    }
}

/// 日志配置构建器
#[derive(Debug, Default)]
pub struct LoggerConfigBuilder {
    config: LoggerConfig,
}

impl LoggerConfigBuilder {
    /// 创建新的构建器
    pub fn new() -> Self {
        Self {
            config: LoggerConfig::default(),
        }
    }

    /// 设置日志级别
    pub fn level(mut self, level: impl Into<String>) -> Self {
        self.config.level = level.into();
        self
    }

    /// 启用 JSON 格式输出
    pub fn json_format(mut self, enable: bool) -> Self {
        self.config.json_format = enable;
        self
    }

    /// 设置控制台输出
    pub fn console_output(mut self, enable: bool) -> Self {
        self.config.console_output = enable;
        self
    }

    /// 设置文件输出目录
    pub fn file_output(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config.file_output = Some(dir.into());
        self
    }

    /// 设置日志文件前缀
    pub fn file_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.config.file_prefix = prefix.into();
        self
    }

    /// 设置轮转策略
    pub fn rotation(mut self, strategy: RotationStrategy) -> Self {
        self.config.rotation = strategy;
        self
    }

    /// 设置保留文件数
    pub fn max_files(mut self, count: usize) -> Self {
        self.config.max_files = count;
        self
    }

    /// 显示目标模块
    pub fn show_target(mut self, enable: bool) -> Self {
        self.config.show_target = enable;
        self
    }

    /// 显示线程 ID
    pub fn show_thread_ids(mut self, enable: bool) -> Self {
        self.config.show_thread_ids = enable;
        self
    }

    /// 显示文件名和行号
    pub fn show_file_line(mut self, enable: bool) -> Self {
        self.config.show_file_line = enable;
        self
    }

    /// 显示时间戳
    pub fn show_timestamp(mut self, enable: bool) -> Self {
        self.config.show_timestamp = enable;
        self
    }

    /// 显示日志级别
    pub fn show_level(mut self, enable: bool) -> Self {
        self.config.show_level = enable;
        self
    }

    /// 设置过滤指令
    pub fn filter_directives(mut self, directives: impl Into<String>) -> Self {
        self.config.filter_directives = Some(directives.into());
        self
    }

    /// 启用 ANSI 颜色
    pub fn ansi_colors(mut self, enable: bool) -> Self {
        self.config.ansi_colors = enable;
        self
    }

    /// 设置 Span 事件配置
    pub fn span_events(mut self, events: SpanEvents) -> Self {
        self.config.span_events = events;
        self
    }

    /// 构建配置
    pub fn build(self) -> LoggerConfig {
        self.config
    }
}

// ============================================================================
// 日志守卫
// ============================================================================

/// 日志系统守卫
///
/// 持有非阻塞写入器的 WorkerGuard，确保在程序退出前完成日志写入。
/// 当此守卫被丢弃时，会等待所有挂起的日志写入完成。
pub struct LogGuard {
    /// 控制台输出守卫
    _console_guard: Option<WorkerGuard>,
    /// 文件输出守卫
    _file_guard: Option<WorkerGuard>,
}

impl LogGuard {
    /// 创建空守卫
    fn empty() -> Self {
        Self {
            _console_guard: None,
            _file_guard: None,
        }
    }

    /// 设置控制台守卫
    fn with_console_guard(mut self, guard: WorkerGuard) -> Self {
        self._console_guard = Some(guard);
        self
    }

    /// 设置文件守卫
    fn with_file_guard(mut self, guard: WorkerGuard) -> Self {
        self._file_guard = Some(guard);
        self
    }
}

// ============================================================================
// 日志系统
// ============================================================================

/// 全局日志初始化状态
static LOGGER_INITIALIZED: OnceLock<bool> = OnceLock::new();

/// 日志系统
///
/// 提供日志系统的初始化和管理功能
pub struct Logger;

impl Logger {
    /// 初始化日志系统
    ///
    /// 根据配置初始化 tracing-subscriber，支持控制台和文件输出。
    ///
    /// # Arguments
    ///
    /// * `config` - 日志配置
    ///
    /// # Returns
    ///
    /// 返回 `LogGuard`，必须保持活动状态直到程序退出
    ///
    /// # Errors
    ///
    /// 如果日志系统已初始化或配置无效，返回错误
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::utils::logger::{Logger, LoggerConfig};
    ///
    /// let _guard = Logger::init(LoggerConfig::default()).unwrap();
    /// tracing::info!("Logger initialized");
    /// ```
    pub fn init(config: LoggerConfig) -> Result<LogGuard> {
        // 检查是否已初始化
        if LOGGER_INITIALIZED.get().is_some() {
            return Err(CoreError::InitFailed(
                "日志系统已初始化，不能重复初始化".to_string(),
            ));
        }

        // 创建 EnvFilter
        let env_filter = Self::create_env_filter(&config)?;

        // 根据配置创建不同的层组合
        let guard = if config.json_format {
            Self::init_json_logger(config, env_filter)?
        } else {
            Self::init_pretty_logger(config, env_filter)?
        };

        // 标记已初始化
        let _ = LOGGER_INITIALIZED.set(true);

        Ok(guard)
    }

    /// 尝试初始化日志系统（不会失败）
    ///
    /// 如果日志系统已初始化，返回空守卫而不是错误。
    /// 适用于测试或多次调用初始化的场景。
    ///
    /// # Arguments
    ///
    /// * `config` - 日志配置
    ///
    /// # Returns
    ///
    /// 返回 `LogGuard`
    pub fn try_init(config: LoggerConfig) -> LogGuard {
        Self::init(config).unwrap_or_else(|_| LogGuard::empty())
    }

    /// 使用默认配置初始化日志系统
    ///
    /// # Returns
    ///
    /// 返回 `LogGuard`
    pub fn init_default() -> Result<LogGuard> {
        Self::init(LoggerConfig::default())
    }

    /// 创建 EnvFilter
    fn create_env_filter(config: &LoggerConfig) -> Result<EnvFilter> {
        // 优先使用环境变量 RUST_LOG
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            // 使用配置中的级别
            let base_filter = format!("{}", config.level);
            EnvFilter::new(&base_filter)
        });

        // 添加自定义过滤指令
        let filter = if let Some(ref directives) = config.filter_directives {
            directives.split(',').fold(filter, |f, directive| {
                f.add_directive(
                    directive
                        .trim()
                        .parse()
                        .unwrap_or_else(|_| config.level.parse().unwrap_or(Level::INFO.into())),
                )
            })
        } else {
            filter
        };

        Ok(filter)
    }

    /// 初始化 JSON 格式日志
    fn init_json_logger(config: LoggerConfig, env_filter: EnvFilter) -> Result<LogGuard> {
        let mut guard = LogGuard::empty();

        // 控制台 JSON 层
        let console_layer = if config.console_output {
            let (non_blocking, console_guard) = tracing_appender::non_blocking(io::stdout());
            guard = guard.with_console_guard(console_guard);

            Some(
                fmt::layer()
                    .json()
                    .with_writer(non_blocking)
                    .with_target(config.show_target)
                    .with_thread_ids(config.show_thread_ids)
                    .with_file(config.show_file_line)
                    .with_line_number(config.show_file_line)
                    .with_span_events(config.span_events.to_fmt_span())
                    .with_ansi(false), // JSON 格式不使用 ANSI 颜色
            )
        } else {
            None
        };

        // 文件 JSON 层
        let file_layer = if let Some(ref log_dir) = config.file_output {
            let file_appender = RollingFileAppender::new(
                config.rotation.to_rotation(),
                log_dir,
                &format!("{}.log", config.file_prefix),
            );

            let (non_blocking, file_guard) = tracing_appender::non_blocking(file_appender);
            guard = guard.with_file_guard(file_guard);

            Some(
                fmt::layer()
                    .json()
                    .with_writer(non_blocking)
                    .with_target(config.show_target)
                    .with_thread_ids(config.show_thread_ids)
                    .with_file(config.show_file_line)
                    .with_line_number(config.show_file_line)
                    .with_span_events(config.span_events.to_fmt_span())
                    .with_ansi(false),
            )
        } else {
            None
        };

        // 注册订阅者
        tracing_subscriber::registry()
            .with(env_filter)
            .with(console_layer)
            .with(file_layer)
            .try_init()
            .map_err(|e| CoreError::InitFailed(format!("日志系统初始化失败: {}", e)))?;

        Ok(guard)
    }

    /// 初始化 Pretty 格式日志
    fn init_pretty_logger(config: LoggerConfig, env_filter: EnvFilter) -> Result<LogGuard> {
        let mut guard = LogGuard::empty();

        // 控制台 Pretty 层
        let console_layer = if config.console_output {
            let (non_blocking, console_guard) = tracing_appender::non_blocking(io::stdout());
            guard = guard.with_console_guard(console_guard);

            Some(
                fmt::layer()
                    .with_writer(non_blocking)
                    .with_target(config.show_target)
                    .with_thread_ids(config.show_thread_ids)
                    .with_file(config.show_file_line)
                    .with_line_number(config.show_file_line)
                    .with_level(config.show_level)
                    .with_span_events(config.span_events.to_fmt_span())
                    .with_ansi(config.ansi_colors),
            )
        } else {
            None
        };

        // 文件 Pretty 层（文件不使用 ANSI 颜色）
        let file_layer = if let Some(ref log_dir) = config.file_output {
            let file_appender = RollingFileAppender::new(
                config.rotation.to_rotation(),
                log_dir,
                &format!("{}.log", config.file_prefix),
            );

            let (non_blocking, file_guard) = tracing_appender::non_blocking(file_appender);
            guard = guard.with_file_guard(file_guard);

            Some(
                fmt::layer()
                    .with_writer(non_blocking)
                    .with_target(config.show_target)
                    .with_thread_ids(config.show_thread_ids)
                    .with_file(config.show_file_line)
                    .with_line_number(config.show_file_line)
                    .with_level(config.show_level)
                    .with_span_events(config.span_events.to_fmt_span())
                    .with_ansi(false), // 文件不使用 ANSI
            )
        } else {
            None
        };

        // 注册订阅者
        tracing_subscriber::registry()
            .with(env_filter)
            .with(console_layer)
            .with(file_layer)
            .try_init()
            .map_err(|e| CoreError::InitFailed(format!("日志系统初始化失败: {}", e)))?;

        Ok(guard)
    }
}

// ============================================================================
// 结构化日志字段定义
// ============================================================================

/// 标准日志字段名称
///
/// 提供统一的日志字段命名，便于日志分析和查询
pub mod fields {
    /// 请求 ID 字段
    pub const REQUEST_ID: &str = "request_id";
    /// 发送方字段
    pub const SENDER: &str = "sender";
    /// 接收方字段
    pub const RECEIVER: &str = "receiver";
    /// 动作字段
    pub const ACTION: &str = "action";
    /// 模块 ID 字段
    pub const MODULE_ID: &str = "module_id";
    /// 错误码字段
    pub const ERROR_CODE: &str = "error_code";
    /// 错误消息字段
    pub const ERROR_MSG: &str = "error_msg";
    /// 耗时字段（微秒）
    pub const DURATION_US: &str = "duration_us";
    /// 耗时字段（毫秒）
    pub const DURATION_MS: &str = "duration_ms";
    /// 状态字段
    pub const STATUS: &str = "status";
    /// 结果字段
    pub const RESULT: &str = "result";
    /// 事件类型字段
    pub const EVENT_TYPE: &str = "event_type";
    /// 事件 ID 字段
    pub const EVENT_ID: &str = "event_id";
    /// 优先级字段
    pub const PRIORITY: &str = "priority";
    /// 版本字段
    pub const VERSION: &str = "version";
    /// 协议版本字段
    pub const PROTOCOL_VERSION: &str = "protocol_version";
    /// 负载大小字段
    pub const PAYLOAD_SIZE: &str = "payload_size";
    /// 队列长度字段
    pub const QUEUE_LEN: &str = "queue_len";
    /// 并发数字段
    pub const CONCURRENT: &str = "concurrent";
}

// ============================================================================
// 便捷宏
// ============================================================================

/// 创建带请求上下文的 span
///
/// # Example
///
/// ```rust,ignore
/// use chips_core::request_span;
///
/// let span = request_span!("req-123", "editor", "file.open");
/// let _enter = span.enter();
/// ```
#[macro_export]
macro_rules! request_span {
    ($request_id:expr, $sender:expr, $action:expr) => {
        tracing::info_span!(
            "request",
            request_id = %$request_id,
            sender = %$sender,
            action = %$action
        )
    };
}

/// 记录请求开始
#[macro_export]
macro_rules! log_request_start {
    ($request_id:expr, $sender:expr, $action:expr) => {
        tracing::info!(
            request_id = %$request_id,
            sender = %$sender,
            action = %$action,
            "Request started"
        )
    };
}

/// 记录请求成功完成
#[macro_export]
macro_rules! log_request_success {
    ($request_id:expr, $duration_us:expr) => {
        tracing::info!(
            request_id = %$request_id,
            duration_us = $duration_us,
            status = "success",
            "Request completed"
        )
    };
}

/// 记录请求失败
#[macro_export]
macro_rules! log_request_error {
    ($request_id:expr, $error_code:expr, $error_msg:expr) => {
        tracing::error!(
            request_id = %$request_id,
            error_code = %$error_code,
            error_msg = %$error_msg,
            status = "error",
            "Request failed"
        )
    };
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // RotationStrategy 测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_rotation_strategy_default() {
        let strategy = RotationStrategy::default();
        assert_eq!(strategy, RotationStrategy::Daily);
    }

    #[test]
    fn test_rotation_strategy_from_str() {
        assert_eq!(RotationStrategy::from_str("daily"), RotationStrategy::Daily);
        assert_eq!(RotationStrategy::from_str("DAILY"), RotationStrategy::Daily);
        assert_eq!(RotationStrategy::from_str("day"), RotationStrategy::Daily);
        assert_eq!(
            RotationStrategy::from_str("hourly"),
            RotationStrategy::Hourly
        );
        assert_eq!(RotationStrategy::from_str("hour"), RotationStrategy::Hourly);
        assert_eq!(
            RotationStrategy::from_str("minutely"),
            RotationStrategy::Minutely
        );
        assert_eq!(RotationStrategy::from_str("never"), RotationStrategy::Never);
        assert_eq!(RotationStrategy::from_str("none"), RotationStrategy::Never);
        // 无效值返回默认值
        assert_eq!(
            RotationStrategy::from_str("invalid"),
            RotationStrategy::Daily
        );
    }

    #[test]
    fn test_rotation_strategy_display() {
        assert_eq!(format!("{}", RotationStrategy::Never), "never");
        assert_eq!(format!("{}", RotationStrategy::Minutely), "minutely");
        assert_eq!(format!("{}", RotationStrategy::Hourly), "hourly");
        assert_eq!(format!("{}", RotationStrategy::Daily), "daily");
    }

    #[test]
    fn test_rotation_to_tracing_rotation() {
        // 验证转换不会 panic
        let _ = RotationStrategy::Never.to_rotation();
        let _ = RotationStrategy::Minutely.to_rotation();
        let _ = RotationStrategy::Hourly.to_rotation();
        let _ = RotationStrategy::Daily.to_rotation();
    }

    // ------------------------------------------------------------------------
    // LoggerConfig 测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_logger_config_default() {
        let config = LoggerConfig::default();
        assert_eq!(config.level, "info");
        assert!(!config.json_format);
        assert!(config.console_output);
        assert!(config.file_output.is_none());
        assert_eq!(config.file_prefix, "chips-core");
        assert_eq!(config.rotation, RotationStrategy::Daily);
        assert_eq!(config.max_files, 7);
        assert!(config.show_target);
        assert!(!config.show_thread_ids);
        assert!(config.show_file_line);
        assert!(config.show_timestamp);
        assert!(config.show_level);
        assert!(config.ansi_colors);
    }

    #[test]
    fn test_logger_config_builder() {
        let config = LoggerConfig::builder()
            .level("debug")
            .json_format(true)
            .console_output(true)
            .file_output("/var/log/chips")
            .file_prefix("myapp")
            .rotation(RotationStrategy::Hourly)
            .max_files(14)
            .show_target(false)
            .show_thread_ids(true)
            .show_file_line(false)
            .show_timestamp(true)
            .show_level(true)
            .filter_directives("chips_core=trace")
            .ansi_colors(false)
            .build();

        assert_eq!(config.level, "debug");
        assert!(config.json_format);
        assert!(config.console_output);
        assert_eq!(
            config.file_output,
            Some(PathBuf::from("/var/log/chips"))
        );
        assert_eq!(config.file_prefix, "myapp");
        assert_eq!(config.rotation, RotationStrategy::Hourly);
        assert_eq!(config.max_files, 14);
        assert!(!config.show_target);
        assert!(config.show_thread_ids);
        assert!(!config.show_file_line);
        assert!(config.show_timestamp);
        assert!(config.show_level);
        assert_eq!(
            config.filter_directives,
            Some("chips_core=trace".to_string())
        );
        assert!(!config.ansi_colors);
    }

    #[test]
    fn test_logger_config_parse_level() {
        let test_cases = vec![
            ("trace", Level::TRACE),
            ("TRACE", Level::TRACE),
            ("debug", Level::DEBUG),
            ("DEBUG", Level::DEBUG),
            ("info", Level::INFO),
            ("INFO", Level::INFO),
            ("warn", Level::WARN),
            ("WARN", Level::WARN),
            ("warning", Level::WARN),
            ("error", Level::ERROR),
            ("ERROR", Level::ERROR),
            ("invalid", Level::INFO), // 默认值
        ];

        for (level_str, expected) in test_cases {
            let config = LoggerConfig::builder().level(level_str).build();
            assert_eq!(
                config.parse_level(),
                expected,
                "Failed for level: {}",
                level_str
            );
        }
    }

    #[test]
    fn test_logger_config_from_log_config() {
        use crate::core::config::LogConfig;

        let log_config = LogConfig {
            level: "debug".to_string(),
            file_output: true,
            log_dir: Some(PathBuf::from("/var/log")),
            json_format: true,
            rotation: "hourly".to_string(),
            max_files: 14,
        };

        let logger_config = LoggerConfig::from_log_config(&log_config);

        assert_eq!(logger_config.level, "debug");
        assert!(logger_config.json_format);
        assert_eq!(logger_config.file_output, Some(PathBuf::from("/var/log")));
        assert_eq!(logger_config.rotation, RotationStrategy::Hourly);
        assert_eq!(logger_config.max_files, 14);
    }

    #[test]
    fn test_logger_config_from_log_config_no_file() {
        use crate::core::config::LogConfig;

        let log_config = LogConfig {
            level: "info".to_string(),
            file_output: false,
            log_dir: Some(PathBuf::from("/var/log")), // 即使有目录，file_output=false 也不输出
            json_format: false,
            rotation: "daily".to_string(),
            max_files: 7,
        };

        let logger_config = LoggerConfig::from_log_config(&log_config);

        assert!(logger_config.file_output.is_none());
    }

    // ------------------------------------------------------------------------
    // SpanEvents 测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_span_events_default() {
        let events = SpanEvents::default();
        assert!(!events.enter);
        assert!(!events.exit);
        assert!(!events.close);
    }

    #[test]
    fn test_span_events_to_fmt_span() {
        // 测试不同的 SpanEvents 配置能够正确转换为 FmtSpan
        // 由于 FmtSpan 的 contains 方法是私有的，我们只能验证转换不会 panic
        let events = SpanEvents {
            enter: true,
            exit: false,
            close: false,
        };
        let _fmt_span = events.to_fmt_span();

        let events_all = SpanEvents {
            enter: true,
            exit: true,
            close: true,
        };
        let _fmt_span_all = events_all.to_fmt_span();

        let events_none = SpanEvents {
            enter: false,
            exit: false,
            close: false,
        };
        let _fmt_span_none = events_none.to_fmt_span();
    }

    // ------------------------------------------------------------------------
    // LogGuard 测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_log_guard_empty() {
        let guard = LogGuard::empty();
        assert!(guard._console_guard.is_none());
        assert!(guard._file_guard.is_none());
    }

    // ------------------------------------------------------------------------
    // fields 模块测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_field_constants() {
        // 验证所有字段常量存在且为预期值
        assert_eq!(fields::REQUEST_ID, "request_id");
        assert_eq!(fields::SENDER, "sender");
        assert_eq!(fields::RECEIVER, "receiver");
        assert_eq!(fields::ACTION, "action");
        assert_eq!(fields::MODULE_ID, "module_id");
        assert_eq!(fields::ERROR_CODE, "error_code");
        assert_eq!(fields::ERROR_MSG, "error_msg");
        assert_eq!(fields::DURATION_US, "duration_us");
        assert_eq!(fields::DURATION_MS, "duration_ms");
        assert_eq!(fields::STATUS, "status");
        assert_eq!(fields::RESULT, "result");
        assert_eq!(fields::EVENT_TYPE, "event_type");
        assert_eq!(fields::EVENT_ID, "event_id");
        assert_eq!(fields::PRIORITY, "priority");
        assert_eq!(fields::VERSION, "version");
        assert_eq!(fields::PROTOCOL_VERSION, "protocol_version");
        assert_eq!(fields::PAYLOAD_SIZE, "payload_size");
        assert_eq!(fields::QUEUE_LEN, "queue_len");
        assert_eq!(fields::CONCURRENT, "concurrent");
    }

    // ------------------------------------------------------------------------
    // EnvFilter 创建测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_create_env_filter_default() {
        let config = LoggerConfig::default();
        let filter = Logger::create_env_filter(&config);
        assert!(filter.is_ok());
    }

    #[test]
    fn test_create_env_filter_with_directives() {
        let config = LoggerConfig::builder()
            .level("info")
            .filter_directives("chips_core=debug,chips_core::router=trace")
            .build();

        let filter = Logger::create_env_filter(&config);
        assert!(filter.is_ok());
    }

    // ------------------------------------------------------------------------
    // Logger 初始化测试
    // 注意：由于全局状态，这些测试需要单独运行或使用 serial_test
    // ------------------------------------------------------------------------

    // 这个测试会影响其他测试，因为它会设置全局状态
    // 在实际使用中，日志系统只应初始化一次
    // #[test]
    // fn test_logger_init_once() {
    //     let result = Logger::init(LoggerConfig::default());
    //     // 第一次应该成功或已初始化
    //     // 注意：如果其他测试先运行，这里可能已经初始化了
    // }

    #[test]
    fn test_logger_try_init_never_fails() {
        // try_init 永远不会 panic
        let _guard = Logger::try_init(LoggerConfig::default());
        // 即使已初始化，也不会失败
        let _guard2 = Logger::try_init(LoggerConfig::default());
    }
}
