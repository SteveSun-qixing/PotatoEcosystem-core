//! 模块元数据定义
//!
//! 定义模块描述文件 (module.yaml) 中的所有数据结构。

use chrono::{DateTime, Utc};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// 模块类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModuleType {
    /// 原子模块 - 最基础的功能单元
    Atomic,
    /// 复合模块 - 由多个原子模块组成
    Composite,
    /// 插件 - 可热插拔的功能扩展
    Plugin,
    /// 应用 - 完整的应用程序
    Application,
    /// 服务 - 后台服务
    Service,
}

impl Default for ModuleType {
    fn default() -> Self {
        ModuleType::Atomic
    }
}

/// 运行时类型
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    /// Rust 原生模块
    Rust,
    /// JavaScript/TypeScript
    #[serde(alias = "js")]
    JavaScript,
    /// Python
    Python,
    /// WebAssembly
    #[serde(alias = "wasm")]
    WebAssembly,
}

impl Default for Runtime {
    fn default() -> Self {
        Runtime::Rust
    }
}

/// 入口点配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPoint {
    /// 运行时类型
    pub runtime: Runtime,
    
    /// 入口文件路径
    pub path: String,
    
    /// 函数名（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
}

impl EntryPoint {
    pub fn rust(path: impl Into<String>) -> Self {
        Self {
            runtime: Runtime::Rust,
            path: path.into(),
            function: None,
        }
    }

    pub fn javascript(path: impl Into<String>) -> Self {
        Self {
            runtime: Runtime::JavaScript,
            path: path.into(),
            function: None,
        }
    }
}

/// 依赖声明
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    /// 依赖模块 ID
    pub module_id: String,
    
    /// 版本要求（semver 格式）
    pub version: String,
    
    /// 是否必须
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_required() -> bool {
    true
}

impl Dependency {
    pub fn new(module_id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            module_id: module_id.into(),
            version: version.into(),
            required: true,
        }
    }

    /// 设置为可选依赖
    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    /// 检查版本是否满足要求
    pub fn version_matches(&self, version: &Version) -> bool {
        if let Ok(req) = VersionReq::parse(&self.version) {
            req.matches(version)
        } else {
            false
        }
    }
}

/// 接口声明
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModuleInterfaces {
    /// 提供的接口（action 列表）
    #[serde(default)]
    pub provides: Vec<String>,
    
    /// 需要的接口（可选）
    #[serde(default)]
    pub requires: Vec<String>,
}

/// 权限声明
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModulePermissions {
    /// 请求的权限列表
    #[serde(default)]
    pub requested: Vec<String>,
    
    /// 授予的权限列表（由内核管理）
    #[serde(skip)]
    pub granted: Vec<String>,
}

/// 兼容性声明
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Compatibility {
    /// 最低协议版本
    #[serde(default = "default_protocol_version")]
    pub protocol_version: String,
    
    /// 支持的平台列表
    #[serde(default)]
    pub platforms: Vec<String>,
    
    /// 最低内核版本
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_core_version: Option<String>,
}

fn default_protocol_version() -> String {
    "1.0".to_string()
}

impl Default for Compatibility {
    fn default() -> Self {
        Self {
            protocol_version: "1.0".to_string(),
            platforms: vec![],
            min_core_version: None,
        }
    }
}

/// 模块元数据
///
/// 对应 module.yaml 文件中的配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleMetadata {
    /// 模块唯一标识
    pub id: String,
    
    /// 模块显示名称
    pub name: String,
    
    /// 模块版本（semver 格式）
    pub version: String,
    
    /// 模块类型
    #[serde(default)]
    pub module_type: ModuleType,
    
    /// 模块描述
    #[serde(default)]
    pub description: String,
    
    /// 作者信息
    #[serde(default)]
    pub author: String,
    
    /// 许可证
    #[serde(default)]
    pub license: String,
    
    /// 入口点配置
    pub entry: EntryPoint,
    
    /// 依赖声明
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    
    /// 接口声明
    #[serde(default)]
    pub interfaces: ModuleInterfaces,
    
    /// 权限声明
    #[serde(default)]
    pub permissions: ModulePermissions,
    
    /// 兼容性声明
    #[serde(default)]
    pub compatibility: Compatibility,
    
    /// 配置模式定义
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<serde_json::Value>,
    
    /// 自定义字段
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl ModuleMetadata {
    /// 创建新的模块元数据
    pub fn new(id: impl Into<String>, name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            module_type: ModuleType::Atomic,
            description: String::new(),
            author: String::new(),
            license: String::new(),
            entry: EntryPoint::rust(""),
            dependencies: vec![],
            interfaces: ModuleInterfaces::default(),
            permissions: ModulePermissions::default(),
            compatibility: Compatibility::default(),
            config_schema: None,
            extra: HashMap::new(),
        }
    }

    /// 解析版本号
    pub fn parsed_version(&self) -> Option<Version> {
        Version::parse(&self.version).ok()
    }

    /// 验证元数据有效性
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = vec![];

        if self.id.is_empty() {
            errors.push("模块 ID 不能为空".to_string());
        }

        if self.name.is_empty() {
            errors.push("模块名称不能为空".to_string());
        }

        if Version::parse(&self.version).is_err() {
            errors.push(format!("无效的版本号格式: {}", self.version));
        }

        if self.entry.path.is_empty() {
            errors.push("入口文件路径不能为空".to_string());
        }

        // 验证依赖版本格式
        for dep in &self.dependencies {
            if VersionReq::parse(&dep.version).is_err() {
                errors.push(format!("依赖 {} 的版本要求格式无效: {}", dep.module_id, dep.version));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// 模块状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleState {
    /// 已注册但未加载
    Registered,
    /// 正在加载
    Loading,
    /// 已加载（已初始化）
    Loaded,
    /// 正在启动
    Starting,
    /// 运行中
    Running,
    /// 正在停止
    Stopping,
    /// 已停止
    Stopped,
    /// 正在卸载
    Unloading,
    /// 已卸载
    Unloaded,
    /// 错误状态
    Error,
}

impl Default for ModuleState {
    fn default() -> Self {
        ModuleState::Registered
    }
}

impl ModuleState {
    /// 是否可以加载
    pub fn can_load(&self) -> bool {
        matches!(self, ModuleState::Registered | ModuleState::Unloaded)
    }

    /// 是否可以启动
    pub fn can_start(&self) -> bool {
        matches!(self, ModuleState::Loaded | ModuleState::Stopped)
    }

    /// 是否可以停止
    pub fn can_stop(&self) -> bool {
        matches!(self, ModuleState::Running)
    }

    /// 是否可以卸载
    pub fn can_unload(&self) -> bool {
        matches!(self, ModuleState::Loaded | ModuleState::Stopped | ModuleState::Error)
    }
}

/// 模块运行时信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    /// 模块元数据
    pub metadata: ModuleMetadata,
    
    /// 当前状态
    pub state: ModuleState,
    
    /// 模块所在路径
    pub path: PathBuf,
    
    /// 加载时间
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loaded_at: Option<DateTime<Utc>>,
    
    /// 启动时间
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    
    /// 最后错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl ModuleInfo {
    /// 创建新的模块信息
    pub fn new(metadata: ModuleMetadata, path: PathBuf) -> Self {
        Self {
            metadata,
            state: ModuleState::Registered,
            path,
            loaded_at: None,
            started_at: None,
            last_error: None,
        }
    }

    /// 获取模块 ID
    pub fn id(&self) -> &str {
        &self.metadata.id
    }

    /// 获取模块版本
    pub fn version(&self) -> &str {
        &self.metadata.version
    }

    /// 检查模块是否正在运行
    pub fn is_running(&self) -> bool {
        self.state == ModuleState::Running
    }
}

/// 健康状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    /// 状态标识
    pub status: String,
    
    /// 详细信息
    #[serde(default)]
    pub details: HashMap<String, serde_json::Value>,
    
    /// 检查时间
    pub checked_at: DateTime<Utc>,
}

impl HealthStatus {
    /// 创建健康状态
    pub fn healthy() -> Self {
        Self {
            status: "healthy".to_string(),
            details: HashMap::new(),
            checked_at: Utc::now(),
        }
    }

    /// 创建降级状态
    pub fn degraded(reason: &str) -> Self {
        let mut details = HashMap::new();
        details.insert("reason".to_string(), serde_json::json!(reason));
        Self {
            status: "degraded".to_string(),
            details,
            checked_at: Utc::now(),
        }
    }

    /// 创建不健康状态
    pub fn unhealthy(reason: &str) -> Self {
        let mut details = HashMap::new();
        details.insert("reason".to_string(), serde_json::json!(reason));
        Self {
            status: "unhealthy".to_string(),
            details,
            checked_at: Utc::now(),
        }
    }

    /// 是否健康
    pub fn is_healthy(&self) -> bool {
        self.status == "healthy"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata_creation() {
        let metadata = ModuleMetadata::new("test_module", "Test Module", "1.0.0");
        
        assert_eq!(metadata.id, "test_module");
        assert_eq!(metadata.name, "Test Module");
        assert_eq!(metadata.version, "1.0.0");
    }

    #[test]
    fn test_module_metadata_validation() {
        let metadata = ModuleMetadata::new("test", "Test", "1.0.0");
        // 入口文件路径为空，应该失败
        assert!(metadata.validate().is_err());

        let mut metadata = metadata;
        metadata.entry = EntryPoint::rust("lib.rs");
        assert!(metadata.validate().is_ok());
    }

    #[test]
    fn test_module_state_transitions() {
        assert!(ModuleState::Registered.can_load());
        assert!(!ModuleState::Running.can_load());

        assert!(ModuleState::Loaded.can_start());
        assert!(!ModuleState::Loading.can_start());

        assert!(ModuleState::Running.can_stop());
        assert!(!ModuleState::Stopped.can_stop());

        assert!(ModuleState::Loaded.can_unload());
        assert!(!ModuleState::Running.can_unload());
    }

    #[test]
    fn test_dependency_version_check() {
        let dep = Dependency::new("other_module", "^1.0.0");
        
        assert!(dep.version_matches(&Version::parse("1.0.0").unwrap()));
        assert!(dep.version_matches(&Version::parse("1.5.0").unwrap()));
        assert!(!dep.version_matches(&Version::parse("2.0.0").unwrap()));
    }

    #[test]
    fn test_health_status() {
        let healthy = HealthStatus::healthy();
        assert!(healthy.is_healthy());

        let unhealthy = HealthStatus::unhealthy("connection failed");
        assert!(!unhealthy.is_healthy());
    }

    #[test]
    fn test_metadata_serialization() {
        let mut metadata = ModuleMetadata::new("test", "Test", "1.0.0");
        metadata.entry = EntryPoint::rust("lib.rs");
        metadata.dependencies.push(Dependency::new("dep1", "^1.0"));

        let yaml = serde_yaml::to_string(&metadata).unwrap();
        let parsed: ModuleMetadata = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(parsed.id, metadata.id);
        assert_eq!(parsed.dependencies.len(), 1);
    }
}
