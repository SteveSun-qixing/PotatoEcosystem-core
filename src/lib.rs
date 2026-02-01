//! # Chips Core - 薯片微内核
//!
//! 薯片微内核是薯片生态的核心组件，提供以下核心功能：
//!
//! - **中心路由系统**: 所有模块间的通信都通过内核路由
//! - **模块管理系统**: 模块的加载、卸载、依赖解析和生命周期管理
//! - **事件总线**: 模块间的松耦合通信机制
//! - **配置管理**: 统一的配置加载和管理
//! - **日志系统**: 结构化日志记录
//!
//! ## 快速开始
//!
//! ```rust,no_run
//! use chips_core::{ChipsCore, CoreConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // 创建内核实例
//!     let config = CoreConfig::default();
//!     let mut core = ChipsCore::new(config).await?;
//!
//!     // 启动内核
//!     core.start().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## 模块结构
//!
//! - `router` - 路由系统相关类型
//! - `module` - 模块管理相关类型
//! - `utils` - 工具函数和错误类型
//! - `core` - 核心配置和管理
//! - `api` - 公共 API 接口

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

pub mod api;
pub mod core;
pub mod module;
pub mod router;
pub mod utils;

// 重导出常用类型，方便使用
pub use router::{
    Event, EventBuilder, EventFilter, Priority, RouteEntry, RouteRequest,
    RouteRequestBuilder, RouteResponse, RouteTable, RouteType, Subscription,
};

pub use module::{
    Dependency, EntryPoint, HealthStatus, ModuleInfo, ModuleMetadata, ModuleState,
    ModuleType, Runtime,
};

pub use utils::{error_code, generate_id, generate_uuid, status_code, CoreError, Result};
pub use utils::logger::{Logger, LoggerConfig, LoggerConfigBuilder, LogGuard, RotationStrategy, fields};

pub use core::config::{
    ConfigManager, ConfigManagerBuilder, ConfigSource, CoreConfig, CoreConfigBuilder,
    LogConfig, ModuleConfig, RouterConfig,
};
pub use api::sdk::ChipsCore;

/// 库版本
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 协议版本
pub const PROTOCOL_VERSION: &str = "1.0";

/// 最低兼容协议版本
pub const MIN_PROTOCOL_VERSION: &str = "1.0";
