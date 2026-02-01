//! 基本使用示例
//!
//! 本示例展示了薯片微内核的基本使用方法，包括：
//!
//! - 创建内核实例
//! - 启动和关闭内核
//! - 发送路由请求
//!
//! # 运行示例
//!
//! ```bash
//! cargo run --example basic_usage
//! ```
//!
//! # 注意
//!
//! 这是一个演示示例，部分功能可能还是骨架实现。
//! 示例代码主要用于展示 API 的使用方式。

use chips_core::{
    ChipsCore, CoreConfig, CoreConfigBuilder, RouteRequest, RouteRequestBuilder,
};
use serde_json::json;

/// 主函数
///
/// 演示薯片微内核的基本用法。
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== 薯片微内核基本使用示例 ===\n");

    // -------------------------------------------------------------------------
    // 1. 使用默认配置创建内核
    // -------------------------------------------------------------------------
    println!("1. 使用默认配置创建内核...");
    
    let config = CoreConfig::default();
    println!("   配置信息:");
    println!("   - 工作线程数: {}", config.router.worker_count);
    println!("   - 最大并发: {}", config.router.max_concurrent);
    println!("   - 日志级别: {}", config.logging.level);
    
    let mut core = ChipsCore::new(config).await?;
    println!("   ✅ 内核创建成功\n");

    // -------------------------------------------------------------------------
    // 2. 启动内核
    // -------------------------------------------------------------------------
    println!("2. 启动内核...");
    core.start().await?;
    
    // 检查内核状态
    let is_running = core.is_running().await;
    println!("   内核状态: {}", if is_running { "运行中" } else { "未运行" });
    println!("   ✅ 内核启动成功\n");

    // -------------------------------------------------------------------------
    // 3. 发送路由请求
    // -------------------------------------------------------------------------
    println!("3. 发送路由请求...");
    
    // 方式一：使用 RouteRequest::new()
    let request = RouteRequest::new("example", "test.action", json!({
        "message": "Hello from basic_usage example",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }));
    
    println!("   请求:");
    println!("   - ID: {}", request.request_id);
    println!("   - 发送方: {}", request.sender);
    println!("   - 动作: {}", request.action);
    
    let response = core.route(request).await;
    
    println!("   响应:");
    println!("   - 状态: {}", response.status);
    println!("   - 耗时: {} ms", response.elapsed_ms);
    if let Some(ref data) = response.data {
        println!("   - 数据: {}", serde_json::to_string_pretty(data)?);
    }
    if let Some(ref err) = response.error {
        println!("   - 错误: {} - {}", err.code, err.message);
    }
    println!();

    // -------------------------------------------------------------------------
    // 4. 使用 Builder 模式发送请求
    // -------------------------------------------------------------------------
    println!("4. 使用 Builder 模式发送请求...");
    
    let request = RouteRequestBuilder::new("builder_example", "module.action")
        .params(json!({
            "key": "value",
            "nested": {
                "data": true
            }
        }))
        .timeout_ms(5000)  // 5 秒超时
        .build();
    
    println!("   请求 ID: {}", request.request_id);
    println!("   超时: {} ms", request.timeout_ms);
    
    let response = core.route(request).await;
    println!("   响应状态: {}\n", response.status);

    // -------------------------------------------------------------------------
    // 5. 查看内核运行时间
    // -------------------------------------------------------------------------
    println!("5. 查看内核运行时间...");
    
    if let Some(uptime) = core.uptime() {
        println!("   运行时间: {:?}\n", uptime);
    }

    // -------------------------------------------------------------------------
    // 6. 关闭内核
    // -------------------------------------------------------------------------
    println!("6. 关闭内核...");
    core.shutdown().await?;
    
    let is_running = core.is_running().await;
    println!("   内核状态: {}", if is_running { "运行中" } else { "已关闭" });
    println!("   ✅ 内核关闭成功\n");

    println!("=== 示例结束 ===");
    
    Ok(())
}

/// 演示使用 Builder 模式配置内核
///
/// 这个函数展示了如何使用 CoreConfigBuilder 创建自定义配置。
#[allow(dead_code)]
async fn demo_builder_config() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n--- 使用 Builder 模式配置 ---");
    
    // 使用 Builder 模式创建配置
    let config = CoreConfigBuilder::new()
        .worker_count(8)
        .max_concurrent(2000)
        .log_level("debug")
        .dev_mode()
        .module_dir("./modules")
        .build();
    
    println!("自定义配置:");
    println!("  工作线程: {}", config.router.worker_count);
    println!("  最大并发: {}", config.router.max_concurrent);
    println!("  日志级别: {}", config.logging.level);
    println!("  开发模式: {}", config.dev_mode);
    
    // 创建内核
    let mut core = ChipsCore::new(config).await?;
    core.start().await?;
    
    // 做一些操作...
    
    core.shutdown().await?;
    println!("--- 配置演示结束 ---\n");
    
    Ok(())
}
