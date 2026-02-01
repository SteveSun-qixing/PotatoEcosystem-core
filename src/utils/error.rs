//! 薯片微内核错误类型定义
//!
//! 本模块定义了内核中使用的所有错误类型。

use thiserror::Error;

/// 薯片内核核心错误类型
#[derive(Error, Debug)]
pub enum CoreError {
    // ==================== 路由系统错误 ====================
    
    /// 路由未找到
    #[error("路由未找到: action '{0}'")]
    RouteNotFound(String),

    /// 目标模块未找到
    #[error("目标模块未找到: '{0}'")]
    ModuleNotFound(String),

    /// 请求格式无效
    #[error("请求格式无效: {0}")]
    InvalidRequest(String),

    /// 请求超时
    #[error("请求超时: request_id '{0}'")]
    Timeout(String),

    /// 权限被拒绝
    #[error("权限被拒绝: {0}")]
    PermissionDenied(String),

    /// 路由已存在
    #[error("路由已存在: action '{0}'")]
    RouteAlreadyExists(String),

    /// 队列已满
    #[error("队列已满: {message}")]
    QueueFull {
        max_size: usize,
        message: String,
    },

    // ==================== 模块管理错误 ====================

    /// 模块已加载
    #[error("模块已加载: '{0}'")]
    ModuleAlreadyLoaded(String),

    /// 模块未加载
    #[error("模块未加载: '{0}'")]
    ModuleNotLoaded(String),

    /// 模块加载失败
    #[error("模块加载失败: '{module_id}' - {reason}")]
    ModuleLoadFailed {
        module_id: String,
        reason: String,
    },

    /// 模块卸载失败
    #[error("模块卸载失败: '{module_id}' - {reason}")]
    ModuleUnloadFailed {
        module_id: String,
        reason: String,
    },

    /// 模块有依赖者，无法卸载
    #[error("模块 '{module}' 被以下模块依赖，无法卸载: {dependents:?}")]
    ModuleHasDependents {
        module: String,
        dependents: Vec<String>,
    },

    /// 循环依赖
    #[error("检测到循环依赖: {0}")]
    CircularDependency(String),

    /// 依赖未找到
    #[error("依赖模块未找到: '{0}'")]
    DependencyNotFound(String),

    /// 版本不匹配
    #[error("版本不匹配: 模块 '{module}' 需要版本 {required}, 但找到版本 {found}")]
    VersionMismatch {
        module: String,
        required: String,
        found: String,
    },

    /// 依赖冲突
    #[error("依赖冲突: 模块 '{module}' 的版本要求冲突: {requirements:?}")]
    DependencyConflict {
        module: String,
        requirements: Vec<String>,
    },

    /// 无效的模块元数据
    #[error("无效的模块元数据: {0}")]
    InvalidMetadata(String),

    /// 不支持的运行时
    #[error("不支持的运行时: {0:?}")]
    UnsupportedRuntime(String),

    // ==================== 配置错误 ====================

    /// 配置加载失败
    #[error("配置加载失败: {0}")]
    ConfigLoadFailed(String),

    /// 配置项未找到
    #[error("配置项未找到: '{0}'")]
    ConfigNotFound(String),

    /// 配置值无效
    #[error("配置值无效: '{key}' - {reason}")]
    InvalidConfigValue {
        key: String,
        reason: String,
    },

    // ==================== 事件系统错误 ====================

    /// 事件发布失败
    #[error("事件发布失败: {0}")]
    EventPublishFailed(String),

    /// 订阅未找到
    #[error("订阅未找到: '{0}'")]
    SubscriptionNotFound(String),

    // ==================== IO 和序列化错误 ====================

    /// IO 错误
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    /// JSON 序列化/反序列化错误
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),

    /// YAML 序列化/反序列化错误
    #[error("YAML 错误: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// 版本解析错误
    #[error("版本解析错误: {0}")]
    VersionParse(#[from] semver::Error),

    // ==================== 通用错误 ====================

    /// 内部错误
    #[error("内部错误: {0}")]
    Internal(String),

    /// 初始化失败
    #[error("初始化失败: {0}")]
    InitFailed(String),

    /// 操作被取消
    #[error("操作被取消")]
    Cancelled,

    /// 其他错误
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// 内核操作结果类型别名
pub type Result<T> = std::result::Result<T, CoreError>;

/// 状态码常量
pub mod status_code {
    /// 成功
    pub const OK: u16 = 200;

    /// 已接受（异步处理中）
    pub const ACCEPTED: u16 = 202;

    /// 请求格式错误
    pub const BAD_REQUEST: u16 = 400;

    /// 未授权
    pub const UNAUTHORIZED: u16 = 401;

    /// 权限不足
    pub const FORBIDDEN: u16 = 403;

    /// 未找到
    pub const NOT_FOUND: u16 = 404;

    /// 请求超时
    pub const TIMEOUT: u16 = 408;

    /// 冲突
    pub const CONFLICT: u16 = 409;

    /// 内部错误
    pub const INTERNAL_ERROR: u16 = 500;

    /// 服务不可用
    pub const SERVICE_UNAVAILABLE: u16 = 503;
}

/// 错误码常量
pub mod error_code {
    // 协议错误 (PROTOCOL-xxx)
    pub const PROTOCOL_VERSION_MISMATCH: &str = "PROTOCOL-001";
    pub const PROTOCOL_MESSAGE_FORMAT_ERROR: &str = "PROTOCOL-002";

    // 核心错误 (CORE-xxx)
    pub const CORE_SERVICE_NOT_FOUND: &str = "CORE-001";
    pub const CORE_ROUTE_FAILED: &str = "CORE-002";
    pub const CORE_INIT_FAILED: &str = "CORE-003";

    // 权限错误 (PERMISSION-xxx)
    pub const PERMISSION_DENIED: &str = "PERMISSION-001";
    pub const PERMISSION_NOT_DECLARED: &str = "PERMISSION-002";

    // 模块错误 (MODULE-xxx)
    pub const MODULE_NOT_LOADED: &str = "MODULE-001";
    pub const MODULE_EXECUTION_ERROR: &str = "MODULE-002";
    pub const MODULE_LOAD_FAILED: &str = "MODULE-003";
    pub const MODULE_UNLOAD_FAILED: &str = "MODULE-004";
    pub const MODULE_CIRCULAR_DEPENDENCY: &str = "MODULE-005";

    // 资源错误 (RESOURCE-xxx)
    pub const RESOURCE_NOT_FOUND: &str = "RESOURCE-001";
    pub const RESOURCE_ACCESS_FAILED: &str = "RESOURCE-002";

    // 配置错误 (CONFIG-xxx)
    pub const CONFIG_NOT_FOUND: &str = "CONFIG-001";
    pub const CONFIG_INVALID_VALUE: &str = "CONFIG-002";

    // 超时错误 (TIMEOUT-xxx)
    pub const TIMEOUT_REQUEST: &str = "TIMEOUT-001";
    pub const TIMEOUT_MODULE_LOAD: &str = "TIMEOUT-002";

    // 队列错误 (QUEUE-xxx)
    pub const QUEUE_FULL: &str = "QUEUE-001";
}

impl CoreError {
    /// 获取错误码
    pub fn error_code(&self) -> &'static str {
        match self {
            CoreError::RouteNotFound(_) => error_code::CORE_SERVICE_NOT_FOUND,
            CoreError::ModuleNotFound(_) => error_code::MODULE_NOT_LOADED,
            CoreError::InvalidRequest(_) => error_code::PROTOCOL_MESSAGE_FORMAT_ERROR,
            CoreError::Timeout(_) => error_code::TIMEOUT_REQUEST,
            CoreError::PermissionDenied(_) => error_code::PERMISSION_DENIED,
            CoreError::ModuleLoadFailed { .. } => error_code::MODULE_LOAD_FAILED,
            CoreError::ModuleUnloadFailed { .. } => error_code::MODULE_UNLOAD_FAILED,
            CoreError::CircularDependency(_) => error_code::MODULE_CIRCULAR_DEPENDENCY,
            CoreError::ConfigNotFound(_) => error_code::CONFIG_NOT_FOUND,
            CoreError::InvalidConfigValue { .. } => error_code::CONFIG_INVALID_VALUE,
            CoreError::QueueFull { .. } => error_code::QUEUE_FULL,
            _ => "UNKNOWN",
        }
    }

    /// 获取 HTTP 状态码
    pub fn status_code(&self) -> u16 {
        match self {
            CoreError::RouteNotFound(_) => status_code::NOT_FOUND,
            CoreError::ModuleNotFound(_) => status_code::NOT_FOUND,
            CoreError::InvalidRequest(_) => status_code::BAD_REQUEST,
            CoreError::Timeout(_) => status_code::TIMEOUT,
            CoreError::PermissionDenied(_) => status_code::FORBIDDEN,
            CoreError::ModuleAlreadyLoaded(_) => status_code::CONFLICT,
            CoreError::RouteAlreadyExists(_) => status_code::CONFLICT,
            CoreError::ConfigNotFound(_) => status_code::NOT_FOUND,
            CoreError::QueueFull { .. } => status_code::SERVICE_UNAVAILABLE,
            _ => status_code::INTERNAL_ERROR,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = CoreError::RouteNotFound("test.action".to_string());
        assert!(err.to_string().contains("test.action"));
    }

    #[test]
    fn test_error_code() {
        let err = CoreError::RouteNotFound("test".to_string());
        assert_eq!(err.error_code(), error_code::CORE_SERVICE_NOT_FOUND);
    }

    #[test]
    fn test_status_code() {
        let err = CoreError::RouteNotFound("test".to_string());
        assert_eq!(err.status_code(), status_code::NOT_FOUND);

        let err = CoreError::Timeout("req-123".to_string());
        assert_eq!(err.status_code(), status_code::TIMEOUT);
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let core_err: CoreError = io_err.into();
        assert!(matches!(core_err, CoreError::Io(_)));
    }
}
