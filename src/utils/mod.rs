//! 工具模块
//!
//! 包含错误类型、ID 生成等通用工具。

pub mod error;
pub mod id;

// 重导出常用类型
pub use error::{CoreError, Result, status_code, error_code};
pub use id::{generate_id, generate_uuid, is_valid_id, parse_id};
