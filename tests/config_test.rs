//! 配置管理系统集成测试
//!
//! 测试配置管理器的完整工作流程

use chips_core::core::config::{ConfigManager, ConfigManagerBuilder, ConfigSource, CoreConfig};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

/// 测试完整的配置加载流程
#[tokio::test]
async fn test_full_config_loading_workflow() {
    let temp_dir = TempDir::new().unwrap();

    // 创建系统配置
    let system_config_path = temp_dir.path().join("system.yaml");
    tokio::fs::write(
        &system_config_path,
        r#"
router:
  worker_count: 4
  max_concurrent: 500
logging:
  level: info
system_only: true
"#,
    )
    .await
    .unwrap();

    // 创建用户配置
    let user_config_path = temp_dir.path().join("user.yaml");
    tokio::fs::write(
        &user_config_path,
        r#"
router:
  worker_count: 8
logging:
  level: debug
  json_format: true
user_only: true
"#,
    )
    .await
    .unwrap();

    // 默认配置
    let default_config = serde_json::json!({
        "router": {
            "worker_count": 2,
            "queue_size": 10000
        },
        "default_only": true
    });

    // 使用构建器创建配置管理器
    let manager = ConfigManagerBuilder::new()
        .env_prefix("TEST")
        .default_config(default_config)
        .system_path(&system_config_path)
        .user_path(&user_config_path)
        .build()
        .await
        .unwrap();

    // 验证层级覆盖
    // router.worker_count: 默认 2 -> 系统 4 -> 用户 8
    assert_eq!(manager.get::<usize>("router.worker_count").await, Some(8));

    // router.max_concurrent: 只在系统配置中设置
    assert_eq!(
        manager.get::<usize>("router.max_concurrent").await,
        Some(500)
    );

    // router.queue_size: 只在默认配置中设置
    assert_eq!(manager.get::<usize>("router.queue_size").await, Some(10000));

    // logging.level: 默认没有 -> 系统 info -> 用户 debug
    assert_eq!(
        manager.get::<String>("logging.level").await,
        Some("debug".to_string())
    );

    // 各层级独有的值
    assert_eq!(manager.get::<bool>("default_only").await, Some(true));
    assert_eq!(manager.get::<bool>("system_only").await, Some(true));
    assert_eq!(manager.get::<bool>("user_only").await, Some(true));
}

/// 测试配置变更监听和通知
#[tokio::test]
async fn test_config_change_notification_workflow() {
    let manager = ConfigManager::new();

    // 跟踪变更次数
    let router_changes = Arc::new(AtomicUsize::new(0));
    let global_changes = Arc::new(AtomicUsize::new(0));

    // 注册路由器配置监听
    let rc = router_changes.clone();
    let router_watcher = manager
        .on_change("router", move |_old, _new| {
            rc.fetch_add(1, Ordering::SeqCst);
        })
        .await;

    // 注册全局监听
    let gc = global_changes.clone();
    let _global_watcher = manager
        .on_change("", move |_old, _new| {
            gc.fetch_add(1, Ordering::SeqCst);
        })
        .await;

    // 修改路由器配置
    manager.set("router.worker_count", 8).await.unwrap();
    assert_eq!(router_changes.load(Ordering::SeqCst), 1);
    assert_eq!(global_changes.load(Ordering::SeqCst), 1);

    // 修改其他配置
    manager.set("logging.level", "debug").await.unwrap();
    assert_eq!(router_changes.load(Ordering::SeqCst), 1); // 不变
    assert_eq!(global_changes.load(Ordering::SeqCst), 2); // 增加

    // 取消路由器监听
    manager.off_change(&router_watcher).await;

    // 再次修改路由器配置
    manager.set("router.worker_count", 16).await.unwrap();
    assert_eq!(router_changes.load(Ordering::SeqCst), 1); // 不变
    assert_eq!(global_changes.load(Ordering::SeqCst), 3); // 增加
}

/// 测试配置持久化
#[tokio::test]
async fn test_config_persistence() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.yaml");

    // 创建并设置配置
    let manager = ConfigManager::new();
    manager.set("router.worker_count", 8).await.unwrap();
    manager.set("logging.level", "debug").await.unwrap();
    manager
        .set("modules.auto_load", vec!["mod1", "mod2"])
        .await
        .unwrap();

    // 保存配置
    manager.save(Some(&config_path)).await.unwrap();

    // 创建新的管理器并加载
    let manager2 = ConfigManager::new();
    manager2.load_from_file(&config_path).await.unwrap();

    // 验证值
    assert_eq!(manager2.get::<usize>("router.worker_count").await, Some(8));
    assert_eq!(
        manager2.get::<String>("logging.level").await,
        Some("debug".to_string())
    );
    assert_eq!(
        manager2.get::<Vec<String>>("modules.auto_load").await,
        Some(vec!["mod1".to_string(), "mod2".to_string()])
    );
}

/// 测试配置验证
#[tokio::test]
async fn test_config_validation() {
    let temp_dir = TempDir::new().unwrap();
    let valid_config = temp_dir.path().join("valid.yaml");
    let invalid_config = temp_dir.path().join("invalid.yaml");

    tokio::fs::write(
        &valid_config,
        r#"
router:
  worker_count: 8
"#,
    )
    .await
    .unwrap();

    tokio::fs::write(
        &invalid_config,
        r#"
router:
  worker_count: 0
"#,
    )
    .await
    .unwrap();

    let manager = ConfigManager::new();

    // 添加验证器
    manager
        .add_validator("router.worker_count", |value| {
            if let Some(count) = value.as_u64() {
                if count > 0 {
                    return Ok(());
                }
            }
            Err(chips_core::CoreError::InvalidConfigValue {
                key: "router.worker_count".to_string(),
                reason: "必须大于 0".to_string(),
            })
        })
        .await;

    // 有效配置应该成功
    assert!(manager.load_from_file(&valid_config).await.is_ok());

    // 清空配置
    let manager2 = ConfigManager::new();
    manager2
        .add_validator("router.worker_count", |value| {
            if let Some(count) = value.as_u64() {
                if count > 0 {
                    return Ok(());
                }
            }
            Err(chips_core::CoreError::InvalidConfigValue {
                key: "router.worker_count".to_string(),
                reason: "必须大于 0".to_string(),
            })
        })
        .await;

    // 无效配置应该失败
    assert!(manager2.load_from_file(&invalid_config).await.is_err());
}

/// 测试热重载
#[tokio::test]
async fn test_hot_reload() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.yaml");

    // 初始配置
    tokio::fs::write(&config_path, "value: 1").await.unwrap();

    let manager = ConfigManager::new();
    manager.load_from_file(&config_path).await.unwrap();

    // 启用热重载
    manager.enable_hot_reload().await.unwrap();
    assert!(manager.is_hot_reload_enabled().await);

    // 修改配置文件
    tokio::fs::write(&config_path, "value: 2").await.unwrap();

    // 手动触发重载
    manager.reload().await.unwrap();

    // 验证新值
    assert_eq!(manager.get::<i32>("value").await, Some(2));

    // 禁用热重载
    manager.disable_hot_reload().await.unwrap();
    assert!(!manager.is_hot_reload_enabled().await);
}

/// 测试 CoreConfig 与 ConfigManager 的互操作
#[tokio::test]
async fn test_core_config_interop() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.yaml");

    // 使用 CoreConfig 创建配置
    let core_config = CoreConfig::builder()
        .worker_count(8)
        .max_concurrent(2000)
        .log_level("debug")
        .dev_mode()
        .build();

    // 序列化为 YAML
    let yaml = serde_yaml::to_string(&core_config).unwrap();
    tokio::fs::write(&config_path, yaml).await.unwrap();

    // 使用 ConfigManager 加载
    let manager = ConfigManager::new();
    manager.load_from_file(&config_path).await.unwrap();

    // 验证值
    assert_eq!(manager.get::<usize>("router.worker_count").await, Some(8));
    assert_eq!(
        manager.get::<usize>("router.max_concurrent").await,
        Some(2000)
    );
    assert_eq!(
        manager.get::<String>("logging.level").await,
        Some("debug".to_string())
    );
    assert_eq!(manager.get::<bool>("dev_mode").await, Some(true));
}

/// 测试深层嵌套配置
#[tokio::test]
async fn test_deep_nested_config() {
    let manager = ConfigManager::new();

    // 设置深层嵌套值
    manager.set("a.b.c.d.e.f", 1).await.unwrap();
    manager.set("a.b.c.d.e.g", 2).await.unwrap();
    manager.set("a.b.c.d.h", 3).await.unwrap();
    manager.set("a.b.i", 4).await.unwrap();

    // 读取深层嵌套值
    assert_eq!(manager.get::<i32>("a.b.c.d.e.f").await, Some(1));
    assert_eq!(manager.get::<i32>("a.b.c.d.e.g").await, Some(2));
    assert_eq!(manager.get::<i32>("a.b.c.d.h").await, Some(3));
    assert_eq!(manager.get::<i32>("a.b.i").await, Some(4));

    // 读取中间节点
    let node: serde_json::Value = manager.get("a.b.c.d.e").await.unwrap();
    assert_eq!(node["f"], 1);
    assert_eq!(node["g"], 2);
}

/// 测试并发读写
#[tokio::test]
async fn test_concurrent_read_write() {
    let manager = Arc::new(ConfigManager::new());
    let mut handles = vec![];

    // 并发写入
    for i in 0..100 {
        let m = manager.clone();
        handles.push(tokio::spawn(async move {
            m.set(&format!("key{}", i), i).await.unwrap();
        }));
    }

    // 等待所有写入完成
    for handle in handles {
        handle.await.unwrap();
    }

    // 并发读取
    let mut read_handles = vec![];
    for i in 0..100 {
        let m = manager.clone();
        read_handles.push(tokio::spawn(async move {
            let value: Option<i32> = m.get(&format!("key{}", i)).await;
            assert_eq!(value, Some(i));
        }));
    }

    for handle in read_handles {
        handle.await.unwrap();
    }
}

/// 测试 JSON 格式配置
#[tokio::test]
async fn test_json_config_format() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.json");

    let json_content = r#"{
        "router": {
            "worker_count": 16,
            "max_concurrent": 3000
        },
        "logging": {
            "level": "trace",
            "json_format": true
        },
        "custom": {
            "array": [1, 2, 3],
            "object": {
                "key": "value"
            }
        }
    }"#;

    tokio::fs::write(&config_path, json_content).await.unwrap();

    let manager = ConfigManager::new();
    manager.load_from_file(&config_path).await.unwrap();

    assert_eq!(manager.get::<usize>("router.worker_count").await, Some(16));
    assert_eq!(
        manager.get::<Vec<i32>>("custom.array").await,
        Some(vec![1, 2, 3])
    );
    assert_eq!(
        manager.get::<String>("custom.object.key").await,
        Some("value".to_string())
    );

    // 保存为 JSON
    let save_path = temp_dir.path().join("saved.json");
    manager.save(Some(&save_path)).await.unwrap();

    // 验证保存的文件
    let saved_content = tokio::fs::read_to_string(&save_path).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&saved_content).unwrap();
    assert_eq!(parsed["router"]["worker_count"], 16);
}
