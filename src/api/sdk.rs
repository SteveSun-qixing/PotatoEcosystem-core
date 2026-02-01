//! ChipsCore SDK
//!
//! 薯片微内核的主要对外接口。

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, error};

use crate::core::config::CoreConfig;
use crate::router::{RouteRequest, RouteResponse, RouteTable, Event};
use crate::utils::{CoreError, Result, status_code};

/// 内核状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreState {
    /// 未初始化
    Uninitialized,
    /// 已初始化
    Initialized,
    /// 运行中
    Running,
    /// 正在关闭
    ShuttingDown,
    /// 已关闭
    Shutdown,
}

/// 薯片微内核主结构体
///
/// 这是整个内核的核心入口点，负责协调路由、模块管理、
/// 事件总线和配置管理等子系统。
pub struct ChipsCore {
    /// 内核配置
    config: CoreConfig,
    
    /// 内核状态
    state: Arc<RwLock<CoreState>>,
    
    /// 路由表
    route_table: Arc<RouteTable>,
    
    /// 启动时间
    started_at: Option<std::time::Instant>,
}

impl ChipsCore {
    /// 创建新的内核实例
    ///
    /// # Arguments
    ///
    /// * `config` - 内核配置
    ///
    /// # Returns
    ///
    /// 返回初始化后的内核实例
    ///
    /// # Errors
    ///
    /// 如果初始化失败（如配置无效），返回错误
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::{ChipsCore, CoreConfig};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let config = CoreConfig::default();
    ///     let core = ChipsCore::new(config).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn new(config: CoreConfig) -> Result<Self> {
        info!("初始化薯片微内核 v{}", crate::VERSION);
        
        // 创建路由表
        let route_table = Arc::new(RouteTable::new());
        
        let core = Self {
            config,
            state: Arc::new(RwLock::new(CoreState::Initialized)),
            route_table,
            started_at: None,
        };
        
        info!("薯片微内核初始化完成");
        Ok(core)
    }

    /// 启动内核
    ///
    /// 启动所有子系统，包括路由处理器、模块加载等。
    pub async fn start(&mut self) -> Result<()> {
        let mut state = self.state.write().await;
        
        if *state != CoreState::Initialized {
            return Err(CoreError::InitFailed(
                "内核必须处于 Initialized 状态才能启动".to_string()
            ));
        }
        
        info!("启动薯片微内核...");
        
        // TODO: 启动路由处理器
        // TODO: 加载自动加载的模块
        // TODO: 启动健康检查
        
        *state = CoreState::Running;
        self.started_at = Some(std::time::Instant::now());
        
        info!("薯片微内核已启动");
        Ok(())
    }

    /// 关闭内核
    ///
    /// 优雅地关闭所有子系统。
    pub async fn shutdown(&mut self) -> Result<()> {
        let mut state = self.state.write().await;
        
        if *state != CoreState::Running {
            return Ok(()); // 已经关闭或未启动
        }
        
        info!("正在关闭薯片微内核...");
        *state = CoreState::ShuttingDown;
        
        // TODO: 停止所有模块
        // TODO: 等待正在处理的请求完成
        // TODO: 清理资源
        
        *state = CoreState::Shutdown;
        info!("薯片微内核已关闭");
        
        Ok(())
    }

    /// 发送路由请求
    ///
    /// # Arguments
    ///
    /// * `request` - 路由请求
    ///
    /// # Returns
    ///
    /// 返回路由响应
    pub async fn route(&self, request: RouteRequest) -> RouteResponse {
        let start = std::time::Instant::now();
        
        // 检查内核状态
        {
            let state = self.state.read().await;
            if *state != CoreState::Running {
                return RouteResponse::error(
                    &request.request_id,
                    status_code::SERVICE_UNAVAILABLE,
                    crate::router::ErrorInfo::new("CORE_NOT_RUNNING", "内核未运行"),
                    0,
                );
            }
        }
        
        // 查找路由
        let route_entry = match self.route_table.find_route(&request).await {
            Some(entry) => entry,
            None => {
                let elapsed = start.elapsed().as_millis() as u64;
                return RouteResponse::error(
                    &request.request_id,
                    status_code::NOT_FOUND,
                    crate::router::ErrorInfo::new("ROUTE_NOT_FOUND", format!("未找到路由: {}", request.action)),
                    elapsed,
                );
            }
        };
        
        // TODO: 实际的请求转发逻辑
        // 目前返回一个占位响应
        let elapsed = start.elapsed().as_millis() as u64;
        RouteResponse::success(
            &request.request_id,
            serde_json::json!({
                "module_id": route_entry.module_id,
                "action": request.action,
                "status": "placeholder"
            }),
            elapsed,
        )
    }

    /// 发布事件
    ///
    /// # Arguments
    ///
    /// * `event` - 要发布的事件
    pub async fn publish(&self, _event: Event) -> Result<()> {
        // TODO: 实现事件发布
        Ok(())
    }

    /// 获取内核状态
    pub async fn state(&self) -> CoreState {
        *self.state.read().await
    }

    /// 获取内核配置
    pub fn config(&self) -> &CoreConfig {
        &self.config
    }

    /// 获取路由表引用
    pub fn route_table(&self) -> &Arc<RouteTable> {
        &self.route_table
    }

    /// 获取运行时间
    pub fn uptime(&self) -> Option<std::time::Duration> {
        self.started_at.map(|t| t.elapsed())
    }

    /// 检查内核是否正在运行
    pub async fn is_running(&self) -> bool {
        *self.state.read().await == CoreState::Running
    }
}

impl Drop for ChipsCore {
    fn drop(&mut self) {
        info!("薯片微内核实例被释放");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_core_creation() {
        let config = CoreConfig::default();
        let core = ChipsCore::new(config).await.unwrap();
        
        assert_eq!(core.state().await, CoreState::Initialized);
    }

    #[tokio::test]
    async fn test_core_start_shutdown() {
        let config = CoreConfig::default();
        let mut core = ChipsCore::new(config).await.unwrap();
        
        core.start().await.unwrap();
        assert_eq!(core.state().await, CoreState::Running);
        assert!(core.is_running().await);
        
        core.shutdown().await.unwrap();
        assert_eq!(core.state().await, CoreState::Shutdown);
        assert!(!core.is_running().await);
    }

    #[tokio::test]
    async fn test_route_before_start() {
        let config = CoreConfig::default();
        let core = ChipsCore::new(config).await.unwrap();
        
        let request = RouteRequest::new("test", "test.action", json!({}));
        let response = core.route(request).await;
        
        assert!(response.is_error());
        assert_eq!(response.status, status_code::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_route_not_found() {
        let config = CoreConfig::default();
        let mut core = ChipsCore::new(config).await.unwrap();
        core.start().await.unwrap();
        
        let request = RouteRequest::new("test", "unknown.action", json!({}));
        let response = core.route(request).await;
        
        assert!(response.is_error());
        assert_eq!(response.status, status_code::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_uptime() {
        let config = CoreConfig::default();
        let mut core = ChipsCore::new(config).await.unwrap();
        
        assert!(core.uptime().is_none());
        
        core.start().await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        assert!(core.uptime().is_some());
        assert!(core.uptime().unwrap().as_millis() >= 10);
    }
}
