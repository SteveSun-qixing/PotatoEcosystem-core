//! 模块管理模块
//!
//! 包含模块管理系统的核心组件：
//! - 模块元数据定义
//! - 运行时管理器接口

pub mod metadata;
pub mod runtime;

// 重导出常用类型
pub use metadata::{
    Compatibility, Dependency, EntryPoint, HealthStatus, ModuleInfo, ModuleInterfaces,
    ModuleMetadata, ModulePermissions, ModuleState, ModuleType, Runtime,
};
