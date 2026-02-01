//! 路由模块
//!
//! 包含中心路由系统的核心组件：
//! - 路由请求/响应数据结构
//! - 路由表
//! - 事件系统

pub mod event;
pub mod request;
pub mod route_table;

// 重导出常用类型
pub use event::{Event, EventBuilder, EventFilter, Subscription, system_events};
pub use request::{
    ErrorInfo, Priority, RouteRequest, RouteRequestBuilder, RouteResponse,
};
pub use route_table::{RouteEntry, RouteTable, RouteTableExport, RouteTableStats, RouteType};
