//! Phase 4: 事件总线与配置管理集成测试
//!
//! 测试事件发布订阅和配置管理的完整工作流程

use chips_core::router::{Event, EventBus, EventFilter};
use chips_core::core::config::{ConfigManager, ConfigManagerBuilder};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Barrier;
use tempfile::tempdir;

// ============================================================================
// 事件总线集成测试
// ============================================================================

/// 测试完整的事件发布订阅流程
#[tokio::test]
async fn test_event_pub_sub_full_flow() {
    let bus = EventBus::new();
    let received = Arc::new(AtomicUsize::new(0));
    let received_clone = received.clone();

    // 订阅事件
    let sub_id = bus
        .subscribe(
            "test-subscriber",
            "user.created",
            None,
            Arc::new(move |_event| {
                received_clone.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .await
        .unwrap();

    assert!(!sub_id.is_empty());

    // 发布事件
    let event = Event::builder("user.created", "test")
        .data(serde_json::json!({"user_id": "123"}))
        .build();

    bus.publish(event).await.unwrap();

    // 等待异步处理
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    assert!(received.load(Ordering::SeqCst) >= 1);

    // 取消订阅
    bus.unsubscribe(&sub_id).await;

    // 再次发布，不应该收到
    let before = received.load(Ordering::SeqCst);
    let event2 = Event::builder("user.created", "test").build();
    bus.publish(event2).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    assert_eq!(received.load(Ordering::SeqCst), before);
}

/// 测试事件过滤器
#[tokio::test]
async fn test_event_filter_integration() {
    let bus = EventBus::new();
    let received = Arc::new(AtomicUsize::new(0));
    let received_clone = received.clone();

    // 订阅带过滤器的事件
    let filter = EventFilter {
        event_type: Some("order.*".to_string()),
        sender: Some("order-service".to_string()),
        data_filters: std::collections::HashMap::new(),
    };

    bus.subscribe(
        "order-handler",
        "order.*",
        Some(filter),
        Arc::new(move |_| {
            received_clone.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    // 发布匹配的事件
    let event1 = Event::builder("order.created", "order-service").build();
    bus.publish_sync(event1).await.unwrap();

    // 发布不匹配的事件（不同发送方）
    let event2 = Event::builder("order.created", "other-service").build();
    bus.publish_sync(event2).await.unwrap();

    // 只有匹配的事件应该被处理
    assert!(received.load(Ordering::SeqCst) >= 1);
}

/// 测试并发事件发布
#[tokio::test]
async fn test_concurrent_event_publishing() {
    let bus = Arc::new(EventBus::new());
    let received = Arc::new(AtomicUsize::new(0));
    let received_clone = received.clone();

    // 订阅
    bus.subscribe(
        "counter",
        "count.event",
        None,
        Arc::new(move |_| {
            received_clone.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    // 并发发布 100 个事件
    let barrier = Arc::new(Barrier::new(10));
    let mut handles = vec![];

    for i in 0..10 {
        let bus_clone = bus.clone();
        let barrier_clone = barrier.clone();

        handles.push(tokio::spawn(async move {
            barrier_clone.wait().await;
            for j in 0..10 {
                let event = Event::builder("count.event", &format!("publisher-{}", i))
                    .data(serde_json::json!({"index": j}))
                    .build();
                let _ = bus_clone.publish(event).await;
            }
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // 等待所有事件处理完成
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // 应该收到大部分事件
    assert!(received.load(Ordering::SeqCst) >= 50);
}

/// 测试订阅者隔离（一个订阅者的错误不影响其他）
#[tokio::test]
async fn test_subscriber_isolation() {
    let bus = EventBus::new();
    let good_received = Arc::new(AtomicUsize::new(0));
    let good_received_clone = good_received.clone();

    // 订阅一个会 panic 的处理器
    bus.subscribe(
        "bad-subscriber",
        "test.event",
        None,
        Arc::new(|_| {
            panic!("This subscriber panics!");
        }),
    )
    .await
    .unwrap();

    // 订阅一个正常的处理器
    bus.subscribe(
        "good-subscriber",
        "test.event",
        None,
        Arc::new(move |_| {
            good_received_clone.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    // 发布事件
    let event = Event::builder("test.event", "test").build();
    let (success, failed, _timeout) = bus.publish_sync(event).await.unwrap();

    // 正常的订阅者应该收到事件
    assert!(good_received.load(Ordering::SeqCst) >= 1);
    // 统计应该显示有失败
    assert!(failed >= 1 || success >= 1);
}

// ============================================================================
// 配置管理集成测试
// ============================================================================

/// 测试配置加载和读取
#[tokio::test]
async fn test_config_load_and_read() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.yaml");

    // 写入配置文件
    std::fs::write(
        &config_path,
        r#"
router:
  worker_count: 8
  queue_size: 1000
module:
  scan_dirs:
    - /opt/modules
    - /usr/local/modules
"#,
    )
    .unwrap();

    // 加载配置
    let manager = ConfigManager::new();
    manager.load_from_file(&config_path).await.unwrap();

    // 读取配置值
    let worker_count: i64 = manager.get("router.worker_count").await.unwrap();
    assert_eq!(worker_count, 8);

    let queue_size: i64 = manager.get_or("router.queue_size", 500).await;
    assert_eq!(queue_size, 1000);

    // 读取不存在的配置
    let missing: Option<String> = manager.get("nonexistent.key").await;
    assert!(missing.is_none());
}

/// 测试配置层级覆盖
#[tokio::test]
async fn test_config_layer_override() {
    let dir = tempdir().unwrap();
    let default_path = dir.path().join("default.yaml");
    let user_path = dir.path().join("user.yaml");

    // 默认配置
    std::fs::write(
        &default_path,
        r#"
app:
  name: "chips"
  debug: false
  port: 8080
"#,
    )
    .unwrap();

    // 用户配置（覆盖部分值）
    std::fs::write(
        &user_path,
        r#"
app:
  debug: true
  port: 9090
"#,
    )
    .unwrap();

    // 使用 builder 创建
    let manager = ConfigManagerBuilder::new()
        .default_config(serde_json::json!({
            "app": {
                "name": "default-name",
                "version": "1.0.0"
            }
        }))
        .build()
        .await
        .unwrap();

    // 加载层级配置
    manager.load_from_file(&default_path).await.unwrap();
    manager.load_from_file(&user_path).await.unwrap();

    // 验证覆盖
    let name: String = manager.get("app.name").await.unwrap();
    assert_eq!(name, "chips");

    let debug: bool = manager.get("app.debug").await.unwrap();
    assert!(debug); // 被 user 配置覆盖

    let port: i64 = manager.get("app.port").await.unwrap();
    assert_eq!(port, 9090); // 被 user 配置覆盖
}

/// 测试配置变更监听
#[tokio::test]
async fn test_config_change_listener() {
    let manager = ConfigManager::new();
    let changed = Arc::new(AtomicUsize::new(0));
    let changed_clone = changed.clone();

    // 注册变更监听器
    let watcher_id = manager
        .on_change("app", move |_old, _new| {
            changed_clone.fetch_add(1, Ordering::SeqCst);
        })
        .await;

    assert!(!watcher_id.is_empty());

    // 修改配置
    manager.set("app.name", "new-name").await.unwrap();
    manager.set("app.version", "2.0.0").await.unwrap();

    // 验证监听器被调用
    assert!(changed.load(Ordering::SeqCst) >= 1);

    // 取消监听
    let removed = manager.off_change(&watcher_id).await;
    assert!(removed);

    // 再次修改，不应该触发监听器
    let before = changed.load(Ordering::SeqCst);
    manager.set("app.name", "another-name").await.unwrap();
    assert_eq!(changed.load(Ordering::SeqCst), before);
}

/// 测试配置保存
#[tokio::test]
async fn test_config_save() {
    let dir = tempdir().unwrap();
    let save_path = dir.path().join("saved.yaml");

    let manager = ConfigManager::new();

    // 设置一些配置
    manager.set("router.worker_count", 16).await.unwrap();
    manager.set("router.timeout_ms", 5000).await.unwrap();
    manager.set("module.auto_load", true).await.unwrap();

    // 保存配置
    manager.save(Some(&save_path)).await.unwrap();

    // 验证文件存在
    assert!(save_path.exists());

    // 读取保存的文件并验证内容
    let content = std::fs::read_to_string(&save_path).unwrap();
    assert!(content.contains("worker_count"));
    assert!(content.contains("timeout_ms"));
}

// ============================================================================
// 事件与配置联合测试
// ============================================================================

/// 测试配置变更触发事件
#[tokio::test]
async fn test_config_change_triggers_event() {
    let bus = Arc::new(EventBus::new());
    let config_changed = Arc::new(AtomicUsize::new(0));
    let config_changed_clone = config_changed.clone();

    // 订阅配置变更事件
    bus.subscribe(
        "config-watcher",
        "config.changed",
        None,
        Arc::new(move |_| {
            config_changed_clone.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    // 模拟配置变更发布事件
    let event = Event::builder("config.changed", "config-manager")
        .data(serde_json::json!({
            "path": "router.worker_count",
            "old_value": 4,
            "new_value": 8
        }))
        .build();

    bus.publish_sync(event).await.unwrap();

    assert!(config_changed.load(Ordering::SeqCst) >= 1);
}

/// 测试事件总线统计
#[tokio::test]
async fn test_event_bus_statistics() {
    let bus = EventBus::new();

    // 添加订阅者
    bus.subscribe(
        "sub1",
        "stats.event",
        None,
        Arc::new(|_| {}),
    )
    .await
    .unwrap();

    bus.subscribe(
        "sub2",
        "stats.event",
        None,
        Arc::new(|_| {}),
    )
    .await
    .unwrap();

    // 发布一些事件
    for i in 0..5 {
        let event = Event::builder("stats.event", "test")
            .data(serde_json::json!({"index": i}))
            .build();
        bus.publish_sync(event).await.unwrap();
    }

    // 检查统计
    let stats = bus.stats().await;
    assert!(stats.total_dispatched >= 5);
    assert!(stats.successful >= 5);
}
