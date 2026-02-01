//! # 端到端集成测试
//!
//! 测试薯片微内核的完整工作流程，包括：
//! - 内核启动 → 模块加载 → 路由请求 → 响应返回
//! - 事件发布 → 订阅接收
//! - 配置变更测试
//! - 错误场景（模块加载失败、路由超时）
//! - 边界情况（并发请求）

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chips_core::router::{
    Event, EventFilter, ModuleHandler, RouteEntry, RouteRequest, RouteResponse, Router,
    RouterConfig,
};
use chips_core::{
    status_code, ChipsCore, CoreConfig, CoreState,
};
use serde_json::json;

// ============================================================================
// 测试辅助结构
// ============================================================================

/// 模拟模块处理器 - 用于测试路由功能
struct MockModuleHandler {
    id: String,
    delay_ms: u64,
    should_fail: bool,
}

impl MockModuleHandler {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            delay_ms: 0,
            should_fail: false,
        }
    }

    fn with_delay(id: &str, delay_ms: u64) -> Self {
        Self {
            id: id.to_string(),
            delay_ms,
            should_fail: false,
        }
    }

    fn failing(id: &str) -> Self {
        Self {
            id: id.to_string(),
            delay_ms: 0,
            should_fail: true,
        }
    }
}

#[async_trait::async_trait]
impl ModuleHandler for MockModuleHandler {
    async fn handle(&self, request: RouteRequest) -> RouteResponse {
        // 模拟延迟
        if self.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        }

        // 模拟失败
        if self.should_fail {
            return RouteResponse::error(
                &request.request_id,
                status_code::INTERNAL_ERROR,
                chips_core::router::ErrorInfo::new("MODULE_ERROR", "模块处理失败"),
                self.delay_ms,
            );
        }

        // 正常响应
        RouteResponse::success(
            &request.request_id,
            json!({
                "module_id": self.id,
                "action": request.action,
                "params": request.params,
                "processed": true
            }),
            self.delay_ms,
        )
    }

    fn module_id(&self) -> &str {
        &self.id
    }
}

// ============================================================================
// 工作流测试：内核启动 → 模块加载 → 路由请求 → 响应返回
// ============================================================================

/// 测试完整的内核生命周期
#[tokio::test]
async fn test_e2e_kernel_lifecycle() {
    // 1. 创建内核配置
    let config = CoreConfig::builder()
        .worker_count(4)
        .max_concurrent(1000)
        .log_level("warn")
        .build();

    // 2. 创建内核实例
    let mut core = ChipsCore::new(config).await.unwrap();
    assert_eq!(core.state().await, CoreState::Initialized);

    // 3. 启动内核
    core.start().await.unwrap();
    assert_eq!(core.state().await, CoreState::Running);
    assert!(core.is_running().await);

    // 4. 验证内核健康状态
    let health = core.health().await;
    assert!(health.router_running);
    assert!(health.uptime_secs.is_some());

    // 5. 关闭内核
    core.shutdown().await.unwrap();
    assert_eq!(core.state().await, CoreState::Shutdown);
    assert!(!core.is_running().await);
}

/// 测试完整的路由工作流程
#[tokio::test]
async fn test_e2e_complete_routing_flow() {
    // 1. 创建并启动路由器
    let config = RouterConfig {
        worker_count: 4,
        max_concurrent: 100,
        queue_size: 1000,
        default_timeout_ms: 5000,
        enable_validation: true,
    };
    let router = Router::new(config);

    // 2. 注册模块处理器
    let handler = Arc::new(MockModuleHandler::new("user_service"));
    router.register_handler(handler).await.unwrap();

    // 3. 注册路由
    router
        .register_route("user.get", RouteEntry::exact("user_service", "user.get"))
        .await
        .unwrap();
    router
        .register_route("user.create", RouteEntry::exact("user_service", "user.create"))
        .await
        .unwrap();

    // 4. 启动路由器
    router.start().await.unwrap();
    assert!(router.is_running());

    // 5. 发送路由请求
    let request = RouteRequest::new(
        "client_app",
        "user.get",
        json!({"user_id": "12345"}),
    );
    let response = router.route(request).await;

    // 6. 验证响应
    assert!(response.is_success());
    assert_eq!(response.status, status_code::OK);

    let data = response.data.unwrap();
    assert_eq!(data["module_id"], "user_service");
    assert_eq!(data["action"], "user.get");
    assert!(data["processed"].as_bool().unwrap());

    // 7. 验证统计信息
    let stats = router.stats();
    assert!(stats.total_requests >= 1);
    assert!(stats.successful_requests >= 1);

    // 8. 停止路由器
    router.stop().await.unwrap();
    assert!(!router.is_running());
}

/// 测试内核路由 API
#[tokio::test]
async fn test_e2e_core_routing_api() {
    let config = CoreConfig::default();
    let mut core = ChipsCore::new(config).await.unwrap();
    core.start().await.unwrap();

    // 注册处理器和路由
    let handler = Arc::new(MockModuleHandler::new("test_module"));
    core.router().register_handler(handler).await.unwrap();
    core.router()
        .register_route("test.action", RouteEntry::exact("test_module", "test.action"))
        .await
        .unwrap();

    // 使用便捷 API 发送请求
    let result = core
        .route_action("client", "test.action", json!({"key": "value"}))
        .await;

    assert!(result.is_ok());
    let data = result.unwrap();
    assert_eq!(data["module_id"], "test_module");

    core.shutdown().await.unwrap();
}

// ============================================================================
// 事件系统测试：事件发布 → 订阅接收
// ============================================================================

/// 测试完整的事件发布订阅流程
#[tokio::test]
async fn test_e2e_event_publish_subscribe() {
    let config = CoreConfig::default();
    let mut core = ChipsCore::new(config).await.unwrap();
    core.start().await.unwrap();

    let received_count = Arc::new(AtomicUsize::new(0));
    let received_clone = received_count.clone();

    // 订阅事件
    let sub_id = core
        .subscribe(
            "test_subscriber",
            "user.created",
            Arc::new(move |event| {
                // 验证事件数据
                assert_eq!(event.event_type, "user.created");
                received_clone.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .await
        .unwrap();

    assert!(!sub_id.is_empty());

    // 发布事件
    let event = Event::new(
        "user.created",
        "user_service",
        json!({"user_id": "123", "username": "testuser"}),
    );
    let delivery_count = core.publish(event).await.unwrap();
    assert_eq!(delivery_count, 1);

    // 等待异步处理完成
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(received_count.load(Ordering::SeqCst) >= 1);

    // 取消订阅
    core.unsubscribe(&sub_id).await.unwrap();

    // 验证取消订阅后不再接收
    let event2 = Event::new("user.created", "user_service", json!({}));
    let delivery_count2 = core.publish(event2).await.unwrap();
    assert_eq!(delivery_count2, 0);

    core.shutdown().await.unwrap();
}

/// 测试通配符事件订阅
#[tokio::test]
async fn test_e2e_wildcard_event_subscription() {
    let config = CoreConfig::default();
    let mut core = ChipsCore::new(config).await.unwrap();
    core.start().await.unwrap();

    let received_count = Arc::new(AtomicUsize::new(0));
    let received_clone = received_count.clone();

    // 订阅通配符事件
    core.subscribe(
        "audit_module",
        "user.*",
        Arc::new(move |_event| {
            received_clone.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    // 发布多种用户相关事件
    for event_type in ["user.created", "user.updated", "user.deleted"] {
        let event = Event::new(event_type, "user_service", json!({}));
        core.publish(event).await.unwrap();
    }

    // 发布不匹配的事件
    let other_event = Event::new("order.created", "order_service", json!({}));
    core.publish(other_event).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    // 应该只收到 3 个用户事件
    assert_eq!(received_count.load(Ordering::SeqCst), 3);

    core.shutdown().await.unwrap();
}

/// 测试带过滤器的事件订阅
#[tokio::test]
async fn test_e2e_event_subscription_with_filter() {
    let config = CoreConfig::default();
    let mut core = ChipsCore::new(config).await.unwrap();
    core.start().await.unwrap();

    let received_count = Arc::new(AtomicUsize::new(0));
    let received_clone = received_count.clone();

    // 订阅带过滤器的事件（只接收特定发送者的事件）
    let filter = EventFilter::by_sender("authorized_service");
    core.subscribe_with_filter(
        "security_module",
        "sensitive.event",
        filter,
        Arc::new(move |_event| {
            received_clone.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    // 发布匹配的事件
    let matched_event = Event::new("sensitive.event", "authorized_service", json!({}));
    core.publish(matched_event).await.unwrap();

    // 发布不匹配的事件
    let unmatched_event = Event::new("sensitive.event", "unknown_service", json!({}));
    core.publish(unmatched_event).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    // 只应收到一个匹配的事件
    assert_eq!(received_count.load(Ordering::SeqCst), 1);

    core.shutdown().await.unwrap();
}

/// 测试同步事件发布
#[tokio::test]
async fn test_e2e_sync_event_publish() {
    let config = CoreConfig::default();
    let mut core = ChipsCore::new(config).await.unwrap();
    core.start().await.unwrap();

    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    // 订阅事件
    core.subscribe(
        "sync_handler",
        "sync.event",
        Arc::new(move |_| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    // 同步发布事件
    let event = Event::new("sync.event", "test", json!({}));
    let (successful, failed, timeouts) = core.publish_sync(event).await.unwrap();

    // 同步发布应立即完成
    assert_eq!(successful, 1);
    assert_eq!(failed, 0);
    assert_eq!(timeouts, 0);
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    core.shutdown().await.unwrap();
}

// ============================================================================
// 配置变更测试
// ============================================================================

/// 测试配置读写
#[tokio::test]
async fn test_e2e_config_operations() {
    let config = CoreConfig::default();
    let core = ChipsCore::new(config).await.unwrap();

    // 设置配置
    core.set_config("app.name", "ChipsCore").await.unwrap();
    core.set_config("app.version", "1.0.0").await.unwrap();
    core.set_config("feature.enabled", true).await.unwrap();
    core.set_config("limits.max_connections", 1000)
        .await
        .unwrap();

    // 读取配置
    let name: Option<String> = core.get_config("app.name").await;
    assert_eq!(name, Some("ChipsCore".to_string()));

    let version: Option<String> = core.get_config("app.version").await;
    assert_eq!(version, Some("1.0.0".to_string()));

    let enabled: Option<bool> = core.get_config("feature.enabled").await;
    assert_eq!(enabled, Some(true));

    // 使用默认值读取
    let max_conn: i64 = core.get_config_or("limits.max_connections", 100).await;
    assert_eq!(max_conn, 1000);

    let missing: String = core.get_config_or("nonexistent.key", "default".to_string()).await;
    assert_eq!(missing, "default");
}

/// 测试复杂配置结构
#[tokio::test]
async fn test_e2e_complex_config() {
    let config = CoreConfig::default();
    let core = ChipsCore::new(config).await.unwrap();

    // 设置嵌套配置
    core.set_config(
        "database",
        json!({
            "host": "localhost",
            "port": 5432,
            "credentials": {
                "username": "admin",
                "password": "secret"
            }
        }),
    )
    .await
    .unwrap();

    // 读取嵌套值
    let db_config: Option<serde_json::Value> = core.get_config("database").await;
    assert!(db_config.is_some());

    let db = db_config.unwrap();
    assert_eq!(db["host"], "localhost");
    assert_eq!(db["port"], 5432);
}

// ============================================================================
// 错误场景测试
// ============================================================================

/// 测试路由不存在的处理
#[tokio::test]
async fn test_e2e_route_not_found() {
    let config = CoreConfig::default();
    let mut core = ChipsCore::new(config).await.unwrap();
    core.start().await.unwrap();

    // 请求不存在的路由
    let request = RouteRequest::new("client", "nonexistent.action", json!({}));
    let response = core.route(request).await;

    assert!(response.is_error());
    assert_eq!(response.status, status_code::NOT_FOUND);

    core.shutdown().await.unwrap();
}

/// 测试请求验证失败
#[tokio::test]
async fn test_e2e_invalid_request() {
    let router = Router::with_defaults();
    router.start().await.unwrap();

    // 创建无效请求（空 action）
    let mut request = RouteRequest::new("test", "valid.action", json!({}));
    request.action = String::new();

    let response = router.route(request).await;

    assert!(response.is_error());
    assert_eq!(response.status, status_code::BAD_REQUEST);

    router.stop().await.unwrap();
}

/// 测试路由超时处理
#[tokio::test]
async fn test_e2e_route_timeout() {
    let config = RouterConfig {
        worker_count: 2,
        max_concurrent: 100,
        queue_size: 1000,
        default_timeout_ms: 100, // 100ms 超时
        enable_validation: true,
    };
    let router = Router::new(config);

    // 注册一个慢处理器
    let slow_handler = Arc::new(MockModuleHandler::with_delay("slow_module", 500));
    router.register_handler(slow_handler).await.unwrap();
    router
        .register_route("slow.action", RouteEntry::exact("slow_module", "slow.action"))
        .await
        .unwrap();

    router.start().await.unwrap();

    // 发送请求（应该超时）
    let mut request = RouteRequest::new("client", "slow.action", json!({}));
    request.timeout_ms = 100;

    let response = router.route(request).await;

    assert!(response.is_error());
    assert_eq!(response.status, status_code::TIMEOUT);

    router.stop().await.unwrap();
}

/// 测试模块处理失败
#[tokio::test]
async fn test_e2e_module_handler_error() {
    let router = Router::with_defaults();

    // 注册一个会失败的处理器
    let failing_handler = Arc::new(MockModuleHandler::failing("failing_module"));
    router.register_handler(failing_handler).await.unwrap();
    router
        .register_route(
            "fail.action",
            RouteEntry::exact("failing_module", "fail.action"),
        )
        .await
        .unwrap();

    router.start().await.unwrap();

    let request = RouteRequest::new("client", "fail.action", json!({}));
    let response = router.route(request).await;

    assert!(response.is_error());
    assert_eq!(response.status, status_code::INTERNAL_ERROR);

    router.stop().await.unwrap();
}

/// 测试内核未启动时的路由请求
#[tokio::test]
async fn test_e2e_route_before_start() {
    let config = CoreConfig::default();
    let core = ChipsCore::new(config).await.unwrap();

    // 内核未启动时发送请求
    let request = RouteRequest::new("client", "test.action", json!({}));
    let response = core.route(request).await;

    assert!(response.is_error());
    assert_eq!(response.status, status_code::SERVICE_UNAVAILABLE);
}

/// 测试处理器注销后的请求
#[tokio::test]
async fn test_e2e_handler_unregistration() {
    let router = Router::with_defaults();

    let handler = Arc::new(MockModuleHandler::new("temp_module"));
    router.register_handler(handler).await.unwrap();
    router
        .register_route("temp.action", RouteEntry::exact("temp_module", "temp.action"))
        .await
        .unwrap();

    router.start().await.unwrap();

    // 第一次请求应成功
    let request1 = RouteRequest::new("client", "temp.action", json!({}));
    let response1 = router.route(request1).await;
    assert!(response1.is_success());

    // 注销处理器
    router.unregister_handler("temp_module").await.unwrap();

    // 第二次请求应失败
    let request2 = RouteRequest::new("client", "temp.action", json!({}));
    let response2 = router.route(request2).await;
    assert!(response2.is_error());

    router.stop().await.unwrap();
}

// ============================================================================
// 边界情况测试：并发请求
// ============================================================================

/// 测试高并发路由请求
#[tokio::test]
async fn test_e2e_concurrent_requests() {
    let config = RouterConfig {
        worker_count: 8,
        max_concurrent: 500,
        queue_size: 10000,
        default_timeout_ms: 5000,
        enable_validation: true,
    };
    let router = Arc::new(Router::new(config));

    let handler = Arc::new(MockModuleHandler::new("concurrent_module"));
    router.register_handler(handler).await.unwrap();
    router
        .register_route(
            "concurrent.test",
            RouteEntry::exact("concurrent_module", "concurrent.test"),
        )
        .await
        .unwrap();

    router.start().await.unwrap();

    let request_count = 200;
    let mut handles = Vec::with_capacity(request_count);

    // 并发发送请求
    for i in 0..request_count {
        let router_clone = Arc::clone(&router);
        handles.push(tokio::spawn(async move {
            let request = RouteRequest::new(
                &format!("client_{}", i),
                "concurrent.test",
                json!({"request_id": i}),
            );
            router_clone.route(request).await
        }));
    }

    // 等待所有请求完成
    let mut success_count = 0;
    for handle in handles {
        let response = handle.await.unwrap();
        if response.is_success() {
            success_count += 1;
        }
    }

    // 所有请求应该成功
    assert_eq!(success_count, request_count);

    // 验证统计
    let stats = router.stats();
    assert_eq!(stats.total_requests, request_count as u64);
    assert_eq!(stats.successful_requests, request_count as u64);

    router.stop().await.unwrap();
}

/// 测试并发事件发布
#[tokio::test]
async fn test_e2e_concurrent_event_publishing() {
    let config = CoreConfig::default();
    let mut core = ChipsCore::new(config).await.unwrap();
    core.start().await.unwrap();

    let received_count = Arc::new(AtomicUsize::new(0));
    let received_clone = received_count.clone();

    // 订阅事件
    core.subscribe(
        "counter",
        "concurrent.event",
        Arc::new(move |_| {
            received_clone.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    let core = Arc::new(tokio::sync::RwLock::new(core));
    let event_count = 100;
    let mut handles = Vec::with_capacity(event_count);

    // 并发发布事件
    for i in 0..event_count {
        let core_clone = Arc::clone(&core);
        handles.push(tokio::spawn(async move {
            let core_guard = core_clone.read().await;
            let event = Event::new(
                "concurrent.event",
                &format!("publisher_{}", i),
                json!({"index": i}),
            );
            core_guard.publish(event).await
        }));
    }

    // 等待所有发布完成
    for handle in handles {
        let _ = handle.await.unwrap();
    }

    // 等待事件处理完成
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 应该收到大部分事件
    let received = received_count.load(Ordering::SeqCst);
    assert!(received >= event_count / 2, "收到 {} 个事件，预期至少 {}", received, event_count / 2);

    let mut core_guard = core.write().await;
    core_guard.shutdown().await.unwrap();
}

/// 测试压力下的路由稳定性
#[tokio::test]
async fn test_e2e_stress_routing() {
    let config = RouterConfig {
        worker_count: 4,
        max_concurrent: 200,
        queue_size: 5000,
        default_timeout_ms: 5000,
        enable_validation: true,
    };
    let router = Arc::new(Router::new(config));

    // 注册多个模块 (使用有效的 action 格式: namespace 用连字符)
    for i in 0..5 {
        let module_id = format!("stress{}", i);
        let handler = Arc::new(MockModuleHandler::new(&module_id));
        router.register_handler(handler).await.unwrap();

        for j in 0..3 {
            let action = format!("stress{}.action{}", i, j);
            router
                .register_route(&action, RouteEntry::exact(&module_id, &action))
                .await
                .unwrap();
        }
    }

    router.start().await.unwrap();

    let start = std::time::Instant::now();
    let total_requests = 500;
    let mut handles = Vec::with_capacity(total_requests);

    // 混合请求到不同模块
    for i in 0..total_requests {
        let router_clone = Arc::clone(&router);
        let module_idx = i % 5;
        let action_idx = i % 3;
        handles.push(tokio::spawn(async move {
            let action = format!("stress{}.action{}", module_idx, action_idx);
            let request = RouteRequest::new("stress_client", &action, json!({"i": i}));
            router_clone.route(request).await
        }));
    }

    let mut success_count = 0;
    for handle in handles {
        let response = handle.await.unwrap();
        if response.is_success() {
            success_count += 1;
        }
    }

    let elapsed = start.elapsed();

    // 所有请求应该成功
    assert_eq!(success_count, total_requests);

    // 应该在合理时间内完成
    assert!(
        elapsed.as_secs() < 5,
        "压力测试耗时过长: {:?}",
        elapsed
    );

    let stats = router.stats();
    assert_eq!(stats.total_requests, total_requests as u64);

    router.stop().await.unwrap();
}

/// 测试空路由表情况
#[tokio::test]
async fn test_e2e_empty_route_table() {
    let router = Router::with_defaults();
    router.start().await.unwrap();

    // 对空路由表发送多个请求（使用有效的 action 格式: namespace.action）
    for i in 0..10 {
        let request = RouteRequest::new("client", &format!("unknown.action_{}", i), json!({}));
        let response = router.route(request).await;
        assert!(response.is_error());
        assert_eq!(response.status, status_code::NOT_FOUND);
    }

    router.stop().await.unwrap();
}

/// 测试多订阅者场景
#[tokio::test]
async fn test_e2e_multiple_subscribers() {
    let config = CoreConfig::default();
    let mut core = ChipsCore::new(config).await.unwrap();
    core.start().await.unwrap();

    let counter1 = Arc::new(AtomicUsize::new(0));
    let counter2 = Arc::new(AtomicUsize::new(0));
    let counter3 = Arc::new(AtomicUsize::new(0));

    let c1 = counter1.clone();
    let c2 = counter2.clone();
    let c3 = counter3.clone();

    // 多个订阅者订阅同一事件
    core.subscribe("sub1", "broadcast.event", Arc::new(move |_| {
        c1.fetch_add(1, Ordering::SeqCst);
    }))
    .await
    .unwrap();

    core.subscribe("sub2", "broadcast.event", Arc::new(move |_| {
        c2.fetch_add(1, Ordering::SeqCst);
    }))
    .await
    .unwrap();

    core.subscribe("sub3", "broadcast.event", Arc::new(move |_| {
        c3.fetch_add(1, Ordering::SeqCst);
    }))
    .await
    .unwrap();

    // 发布事件
    let event = Event::new("broadcast.event", "broadcaster", json!({}));
    let (successful, failed, _) = core.publish_sync(event).await.unwrap();

    assert_eq!(successful, 3);
    assert_eq!(failed, 0);
    assert_eq!(counter1.load(Ordering::SeqCst), 1);
    assert_eq!(counter2.load(Ordering::SeqCst), 1);
    assert_eq!(counter3.load(Ordering::SeqCst), 1);

    core.shutdown().await.unwrap();
}

/// 测试批量取消订阅
#[tokio::test]
async fn test_e2e_unsubscribe_all() {
    let config = CoreConfig::default();
    let mut core = ChipsCore::new(config).await.unwrap();
    core.start().await.unwrap();

    // 为同一模块订阅多个事件
    core.subscribe("module_a", "event.type1", Arc::new(|_| {}))
        .await
        .unwrap();
    core.subscribe("module_a", "event.type2", Arc::new(|_| {}))
        .await
        .unwrap();
    core.subscribe("module_a", "event.type3", Arc::new(|_| {}))
        .await
        .unwrap();
    core.subscribe("module_b", "event.type1", Arc::new(|_| {}))
        .await
        .unwrap();

    // 批量取消 module_a 的所有订阅
    let removed = core.unsubscribe_all("module_a").await.unwrap();
    assert_eq!(removed, 3);

    // 验证 module_b 的订阅仍然存在
    let event = Event::new("event.type1", "test", json!({}));
    let delivery_count = core.publish(event).await.unwrap();
    assert_eq!(delivery_count, 1); // 只有 module_b 订阅

    core.shutdown().await.unwrap();
}

/// 测试路由器统计重置
#[tokio::test]
async fn test_e2e_stats_reset() {
    let router = Router::with_defaults();

    let handler = Arc::new(MockModuleHandler::new("stats_module"));
    router.register_handler(handler).await.unwrap();
    router
        .register_route("stats.action", RouteEntry::exact("stats_module", "stats.action"))
        .await
        .unwrap();

    router.start().await.unwrap();

    // 发送一些请求
    for _ in 0..10 {
        let request = RouteRequest::new("client", "stats.action", json!({}));
        let _ = router.route(request).await;
    }

    // 验证统计
    let stats = router.stats();
    assert_eq!(stats.total_requests, 10);

    // 重置统计
    router.reset_stats();

    // 验证重置
    let stats_after = router.stats();
    assert_eq!(stats_after.total_requests, 0);

    router.stop().await.unwrap();
}

// ============================================================================
// 综合测试场景
// ============================================================================

/// 测试完整的业务场景：用户注册流程
#[tokio::test]
async fn test_e2e_user_registration_scenario() {
    let config = CoreConfig::default();
    let mut core = ChipsCore::new(config).await.unwrap();
    core.start().await.unwrap();

    // 注册用户服务处理器
    let user_handler = Arc::new(MockModuleHandler::new("user_service"));
    core.router().register_handler(user_handler).await.unwrap();
    core.router()
        .register_route("user.register", RouteEntry::exact("user_service", "user.register"))
        .await
        .unwrap();

    // 设置事件监听：邮件服务
    let email_sent = Arc::new(AtomicUsize::new(0));
    let email_clone = email_sent.clone();
    core.subscribe(
        "email_service",
        "user.registered",
        Arc::new(move |_event| {
            // 模拟发送欢迎邮件
            email_clone.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    // 设置事件监听：审计日志
    let audit_logged = Arc::new(AtomicUsize::new(0));
    let audit_clone = audit_logged.clone();
    core.subscribe(
        "audit_service",
        "user.registered",
        Arc::new(move |_event| {
            // 模拟记录审计日志
            audit_clone.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    // 步骤 1: 发送注册请求
    let register_request = RouteRequest::new(
        "web_client",
        "user.register",
        json!({
            "username": "newuser",
            "email": "newuser@example.com",
            "password": "secure123"
        }),
    );
    let response = core.route(register_request).await;
    assert!(response.is_success());

    // 步骤 2: 发布用户注册成功事件
    let event = Event::new(
        "user.registered",
        "user_service",
        json!({
            "user_id": "new_user_123",
            "username": "newuser"
        }),
    );
    let (successful, _, _) = core.publish_sync(event).await.unwrap();
    assert_eq!(successful, 2); // 邮件服务 + 审计服务

    // 验证各服务已响应
    assert_eq!(email_sent.load(Ordering::SeqCst), 1);
    assert_eq!(audit_logged.load(Ordering::SeqCst), 1);

    core.shutdown().await.unwrap();
}

/// 测试内核健康检查
#[tokio::test]
async fn test_e2e_health_check() {
    let config = CoreConfig::default();
    let mut core = ChipsCore::new(config).await.unwrap();
    core.start().await.unwrap();

    // 注册处理器并发送一些请求
    let handler = Arc::new(MockModuleHandler::new("health_module"));
    core.router().register_handler(handler).await.unwrap();
    core.router()
        .register_route("health.ping", RouteEntry::exact("health_module", "health.ping"))
        .await
        .unwrap();

    for _ in 0..5 {
        let request = RouteRequest::new("client", "health.ping", json!({}));
        let _ = core.route(request).await;
    }

    // 检查健康状态
    let health = core.health().await;

    assert_eq!(health.state, CoreState::Running);
    assert!(health.router_running);
    assert!(health.uptime_secs.is_some());
    assert_eq!(health.total_requests, 5);
    assert!(health.success_rate > 0.0);

    core.shutdown().await.unwrap();
}
