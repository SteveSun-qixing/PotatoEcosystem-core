//! 工具模块
//!
//! 包含错误类型、ID 生成、日志系统、性能监控等通用工具。

pub mod error;
pub mod id;
pub mod logger;
pub mod metrics;

// 重导出常用类型
pub use error::{CoreError, Result, status_code, error_code};
pub use id::{generate_id, generate_uuid, is_valid_id, parse_id};
pub use logger::{Logger, LoggerConfig, LoggerConfigBuilder, LogGuard, RotationStrategy, fields};
pub use metrics::{
    MetricsCollector, MetricsReport, MonitoringSystem,
    ResourceMonitor, ResourceMonitorConfig, ResourceStatus,
    RouteMetrics, LatencyMetrics, ResourceMetrics,
};
