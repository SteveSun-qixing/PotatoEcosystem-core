//! 路由模块
//!
//! 包含中心路由系统的核心组件：
//! - 路由请求/响应数据结构
//! - 路由表
//! - 事件系统
//! - 事件总线
//! - 请求验证器
//! - 并发控制和超时处理
//! - 路由器主结构体

pub mod concurrency;
pub mod event;
pub mod event_bus;
pub mod queue;
pub mod request;
pub mod route_table;
pub mod router;
pub mod validator;

// 重导出常用类型
pub use concurrency::{ConcurrencyLimiter, TimeoutManager, route_with_timeout, with_timeout};
pub use event::{Event, EventBuilder, EventFilter, Subscription, system_events};
pub use event_bus::{DispatchStats, EventBus, EventBusConfig, EventCallback};
pub use queue::{QueueItem, QueueStats, RequestQueue};
pub use request::{
    ErrorInfo, Priority, RouteRequest, RouteRequestBuilder, RouteResponse,
};
pub use route_table::{
    RouteCache, RouteCacheStats, RouteEntry, RouteTable, RouteTableConfig, RouteTableExport,
    RouteTableStats, RouteType,
};
pub use router::{ModuleHandler, Router, RouterConfig, RouterStats, RouterStatsSnapshot};
pub use validator::{
    is_valid_action_format, parse_action, RequestValidator, ValidationError,
    ValidationErrorCode, ValidationResult, ValidatorConfig,
};
