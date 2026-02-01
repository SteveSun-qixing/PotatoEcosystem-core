//! 核心模块
//!
//! 包含内核配置和核心管理组件。

pub mod config;

pub use config::{
    ConfigManager, ConfigManagerBuilder, ConfigSource, CoreConfig, CoreConfigBuilder,
    LogConfig, ModuleConfig, RouterConfig,
};
