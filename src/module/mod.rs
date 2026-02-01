//! 模块管理模块
//!
//! 包含模块管理系统的核心组件：
//! - 模块元数据定义
//! - 模块元数据解析器
//! - 模块注册表
//! - 依赖图和依赖解析器
//! - 运行时管理器接口
//! - 模块加载器
//! - 生命周期管理器
//! - 模块管理器主结构体

pub mod dependency;
pub mod lifecycle;
pub mod loader;
pub mod manager;
pub mod metadata;
pub mod parser;
pub mod registry;
pub mod runtime;

// 重导出常用类型
pub use dependency::{DependencyGraph, DependencyResolver};
pub use lifecycle::LifecycleManager;
pub use loader::ModuleLoader;
pub use manager::{ModuleManager, ModuleManagerConfig};
pub use metadata::{
    Compatibility, Dependency, EntryPoint, HealthStatus, ModuleInfo, ModuleInterfaces,
    ModuleMetadata, ModulePermissions, ModuleState, ModuleType, Runtime,
};
pub use parser::ModuleParser;
pub use registry::ModuleRegistry;
pub use runtime::{ModuleHandle, ModuleInstance, RuntimeManager, RustRuntimeManager};
