//! 路由器主结构体
//!
//! 整合路由系统的所有组件，提供完整的路由功能。

use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, oneshot};
use tokio::task::JoinHandle;
use tracing::{info, debug, instrument};

use super::concurrency::{ConcurrencyLimiter, TimeoutManager};
use super::queue::{RequestQueue, QueueItem};
use super::request::{ErrorInfo, RouteRequest, RouteResponse};
use super::route_table::{RouteEntry, RouteTable};
use super::validator::RequestValidator;
use crate::utils::{CoreError, Result, status_code};

/// 路由器配置
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// 工作线程数
    pub worker_count: usize,
    /// 最大并发请求数
    pub max_concurrent: usize,
    /// 请求队列大小
    pub queue_size: usize,
    /// 默认超时时间（毫秒）
    pub default_timeout_ms: u64,
    /// 是否启用验证
    pub enable_validation: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        let cpu_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        
        Self {
            worker_count: cpu_count * 2,
            max_concurrent: 1000,
            queue_size: 10000,
            default_timeout_ms: 30000,
            enable_validation: true,
        }
    }
}

/// 路由统计信息
#[derive(Debug, Default)]
pub struct RouterStats {
    /// 总请求数
    total_requests: AtomicU64,
    /// 成功请求数
    successful_requests: AtomicU64,
    /// 失败请求数
    failed_requests: AtomicU64,
    /// 超时请求数
    timeout_requests: AtomicU64,
    /// 总延迟（微秒）
    total_latency_us: AtomicU64,
    /// 最小延迟（微秒）
    min_latency_us: AtomicU64,
    /// 最大延迟（微秒）
    max_latency_us: AtomicU64,
}

impl RouterStats {
    /// 创建新的统计实例
    pub fn new() -> Self {
        Self {
            min_latency_us: AtomicU64::new(u64::MAX),
            ..Default::default()
        }
    }

    /// 记录请求结果
    pub fn record(&self, success: bool, latency_us: u64, timeout: bool) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.total_latency_us.fetch_add(latency_us, Ordering::Relaxed);
        
        if success {
            self.successful_requests.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_requests.fetch_add(1, Ordering::Relaxed);
        }
        
        if timeout {
            self.timeout_requests.fetch_add(1, Ordering::Relaxed);
        }
        
        // 更新最小延迟
        let mut current_min = self.min_latency_us.load(Ordering::Relaxed);
        while latency_us < current_min {
            match self.min_latency_us.compare_exchange_weak(
                current_min,
                latency_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_min = actual,
            }
        }
        
        // 更新最大延迟
        let mut current_max = self.max_latency_us.load(Ordering::Relaxed);
        while latency_us > current_max {
            match self.max_latency_us.compare_exchange_weak(
                current_max,
                latency_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_max = actual,
            }
        }
    }

    /// 获取统计快照
    pub fn snapshot(&self) -> RouterStatsSnapshot {
        let total = self.total_requests.load(Ordering::Relaxed);
        let successful = self.successful_requests.load(Ordering::Relaxed);
        let failed = self.failed_requests.load(Ordering::Relaxed);
        let timeouts = self.timeout_requests.load(Ordering::Relaxed);
        let total_latency = self.total_latency_us.load(Ordering::Relaxed);
        let min_latency = self.min_latency_us.load(Ordering::Relaxed);
        let max_latency = self.max_latency_us.load(Ordering::Relaxed);
        
        RouterStatsSnapshot {
            total_requests: total,
            successful_requests: successful,
            failed_requests: failed,
            timeout_requests: timeouts,
            success_rate: if total > 0 { successful as f64 / total as f64 } else { 0.0 },
            avg_latency_us: if total > 0 { total_latency / total } else { 0 },
            min_latency_us: if min_latency == u64::MAX { 0 } else { min_latency },
            max_latency_us: max_latency,
        }
    }

    /// 重置统计
    pub fn reset(&self) {
        self.total_requests.store(0, Ordering::Relaxed);
        self.successful_requests.store(0, Ordering::Relaxed);
        self.failed_requests.store(0, Ordering::Relaxed);
        self.timeout_requests.store(0, Ordering::Relaxed);
        self.total_latency_us.store(0, Ordering::Relaxed);
        self.min_latency_us.store(u64::MAX, Ordering::Relaxed);
        self.max_latency_us.store(0, Ordering::Relaxed);
    }
}

/// 路由统计快照
#[derive(Debug, Clone)]
pub struct RouterStatsSnapshot {
    /// 总请求数
    pub total_requests: u64,
    /// 成功请求数
    pub successful_requests: u64,
    /// 失败请求数
    pub failed_requests: u64,
    /// 超时请求数
    pub timeout_requests: u64,
    /// 成功率
    pub success_rate: f64,
    /// 平均延迟（微秒）
    pub avg_latency_us: u64,
    /// 最小延迟（微秒）
    pub min_latency_us: u64,
    /// 最大延迟（微秒）
    pub max_latency_us: u64,
}

/// 模块处理器 trait
///
/// 模块需要实现此 trait 以处理路由请求
#[async_trait::async_trait]
pub trait ModuleHandler: Send + Sync {
    /// 处理请求
    async fn handle(&self, request: RouteRequest) -> RouteResponse;
    
    /// 获取模块 ID
    fn module_id(&self) -> &str;
}

/// 路由器
///
/// 中心路由系统的核心组件，负责：
/// - 接收和验证请求
/// - 查找路由
/// - 转发请求到目标模块
/// - 管理并发和超时
pub struct Router {
    /// 路由表
    route_table: Arc<RouteTable>,
    /// 请求队列
    queue: Arc<RequestQueue>,
    /// 并发限制器
    concurrency_limiter: Arc<ConcurrencyLimiter>,
    /// 超时管理器
    timeout_manager: Arc<TimeoutManager>,
    /// 请求验证器
    validator: Arc<RequestValidator>,
    /// 统计信息
    stats: Arc<RouterStats>,
    /// 模块处理器
    handlers: Arc<RwLock<std::collections::HashMap<String, Arc<dyn ModuleHandler>>>>,
    /// 配置
    config: RouterConfig,
    /// 是否正在运行
    running: Arc<AtomicBool>,
    /// 工作线程句柄
    workers: Arc<RwLock<Vec<JoinHandle<()>>>>,
}

impl Router {
    /// 创建新的路由器
    pub fn new(config: RouterConfig) -> Self {
        let route_table = Arc::new(RouteTable::new());
        let queue = Arc::new(RequestQueue::new(config.queue_size));
        let concurrency_limiter = Arc::new(ConcurrencyLimiter::new(config.max_concurrent));
        let timeout_manager = Arc::new(TimeoutManager::new(
            Duration::from_millis(config.default_timeout_ms)
        ));
        let validator = Arc::new(RequestValidator::new());
        let stats = Arc::new(RouterStats::new());
        
        Self {
            route_table,
            queue,
            concurrency_limiter,
            timeout_manager,
            validator,
            stats,
            handlers: Arc::new(RwLock::new(std::collections::HashMap::new())),
            config,
            running: Arc::new(AtomicBool::new(false)),
            workers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 使用默认配置创建路由器
    pub fn with_defaults() -> Self {
        Self::new(RouterConfig::default())
    }

    /// 启动路由器
    pub async fn start(&self) -> Result<()> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(CoreError::InitFailed("路由器已经在运行".to_string()));
        }
        
        info!(
            worker_count = self.config.worker_count,
            max_concurrent = self.config.max_concurrent,
            queue_size = self.config.queue_size,
            "启动路由器"
        );
        
        // 启动工作线程
        let mut workers = self.workers.write().await;
        for i in 0..self.config.worker_count {
            let worker = self.spawn_worker(i);
            workers.push(worker);
        }
        
        Ok(())
    }

    /// 停止路由器
    pub async fn stop(&self) -> Result<()> {
        if !self.running.swap(false, Ordering::SeqCst) {
            return Ok(()); // 已经停止
        }
        
        info!("停止路由器");
        
        // 等待工作线程结束
        let mut workers = self.workers.write().await;
        for worker in workers.drain(..) {
            // 给工作线程一点时间来完成当前任务
            let _ = tokio::time::timeout(Duration::from_secs(5), worker).await;
        }
        
        Ok(())
    }

    /// 生成工作线程
    fn spawn_worker(&self, worker_id: usize) -> JoinHandle<()> {
        let queue = Arc::clone(&self.queue);
        let handlers = Arc::clone(&self.handlers);
        let stats = Arc::clone(&self.stats);
        let running = Arc::clone(&self.running);
        let concurrency_limiter = Arc::clone(&self.concurrency_limiter);
        let timeout_manager = Arc::clone(&self.timeout_manager);
        
        tokio::spawn(async move {
            debug!(worker_id, "工作线程启动");
            
            while running.load(Ordering::Relaxed) {
                // 尝试获取请求
                let item = match tokio::time::timeout(
                    Duration::from_millis(100),
                    queue.dequeue(),
                ).await {
                    Ok(item) => item,
                    Err(_) => continue, // 超时，继续检查是否还在运行
                };
                
                // 获取并发许可
                let _permit = concurrency_limiter.acquire().await;
                
                let start = Instant::now();
                let request_id = item.request.request_id.clone();
                let timeout_ms = item.request.timeout_ms;
                
                // 处理请求
                let response = Self::process_item(
                    item,
                    &handlers,
                    &timeout_manager,
                    timeout_ms,
                ).await;
                
                let elapsed_us = start.elapsed().as_micros() as u64;
                let success = response.is_success();
                let timeout = response.status == status_code::TIMEOUT;
                
                // 记录统计
                stats.record(success, elapsed_us, timeout);
                
                debug!(
                    request_id,
                    success,
                    elapsed_us,
                    "请求处理完成"
                );
            }
            
            debug!(worker_id, "工作线程结束");
        })
    }

    /// 处理单个请求
    async fn process_item(
        item: QueueItem,
        handlers: &Arc<RwLock<std::collections::HashMap<String, Arc<dyn ModuleHandler>>>>,
        timeout_manager: &Arc<TimeoutManager>,
        timeout_ms: u64,
    ) -> RouteResponse {
        let request = item.request;
        let target_module = item.target_module;
        let response_tx = item.response_tx;
        
        // 查找处理器
        let handler = {
            let handlers = handlers.read().await;
            handlers.get(&target_module).cloned()
        };
        
        let response = match handler {
            Some(handler) => {
                // 带超时执行
                let timeout = Duration::from_millis(timeout_ms);
                match timeout_manager.with_timeout(timeout, handler.handle(request.clone())).await {
                    Ok(response) => response,
                    Err(_) => {
                        RouteResponse::error(
                            &request.request_id,
                            status_code::TIMEOUT,
                            ErrorInfo::new("TIMEOUT", "请求超时"),
                            timeout_ms,
                        )
                    }
                }
            }
            None => {
                RouteResponse::error(
                    &request.request_id,
                    status_code::NOT_FOUND,
                    ErrorInfo::new("MODULE_NOT_FOUND", format!("模块未找到: {}", target_module)),
                    0,
                )
            }
        };
        
        // 发送响应
        let _ = response_tx.send(response.clone());
        response
    }

    /// 发送路由请求
    #[instrument(skip(self), fields(request_id = %request.request_id, action = %request.action))]
    pub async fn route(&self, request: RouteRequest) -> RouteResponse {
        let start = Instant::now();
        let request_id = request.request_id.clone();
        
        // 检查是否正在运行
        if !self.running.load(Ordering::Relaxed) {
            return RouteResponse::error(
                &request_id,
                status_code::SERVICE_UNAVAILABLE,
                ErrorInfo::new("ROUTER_NOT_RUNNING", "路由器未运行"),
                start.elapsed().as_millis() as u64,
            );
        }
        
        // 验证请求
        if self.config.enable_validation {
            let validation_result = self.validator.validate_format(&request);
            if !validation_result.is_valid {
                let errors: Vec<String> = validation_result.errors.iter()
                    .map(|e| e.message.clone())
                    .collect();
                return RouteResponse::error(
                    &request_id,
                    status_code::BAD_REQUEST,
                    ErrorInfo::new("INVALID_REQUEST", errors.join("; ")),
                    start.elapsed().as_millis() as u64,
                );
            }
        }
        
        // 查找路由
        let route_entry = match self.route_table.find_route(&request).await {
            Some(entry) => entry,
            None => {
                return RouteResponse::error(
                    &request_id,
                    status_code::NOT_FOUND,
                    ErrorInfo::new("ROUTE_NOT_FOUND", format!("未找到路由: {}", request.action)),
                    start.elapsed().as_millis() as u64,
                );
            }
        };
        
        // 创建响应通道
        let (response_tx, response_rx) = oneshot::channel();
        
        // 创建队列项
        let item = QueueItem::new(
            request.clone(),
            route_entry.module_id.clone(),
            response_tx,
        );
        
        // 入队
        if let Err(e) = self.queue.enqueue(item).await {
            return RouteResponse::error(
                &request_id,
                status_code::SERVICE_UNAVAILABLE,
                ErrorInfo::new("QUEUE_FULL", e.to_string()),
                start.elapsed().as_millis() as u64,
            );
        }
        
        // 等待响应
        match tokio::time::timeout(
            Duration::from_millis(request.timeout_ms),
            response_rx,
        ).await {
            Ok(Ok(mut response)) => {
                response.elapsed_ms = start.elapsed().as_millis() as u64;
                response
            }
            Ok(Err(_)) => {
                // 响应通道关闭
                RouteResponse::error(
                    &request_id,
                    status_code::INTERNAL_ERROR,
                    ErrorInfo::new("CHANNEL_CLOSED", "响应通道已关闭"),
                    start.elapsed().as_millis() as u64,
                )
            }
            Err(_) => {
                // 超时
                RouteResponse::error(
                    &request_id,
                    status_code::TIMEOUT,
                    ErrorInfo::new("TIMEOUT", "请求超时"),
                    start.elapsed().as_millis() as u64,
                )
            }
        }
    }

    /// 注册模块处理器
    pub async fn register_handler(&self, handler: Arc<dyn ModuleHandler>) -> Result<()> {
        let module_id = handler.module_id().to_string();
        let mut handlers = self.handlers.write().await;
        handlers.insert(module_id.clone(), handler);
        info!(module_id, "注册模块处理器");
        Ok(())
    }

    /// 注销模块处理器
    pub async fn unregister_handler(&self, module_id: &str) -> Result<()> {
        let mut handlers = self.handlers.write().await;
        handlers.remove(module_id);
        
        // 同时移除相关路由
        self.route_table.remove_module_routes(module_id).await;
        
        info!(module_id, "注销模块处理器");
        Ok(())
    }

    /// 注册路由
    pub async fn register_route(&self, action: &str, entry: RouteEntry) -> Result<()> {
        self.route_table.register_action_route(action, entry).await
    }

    /// 获取路由表引用
    pub fn route_table(&self) -> &Arc<RouteTable> {
        &self.route_table
    }

    /// 获取统计信息
    pub fn stats(&self) -> RouterStatsSnapshot {
        self.stats.snapshot()
    }

    /// 重置统计信息
    pub fn reset_stats(&self) {
        self.stats.reset();
    }

    /// 检查是否正在运行
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// 获取配置
    pub fn config(&self) -> &RouterConfig {
        &self.config
    }

    /// 获取当前队列大小
    pub fn queue_size(&self) -> usize {
        self.queue.size()
    }

    /// 获取当前并发数
    pub fn current_concurrency(&self) -> usize {
        self.concurrency_limiter.current_concurrency()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct MockHandler {
        id: String,
        response_data: serde_json::Value,
    }

    impl MockHandler {
        fn new(id: &str, response_data: serde_json::Value) -> Self {
            Self {
                id: id.to_string(),
                response_data,
            }
        }
    }

    #[async_trait::async_trait]
    impl ModuleHandler for MockHandler {
        async fn handle(&self, request: RouteRequest) -> RouteResponse {
            RouteResponse::success(&request.request_id, self.response_data.clone(), 1)
        }

        fn module_id(&self) -> &str {
            &self.id
        }
    }

    #[tokio::test]
    async fn test_router_creation() {
        let router = Router::with_defaults();
        assert!(!router.is_running());
    }

    #[tokio::test]
    async fn test_router_start_stop() {
        let router = Router::with_defaults();
        
        router.start().await.unwrap();
        assert!(router.is_running());
        
        router.stop().await.unwrap();
        assert!(!router.is_running());
    }

    #[tokio::test]
    async fn test_route_not_running() {
        let router = Router::with_defaults();
        let request = RouteRequest::new("test", "test.action", json!({}));
        
        let response = router.route(request).await;
        assert!(response.is_error());
        assert_eq!(response.status, status_code::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_route_not_found() {
        let router = Router::with_defaults();
        router.start().await.unwrap();
        
        let request = RouteRequest::new("test", "unknown.action", json!({}));
        let response = router.route(request).await;
        
        assert!(response.is_error());
        assert_eq!(response.status, status_code::NOT_FOUND);
        
        router.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_stats() {
        let stats = RouterStats::new();
        
        stats.record(true, 1000, false);
        stats.record(true, 2000, false);
        stats.record(false, 3000, true);
        
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.total_requests, 3);
        assert_eq!(snapshot.successful_requests, 2);
        assert_eq!(snapshot.failed_requests, 1);
        assert_eq!(snapshot.timeout_requests, 1);
        assert_eq!(snapshot.min_latency_us, 1000);
        assert_eq!(snapshot.max_latency_us, 3000);
    }

    #[tokio::test]
    async fn test_stats_reset() {
        let stats = RouterStats::new();
        
        stats.record(true, 1000, false);
        stats.reset();
        
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.total_requests, 0);
    }
}
