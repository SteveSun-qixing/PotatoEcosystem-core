//! API 模块
//!
//! 包含对外提供的 SDK 接口和通信机制。
//!
//! # 模块概览
//!
//! - `sdk`: ChipsCore 主接口，提供内核的所有功能访问
//! - `ipc`: 进程间通信接口，支持 Unix Socket 和 TCP
//!
//! # 示例
//!
//! ## 使用 SDK
//!
//! ```rust,no_run
//! use chips_core::{ChipsCore, CoreConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = CoreConfig::builder()
//!         .worker_count(8)
//!         .log_level("info")
//!         .build();
//!
//!     let mut core = ChipsCore::new(config).await?;
//!     core.start().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## 使用 IPC
//!
//! ```rust,no_run
//! use chips_core::api::ipc::{IpcClient, IpcConfig, IpcRequest};
//! use serde_json::json;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = IpcConfig::tcp("127.0.0.1", 9527);
//!     let mut client = IpcClient::connect(config).await?;
//!     
//!     let response = client.route("client", "test.action", json!({})).await?;
//!     println!("Response: {:?}", response);
//!     Ok(())
//! }
//! ```

pub mod ipc;
pub mod sdk;

// 重导出主要类型
pub use sdk::{ChipsCore, CoreState, HealthInfo};

pub use ipc::{
    IpcClient, IpcConfig, IpcConfigBuilder, IpcEvent, IpcMessageType, IpcRequest, IpcResponse,
    IpcServer, IpcServerStats, IpcTransport,
};
