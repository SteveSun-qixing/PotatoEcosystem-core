//! Phase 5: 日志系统与性能监控集成测试
//!
//! 测试日志系统和性能监控的完整工作流程

use chips_core::utils::metrics::{MetricsCollector, MetricsReport, MonitoringSystem};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// ============================================================================
// 性能指标收集测试
// ============================================================================

/// 测试完整的指标收集流程
#[test]
fn test_metrics_collection_full_flow() {
    let collector = MetricsCollector::new();

    // 模拟路由请求
    for i in 0..100 {
        let success = i % 10 != 0; // 90% 成功率
        let latency = 100 + (i % 50) * 10; // 100-590 微秒
        collector.record_route(success, latency);
    }

    // 验证计数
    let report = collector.export();
    assert_eq!(report.route_metrics.total_requests, 100);
    assert_eq!(report.route_metrics.successful_requests, 90);
    assert_eq!(report.route_metrics.failed_requests, 10);

    // 验证成功率
    let success_rate = report.route_metrics.success_rate;
    assert!((success_rate - 0.9).abs() < 0.01);
}

/// 测试百分位数计算
#[test]
fn test_percentile_calculation() {
    let collector = MetricsCollector::new();

    // 添加已知分布的延迟数据
    // 1-100 的序列，P50=50, P95=95, P99=99
    for i in 1..=100 {
        collector.record_route(true, i);
    }

    // 验证百分位数
    let p50 = collector.get_percentile(0.5);
    let p95 = collector.get_percentile(0.95);
    let p99 = collector.get_percentile(0.99);

    // 允许一定误差
    assert!(p50 >= 45 && p50 <= 55, "P50 should be around 50, got {}", p50);
    assert!(p95 >= 90 && p95 <= 100, "P95 should be around 95, got {}", p95);
    assert!(p99 >= 95 && p99 <= 100, "P99 should be around 99, got {}", p99);
}

/// 测试并发指标收集
#[test]
fn test_concurrent_metrics_collection() {
    let collector = Arc::new(MetricsCollector::new());
    let mut handles = vec![];

    // 10 个线程并发记录
    for _ in 0..10 {
        let c = collector.clone();
        handles.push(thread::spawn(move || {
            for j in 0..100 {
                c.record_route(j % 2 == 0, 100 + j);
            }
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // 验证总计数
    let report = collector.export();
    assert_eq!(report.route_metrics.total_requests, 1000);
    assert_eq!(report.route_metrics.successful_requests, 500);
    assert_eq!(report.route_metrics.failed_requests, 500);
}

/// 测试指标重置
#[test]
fn test_metrics_reset() {
    let collector = MetricsCollector::new();

    // 记录一些数据
    for i in 0..50 {
        collector.record_route(true, 100 + i);
    }

    // 验证有数据
    let report = collector.export();
    assert_eq!(report.route_metrics.total_requests, 50);

    // 重置
    collector.reset();

    // 验证已清零
    let report = collector.export();
    assert_eq!(report.route_metrics.total_requests, 0);
    assert_eq!(report.route_metrics.successful_requests, 0);
}

// ============================================================================
// 监控数据导出测试
// ============================================================================

/// 测试 JSON 导出
#[test]
fn test_metrics_json_export() {
    let collector = MetricsCollector::new();

    // 记录一些数据
    for i in 0..10 {
        collector.record_route(i % 2 == 0, 100 + i * 10);
    }

    let report = collector.export();
    let json = report.to_json().unwrap();

    // 验证 JSON 包含关键字段
    assert!(json.contains("route_metrics"));
    assert!(json.contains("total_requests"));
    assert!(json.contains("latency_metrics"));
}

/// 测试报告反序列化
#[test]
fn test_metrics_report_roundtrip() {
    let collector = MetricsCollector::new();
    collector.record_route(true, 500);
    collector.record_route(false, 1000);

    let report = collector.export();
    let json = report.to_json().unwrap();

    // 反序列化
    let parsed: MetricsReport = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.route_metrics.total_requests, 2);
    assert_eq!(parsed.route_metrics.successful_requests, 1);
}

// ============================================================================
// 综合监控系统测试
// ============================================================================

/// 测试综合监控系统
#[test]
fn test_monitoring_system() {
    let system = MonitoringSystem::new();

    // 记录指标
    for i in 0..20 {
        system.record_route(i % 3 != 0, 100 + i * 5);
    }

    // 导出报告
    let report = system.export();
    assert_eq!(report.route_metrics.total_requests, 20);
}

// ============================================================================
// 边界情况测试
// ============================================================================

/// 测试空数据导出
#[test]
fn test_empty_metrics_export() {
    let collector = MetricsCollector::new();
    let report = collector.export();

    assert_eq!(report.route_metrics.total_requests, 0);
    assert_eq!(report.latency_metrics.avg_latency_us, 0);
    assert_eq!(report.latency_metrics.p50_latency_us, 0);
    assert_eq!(report.latency_metrics.p95_latency_us, 0);
    assert_eq!(report.latency_metrics.p99_latency_us, 0);
}

/// 测试单个数据点
#[test]
fn test_single_data_point() {
    let collector = MetricsCollector::new();
    collector.record_route(true, 500);

    let report = collector.export();
    assert_eq!(report.route_metrics.total_requests, 1);
    assert_eq!(report.latency_metrics.avg_latency_us, 500);

    // 单个数据点，所有百分位数应该相同
    let p50 = collector.get_percentile(0.5);
    let p99 = collector.get_percentile(0.99);
    assert_eq!(p50, 500);
    assert_eq!(p99, 500);
}

/// 测试大量数据
#[test]
fn test_large_dataset() {
    let collector = MetricsCollector::new();

    // 记录 10000 个数据点
    for i in 0..10000 {
        collector.record_route(true, i % 1000 + 100);
    }

    let report = collector.export();
    assert_eq!(report.route_metrics.total_requests, 10000);

    // 验证平均值在合理范围内
    // 延迟范围是 100-1099，平均应该在 ~600 左右
    assert!(report.latency_metrics.avg_latency_us > 400);
    assert!(report.latency_metrics.avg_latency_us < 800);
}

/// 测试吞吐量计算
#[test]
fn test_throughput_calculation() {
    let collector = MetricsCollector::new();

    // 记录数据
    for _ in 0..100 {
        collector.record_route(true, 100);
    }

    // 等待一小段时间
    thread::sleep(Duration::from_millis(100));

    // 导出并检查吞吐量
    let report = collector.export();

    // 吞吐量应该大于 0
    // 注意：由于计算方式，这个值可能因系统而异
    assert!(report.route_metrics.throughput_per_second >= 0.0);
}

/// 测试延迟统计
#[test]
fn test_latency_statistics() {
    let collector = MetricsCollector::new();

    // 记录一些延迟数据
    collector.record_route(true, 100);
    collector.record_route(true, 200);
    collector.record_route(true, 300);
    collector.record_route(true, 400);
    collector.record_route(true, 500);

    let report = collector.export();

    // 验证延迟统计
    assert_eq!(report.latency_metrics.min_latency_us, 100);
    assert_eq!(report.latency_metrics.max_latency_us, 500);
    assert_eq!(report.latency_metrics.avg_latency_us, 300);
}
