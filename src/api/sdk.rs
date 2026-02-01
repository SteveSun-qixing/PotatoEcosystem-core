//! ChipsCore SDK
//!
//! 薯片微内核的主要对外接口。提供统一的 API 来访问内核的所有功能，包括：
//!
//! - 路由系统：发送请求、获取响应
//! - 事件系统：发布/订阅事件
//! - 模块管理：加载、卸载、管理模块
//! - 配置管理：读取和修改配置
//!
//! # 示例
//!
//! ```rust,no_run
//! use chips_core::{ChipsCore, CoreConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // 使用 Builder 模式创建配置
//!     let config = CoreConfig::builder()
//!         .worker_count(8)
//!         .log_level("info")
//!         .build();
//!
//!     // 创建内核实例
//!     let mut core = ChipsCore::new(config).await?;
//!
//!     // 启动内核
//!     core.start().await?;
//!
//!     // 发送路由请求
//!     let response = core.route_action("example", "test.action", serde_json::json!({})).await;
//!
//!     // 关闭内核
//!     core.shutdown().await?;
//!
//!     Ok(())
//! }
//! ```

use std::sync::Arc;
use std::time::Instant;

use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::core::config::{ConfigManager, CoreConfig};
use crate::module::manager::{ModuleManager, ModuleManagerConfig};
use crate::router::{
    Event, EventBus, EventBusConfig, EventCallback, EventFilter, RouteRequest, RouteResponse,
    Router, RouterConfig, RouterStatsSnapshot,
};
use crate::utils::metrics::MetricsCollector;
use crate::utils::{status_code, CoreError, Result};

// ============================================================================
// 内核状态
// ============================================================================

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

impl CoreState {
    /// 检查是否可以启动
    pub fn can_start(&self) -> bool {
        matches!(self, CoreState::Initialized)
    }

    /// 检查是否可以关闭
    pub fn can_shutdown(&self) -> bool {
        matches!(self, CoreState::Running)
    }

    /// 检查是否正在运行
    pub fn is_running(&self) -> bool {
        matches!(self, CoreState::Running)
    }
}

// ============================================================================
// ChipsCore 主结构体
// ============================================================================

/// 薯片微内核主结构体
///
/// 这是整个内核的核心入口点，负责协调路由、模块管理、
/// 事件总线和配置管理等子系统。
///
/// # 组件
///
/// - `router`: 中心路由器，处理所有模块间通信
/// - `event_bus`: 事件总线，提供发布/订阅机制
/// - `config_manager`: 配置管理器，处理配置的加载和存储
/// - `module_manager`: 模块管理器，管理模块的生命周期
/// - `metrics`: 性能指标收集器
///
/// # 生命周期
///
/// 1. `new()` - 创建并初始化内核
/// 2. `start()` - 启动内核和所有子系统
/// 3. `shutdown()` - 优雅关闭内核
pub struct ChipsCore {
    /// 内核配置
    config: CoreConfig,

    /// 内核状态
    state: Arc<RwLock<CoreState>>,

    /// 中心路由器
    router: Arc<Router>,

    /// 事件总线
    event_bus: Arc<EventBus>,

    /// 配置管理器
    config_manager: Arc<ConfigManager>,

    /// 模块管理器
    module_manager: Arc<RwLock<ModuleManager>>,

    /// 性能指标收集器
    metrics: Arc<MetricsCollector>,

    /// 启动时间
    started_at: Option<Instant>,
}

impl ChipsCore {
    // ========================================================================
    // Task 6.2: 初始化和生命周期
    // ========================================================================

    /// 创建新的内核实例
    ///
    /// 初始化所有子系统，包括路由器、事件总线、配置管理器、模块管理器和指标收集器。
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

        // 1. 创建路由器
        let router_config = RouterConfig {
            worker_count: config.router.worker_count,
            max_concurrent: config.router.max_concurrent,
            queue_size: config.router.queue_size,
            default_timeout_ms: config.router.default_timeout_ms,
            enable_validation: true,
        };
        let router = Arc::new(Router::new(router_config));
        debug!("路由器初始化完成");

        // 2. 创建事件总线
        let event_bus = Arc::new(EventBus::with_config(EventBusConfig::default()));
        debug!("事件总线初始化完成");

        // 3. 创建配置管理器
        let config_manager = Arc::new(ConfigManager::new());
        debug!("配置管理器初始化完成");

        // 4. 创建模块管理器
        let module_config = ModuleManagerConfig {
            module_dirs: config.modules.module_dirs.clone(),
            hot_reload: config.modules.hot_reload,
            health_check_interval_secs: config.modules.health_check_interval_secs,
            auto_load: config.modules.auto_load.clone(),
        };
        let mut module_manager = ModuleManager::new(module_config);

        // 设置路由表引用
        module_manager.set_route_table(router.route_table().clone());

        // 设置事件发布器
        let event_bus_clone = event_bus.clone();
        module_manager.set_event_publisher(move |event| {
            let bus = event_bus_clone.clone();
            Box::pin(async move {
                let _ = bus.publish(event).await;
            })
        });

        debug!("模块管理器初始化完成");

        // 5. 创建指标收集器
        let metrics = Arc::new(MetricsCollector::new());
        debug!("指标收集器初始化完成");

        let core = Self {
            config,
            state: Arc::new(RwLock::new(CoreState::Initialized)),
            router,
            event_bus,
            config_manager,
            module_manager: Arc::new(RwLock::new(module_manager)),
            metrics,
            started_at: None,
        };

        info!("薯片微内核初始化完成");
        Ok(core)
    }

    /// 启动内核
    ///
    /// 启动所有子系统，包括路由器和模块管理器。
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::{ChipsCore, CoreConfig};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let mut core = ChipsCore::new(CoreConfig::default()).await?;
    ///     core.start().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn start(&mut self) -> Result<()> {
        let mut state = self.state.write().await;

        if !state.can_start() {
            return Err(CoreError::InitFailed(format!(
                "内核当前状态 {:?} 不允许启动",
                *state
            )));
        }

        info!("启动薯片微内核...");

        // 1. 启动路由器
        self.router.start().await?;
        debug!("路由器已启动");

        // 2. 初始化模块管理器
        {
            let manager = self.module_manager.read().await;
            manager.initialize().await?;
        }
        debug!("模块管理器已初始化");

        // 3. 发布启动事件
        let _ = self
            .event_bus
            .publish(Event::new(
                "core.started",
                "chips_core",
                serde_json::json!({
                    "version": crate::VERSION,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }),
            ))
            .await;

        *state = CoreState::Running;
        self.started_at = Some(Instant::now());

        info!("薯片微内核已启动");
        Ok(())
    }

    /// 关闭内核
    ///
    /// 优雅地关闭所有子系统，包括停止所有模块、停止路由器。
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::{ChipsCore, CoreConfig};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let mut core = ChipsCore::new(CoreConfig::default()).await?;
    ///     core.start().await?;
    ///     // ... 使用内核 ...
    ///     core.shutdown().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn shutdown(&mut self) -> Result<()> {
        let mut state = self.state.write().await;

        if !state.can_shutdown() {
            // 如果已经关闭或未启动，静默返回
            return Ok(());
        }

        info!("正在关闭薯片微内核...");
        *state = CoreState::ShuttingDown;

        // 发布关闭事件
        let _ = self
            .event_bus
            .publish(Event::new(
                "core.shutting_down",
                "chips_core",
                serde_json::json!({
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }),
            ))
            .await;

        // 1. 停止所有运行中的模块
        {
            let manager = self.module_manager.read().await;
            let running_modules = manager.get_running_modules().await;
            for module in running_modules {
                if let Err(e) = manager.stop_module(&module.metadata.id).await {
                    warn!(
                        module_id = %module.metadata.id,
                        error = %e,
                        "停止模块失败"
                    );
                }
            }
        }
        debug!("所有模块已停止");

        // 2. 停止路由器
        self.router.stop().await?;
        debug!("路由器已停止");

        *state = CoreState::Shutdown;

        info!("薯片微内核已关闭");
        Ok(())
    }

    // ========================================================================
    // Task 6.3: 便捷路由 API
    // ========================================================================

    /// 发送路由请求
    ///
    /// 将请求通过路由器转发到目标模块。
    ///
    /// # Arguments
    ///
    /// * `request` - 路由请求
    ///
    /// # Returns
    ///
    /// 返回路由响应
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::{ChipsCore, CoreConfig, RouteRequest};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let mut core = ChipsCore::new(CoreConfig::default()).await?;
    ///     core.start().await?;
    ///
    ///     let request = RouteRequest::new("sender", "module.action", serde_json::json!({}));
    ///     let response = core.route(request).await;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn route(&self, request: RouteRequest) -> RouteResponse {
        let start = Instant::now();

        // 检查内核状态
        {
            let state = self.state.read().await;
            if !state.is_running() {
                return RouteResponse::error(
                    &request.request_id,
                    status_code::SERVICE_UNAVAILABLE,
                    crate::router::ErrorInfo::new("CORE_NOT_RUNNING", "内核未运行"),
                    0,
                );
            }
        }

        // 使用路由器处理请求
        let response = self.router.route(request).await;

        // 记录指标
        let latency_us = start.elapsed().as_micros() as u64;
        self.metrics.record_route(response.is_success(), latency_us);

        response
    }

    /// 便捷路由方法
    ///
    /// 简化的路由请求方法，自动处理序列化和错误处理。
    ///
    /// # Arguments
    ///
    /// * `sender` - 发送者标识
    /// * `action` - 目标动作
    /// * `params` - 请求参数（任何可序列化的类型）
    ///
    /// # Returns
    ///
    /// 成功返回响应数据，失败返回错误
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::{ChipsCore, CoreConfig};
    /// use serde_json::json;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let mut core = ChipsCore::new(CoreConfig::default()).await?;
    ///     core.start().await?;
    ///
    ///     let result = core.route_action("client", "user.login", json!({
    ///         "username": "test",
    ///         "password": "password"
    ///     })).await;
    ///
    ///     match result {
    ///         Ok(data) => println!("Success: {:?}", data),
    ///         Err(e) => println!("Error: {:?}", e),
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn route_action<P: Serialize>(
        &self,
        sender: &str,
        action: &str,
        params: P,
    ) -> Result<serde_json::Value> {
        let params_value = serde_json::to_value(params)
            .map_err(|e| CoreError::InvalidRequest(format!("参数序列化失败: {}", e)))?;

        let request = RouteRequest::new(sender, action, params_value);
        let response = self.route(request).await;

        if response.is_success() {
            Ok(response.data.unwrap_or(serde_json::Value::Null))
        } else {
            Err(CoreError::RouteFailed {
                action: action.to_string(),
                reason: response
                    .error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "未知错误".to_string()),
            })
        }
    }

    /// 带类型的路由方法
    ///
    /// 自动将响应数据反序列化为指定类型。
    ///
    /// # Type Parameters
    ///
    /// * `P` - 参数类型
    /// * `R` - 响应类型
    ///
    /// # Arguments
    ///
    /// * `sender` - 发送者标识
    /// * `action` - 目标动作
    /// * `params` - 请求参数
    ///
    /// # Returns
    ///
    /// 成功返回反序列化后的响应数据
    pub async fn route_typed<P: Serialize, R: DeserializeOwned>(
        &self,
        sender: &str,
        action: &str,
        params: P,
    ) -> Result<R> {
        let data = self.route_action(sender, action, params).await?;
        serde_json::from_value(data)
            .map_err(|e| CoreError::InvalidRequest(format!("响应反序列化失败: {}", e)))
    }

    // ========================================================================
    // Task 6.4: 便捷事件 API
    // ========================================================================

    /// 订阅事件
    ///
    /// 注册一个事件处理器，当匹配的事件发生时调用。
    ///
    /// # Arguments
    ///
    /// * `subscriber_id` - 订阅者标识
    /// * `event_type` - 事件类型（支持通配符，如 `module.*`）
    /// * `handler` - 事件处理函数
    ///
    /// # Returns
    ///
    /// 返回订阅 ID，用于取消订阅
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::{ChipsCore, CoreConfig};
    /// use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let mut core = ChipsCore::new(CoreConfig::default()).await?;
    ///     core.start().await?;
    ///
    ///     // 订阅所有模块事件
    ///     let sub_id = core.subscribe("my_handler", "module.*", Arc::new(|event| {
    ///         println!("收到事件: {:?}", event.event_type);
    ///     })).await?;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn subscribe(
        &self,
        subscriber_id: &str,
        event_type: &str,
        handler: EventCallback,
    ) -> Result<String> {
        self.event_bus
            .subscribe(subscriber_id, event_type, None, handler)
            .await
    }

    /// 订阅事件（带过滤器）
    ///
    /// 注册一个带过滤条件的事件处理器。
    ///
    /// # Arguments
    ///
    /// * `subscriber_id` - 订阅者标识
    /// * `event_type` - 事件类型
    /// * `filter` - 事件过滤器
    /// * `handler` - 事件处理函数
    ///
    /// # Returns
    ///
    /// 返回订阅 ID
    pub async fn subscribe_with_filter(
        &self,
        subscriber_id: &str,
        event_type: &str,
        filter: EventFilter,
        handler: EventCallback,
    ) -> Result<String> {
        self.event_bus
            .subscribe(subscriber_id, event_type, Some(filter), handler)
            .await
    }

    /// 发布事件
    ///
    /// 向事件总线发布一个事件，所有匹配的订阅者都会收到通知。
    ///
    /// # Arguments
    ///
    /// * `event` - 要发布的事件
    ///
    /// # Returns
    ///
    /// 返回收到事件的订阅者数量
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::{ChipsCore, CoreConfig, Event};
    /// use serde_json::json;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let mut core = ChipsCore::new(CoreConfig::default()).await?;
    ///     core.start().await?;
    ///
    ///     // 发布自定义事件
    ///     let event = Event::new("custom.event", "my_module", json!({
    ///         "key": "value"
    ///     }));
    ///     let count = core.publish(event).await?;
    ///     println!("事件发送给 {} 个订阅者", count);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn publish(&self, event: Event) -> Result<usize> {
        self.event_bus.publish(event).await
    }

    /// 发布事件（便捷方法）
    ///
    /// 快速创建并发布事件。
    ///
    /// # Arguments
    ///
    /// * `event_type` - 事件类型
    /// * `sender` - 发送者标识
    /// * `data` - 事件数据
    ///
    /// # Returns
    ///
    /// 返回收到事件的订阅者数量
    pub async fn publish_event<D: Serialize>(
        &self,
        event_type: &str,
        sender: &str,
        data: D,
    ) -> Result<usize> {
        let data_value = serde_json::to_value(data)
            .map_err(|e| CoreError::InvalidRequest(format!("事件数据序列化失败: {}", e)))?;
        let event = Event::new(event_type, sender, data_value);
        self.event_bus.publish(event).await
    }

    /// 同步发布事件
    ///
    /// 发布事件并等待所有处理器完成。
    ///
    /// # Arguments
    ///
    /// * `event` - 要发布的事件
    ///
    /// # Returns
    ///
    /// 返回 (成功数, 失败数, 超时数)
    pub async fn publish_sync(&self, event: Event) -> Result<(usize, usize, usize)> {
        self.event_bus.publish_sync(event).await
    }

    /// 取消订阅
    ///
    /// 根据订阅 ID 取消事件订阅。
    ///
    /// # Arguments
    ///
    /// * `subscription_id` - 订阅 ID（由 `subscribe` 返回）
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::{ChipsCore, CoreConfig};
    /// use std::sync::Arc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let mut core = ChipsCore::new(CoreConfig::default()).await?;
    ///     core.start().await?;
    ///
    ///     let sub_id = core.subscribe("handler", "event.*", Arc::new(|_| {})).await?;
    ///
    ///     // 取消订阅
    ///     core.unsubscribe(&sub_id).await?;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn unsubscribe(&self, subscription_id: &str) -> Result<()> {
        self.event_bus.unsubscribe(subscription_id).await
    }

    /// 取消模块的所有订阅
    ///
    /// # Arguments
    ///
    /// * `subscriber_id` - 订阅者模块 ID
    ///
    /// # Returns
    ///
    /// 返回取消的订阅数量
    pub async fn unsubscribe_all(&self, subscriber_id: &str) -> Result<usize> {
        self.event_bus.unsubscribe_all(subscriber_id).await
    }

    // ========================================================================
    // 配置管理 API
    // ========================================================================

    /// 获取配置值
    ///
    /// # Arguments
    ///
    /// * `key` - 配置键（支持点号分隔的嵌套路径）
    ///
    /// # Returns
    ///
    /// 返回配置值，如果不存在则返回 None
    pub async fn get_config<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.config_manager.get(key).await
    }

    /// 获取配置值（带默认值）
    pub async fn get_config_or<T: DeserializeOwned>(&self, key: &str, default: T) -> T {
        self.config_manager.get_or(key, default).await
    }

    /// 设置配置值
    ///
    /// # Arguments
    ///
    /// * `key` - 配置键
    /// * `value` - 配置值
    pub async fn set_config<T: Serialize>(&self, key: &str, value: T) -> Result<()> {
        self.config_manager.set(key, value).await
    }

    // ========================================================================
    // 模块管理 API
    // ========================================================================

    /// 加载模块
    ///
    /// # Arguments
    ///
    /// * `module_id` - 模块 ID
    pub async fn load_module(&self, module_id: &str) -> Result<()> {
        let manager = self.module_manager.read().await;
        manager.load_module(module_id).await
    }

    /// 卸载模块
    ///
    /// # Arguments
    ///
    /// * `module_id` - 模块 ID
    pub async fn unload_module(&self, module_id: &str) -> Result<()> {
        let manager = self.module_manager.read().await;
        manager.unload_module(module_id).await
    }

    /// 获取模块列表
    pub async fn list_modules(&self) -> Vec<crate::module::metadata::ModuleInfo> {
        let manager = self.module_manager.read().await;
        manager.list_modules().await
    }

    /// 获取模块信息
    pub async fn get_module(
        &self,
        module_id: &str,
    ) -> Option<crate::module::metadata::ModuleInfo> {
        let manager = self.module_manager.read().await;
        manager.get_module(module_id).await
    }

    // ========================================================================
    // 状态和统计信息
    // ========================================================================

    /// 获取内核状态
    pub async fn state(&self) -> CoreState {
        *self.state.read().await
    }

    /// 获取内核配置
    pub fn config(&self) -> &CoreConfig {
        &self.config
    }

    /// 获取路由器引用
    pub fn router(&self) -> &Arc<Router> {
        &self.router
    }

    /// 获取事件总线引用
    pub fn event_bus(&self) -> &Arc<EventBus> {
        &self.event_bus
    }

    /// 获取配置管理器引用
    pub fn config_manager(&self) -> &Arc<ConfigManager> {
        &self.config_manager
    }

    /// 获取指标收集器引用
    pub fn metrics(&self) -> &Arc<MetricsCollector> {
        &self.metrics
    }

    /// 获取路由器统计信息
    pub fn router_stats(&self) -> RouterStatsSnapshot {
        self.router.stats()
    }

    /// 获取运行时间
    pub fn uptime(&self) -> Option<std::time::Duration> {
        self.started_at.map(|t| t.elapsed())
    }

    /// 检查内核是否正在运行
    pub async fn is_running(&self) -> bool {
        self.state.read().await.is_running()
    }

    /// 获取健康状态
    pub async fn health(&self) -> HealthInfo {
        let state = self.state.read().await;
        let router_stats = self.router.stats();
        let event_stats = self.event_bus.stats().await;

        HealthInfo {
            state: *state,
            uptime_secs: self.uptime().map(|d| d.as_secs()),
            router_running: self.router.is_running(),
            total_requests: router_stats.total_requests,
            success_rate: router_stats.success_rate,
            event_subscriptions: self.event_bus.subscription_count().await,
            events_dispatched: event_stats.total_dispatched,
        }
    }
}

impl Drop for ChipsCore {
    fn drop(&mut self) {
        info!("薯片微内核实例被释放");
    }
}

// ============================================================================
// 健康信息
// ============================================================================

/// 健康状态信息
#[derive(Debug, Clone)]
pub struct HealthInfo {
    /// 内核状态
    pub state: CoreState,
    /// 运行时间（秒）
    pub uptime_secs: Option<u64>,
    /// 路由器是否运行
    pub router_running: bool,
    /// 总请求数
    pub total_requests: u64,
    /// 成功率
    pub success_rate: f64,
    /// 事件订阅数
    pub event_subscriptions: usize,
    /// 已分发事件数
    pub events_dispatched: u64,
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

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

        // 启动
        core.start().await.unwrap();
        assert_eq!(core.state().await, CoreState::Running);
        assert!(core.is_running().await);
        assert!(core.uptime().is_some());

        // 关闭
        core.shutdown().await.unwrap();
        assert_eq!(core.state().await, CoreState::Shutdown);
        assert!(!core.is_running().await);
    }

    #[tokio::test]
    async fn test_double_start_fails() {
        let config = CoreConfig::default();
        let mut core = ChipsCore::new(config).await.unwrap();

        core.start().await.unwrap();

        // 再次启动应该失败
        let result = core.start().await;
        assert!(result.is_err());
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
    async fn test_route_action() {
        let config = CoreConfig::default();
        let mut core = ChipsCore::new(config).await.unwrap();
        core.start().await.unwrap();

        // 路由到不存在的 action
        let result = core
            .route_action("test", "nonexistent.action", json!({}))
            .await;
        assert!(result.is_err());

        core.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_event_subscribe_publish() {
        let config = CoreConfig::default();
        let mut core = ChipsCore::new(config).await.unwrap();
        core.start().await.unwrap();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // 订阅事件
        let sub_id = core
            .subscribe(
                "test_subscriber",
                "test.event",
                Arc::new(move |_event| {
                    counter_clone.fetch_add(1, Ordering::SeqCst);
                }),
            )
            .await
            .unwrap();

        // 发布事件
        let event = Event::new("test.event", "test_sender", json!({}));
        let count = core.publish(event).await.unwrap();
        assert_eq!(count, 1);

        // 等待异步处理
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // 取消订阅
        core.unsubscribe(&sub_id).await.unwrap();

        // 再次发布，不应增加计数
        let event = Event::new("test.event", "test_sender", json!({}));
        let count = core.publish(event).await.unwrap();
        assert_eq!(count, 0);

        core.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_publish_event_convenience() {
        let config = CoreConfig::default();
        let mut core = ChipsCore::new(config).await.unwrap();
        core.start().await.unwrap();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        core.subscribe(
            "test_subscriber",
            "custom.*",
            Arc::new(move |_| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .await
        .unwrap();

        // 使用便捷方法发布
        core.publish_event("custom.test", "sender", json!({"key": "value"}))
            .await
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        core.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_health_info() {
        let config = CoreConfig::default();
        let mut core = ChipsCore::new(config).await.unwrap();
        core.start().await.unwrap();

        let health = core.health().await;
        assert_eq!(health.state, CoreState::Running);
        assert!(health.router_running);
        assert!(health.uptime_secs.is_some());

        core.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_config_api() {
        let config = CoreConfig::default();
        let core = ChipsCore::new(config).await.unwrap();

        // 设置配置
        core.set_config("test.key", "value").await.unwrap();

        // 获取配置
        let value: Option<String> = core.get_config("test.key").await;
        assert_eq!(value, Some("value".to_string()));

        // 获取不存在的配置（带默认值）
        let value: String = core.get_config_or("nonexistent", "default".to_string()).await;
        assert_eq!(value, "default");
    }

    #[tokio::test]
    async fn test_metrics_collection() {
        let config = CoreConfig::default();
        let mut core = ChipsCore::new(config).await.unwrap();
        core.start().await.unwrap();

        // 发送几个请求
        for _ in 0..5 {
            let request = RouteRequest::new("test", "test.action", json!({}));
            let _ = core.route(request).await;
        }

        // 检查指标
        let report = core.metrics().export();
        assert_eq!(report.route_metrics.total_requests, 5);

        core.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_core_state_transitions() {
        let state = CoreState::Uninitialized;
        assert!(!state.can_start());
        assert!(!state.can_shutdown());
        assert!(!state.is_running());

        let state = CoreState::Initialized;
        assert!(state.can_start());
        assert!(!state.can_shutdown());
        assert!(!state.is_running());

        let state = CoreState::Running;
        assert!(!state.can_start());
        assert!(state.can_shutdown());
        assert!(state.is_running());

        let state = CoreState::Shutdown;
        assert!(!state.can_start());
        assert!(!state.can_shutdown());
        assert!(!state.is_running());
    }

    #[tokio::test]
    async fn test_subscribe_with_filter() {
        let config = CoreConfig::default();
        let mut core = ChipsCore::new(config).await.unwrap();
        core.start().await.unwrap();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // 订阅带过滤器
        let filter = EventFilter::by_sender("allowed_sender");
        core.subscribe_with_filter(
            "test_subscriber",
            "test.event",
            filter,
            Arc::new(move |_| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .await
        .unwrap();

        // 发布匹配的事件
        let event = Event::new("test.event", "allowed_sender", json!({}));
        core.publish(event).await.unwrap();

        // 发布不匹配的事件
        let event = Event::new("test.event", "other_sender", json!({}));
        core.publish(event).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        core.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_unsubscribe_all() {
        let config = CoreConfig::default();
        let mut core = ChipsCore::new(config).await.unwrap();
        core.start().await.unwrap();

        // 多次订阅
        core.subscribe("module_a", "event1", Arc::new(|_| {}))
            .await
            .unwrap();
        core.subscribe("module_a", "event2", Arc::new(|_| {}))
            .await
            .unwrap();
        core.subscribe("module_b", "event1", Arc::new(|_| {}))
            .await
            .unwrap();

        // 取消 module_a 的所有订阅
        let count = core.unsubscribe_all("module_a").await.unwrap();
        assert_eq!(count, 2);

        core.shutdown().await.unwrap();
    }
}
