//! 路由系统性能基准测试
//!
//! 使用 Criterion 框架进行性能测试，包括：
//! - 路由表查找基准
//! - 路由请求创建基准
//! - 并发路由查找基准
//! - 事件发布基准
//! - 完整路由流程基准

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use chips_core::{RouteRequest, RouteTable, RouteEntry, Event};
use chips_core::router::{EventBus, Router, RouterConfig, ModuleHandler, RouteResponse};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

// ============================================================================
// 测试辅助结构
// ============================================================================

/// 基准测试用的模拟模块处理器
struct BenchModuleHandler {
    id: String,
}

impl BenchModuleHandler {
    fn new(id: &str) -> Self {
        Self { id: id.to_string() }
    }
}

#[async_trait::async_trait]
impl ModuleHandler for BenchModuleHandler {
    async fn handle(&self, request: RouteRequest) -> RouteResponse {
        RouteResponse::success(
            &request.request_id,
            json!({
                "module_id": self.id,
                "action": request.action,
            }),
            0,
        )
    }

    fn module_id(&self) -> &str {
        &self.id
    }
}

// ============================================================================
// 路由表查找基准测试
// ============================================================================

/// 路由表精确查找基准测试
fn route_table_lookup_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    // 创建路由表并注册一些路由
    let route_table = rt.block_on(async {
        let table = RouteTable::new();
        
        // 注册100个路由
        for i in 0..100 {
            let action = format!("module{}.action{}", i / 10, i % 10);
            let entry = RouteEntry::exact(format!("module_{}", i / 10), &action);
            table.register_action_route(&action, entry).await.unwrap();
        }
        
        table
    });
    
    c.bench_function("route_table_exact_lookup", |b| {
        b.to_async(&rt).iter(|| async {
            let request = RouteRequest::new("test", "module5.action5", json!({}));
            route_table.find_route(black_box(&request)).await
        });
    });
}

/// 路由表不同大小的查找性能
fn route_table_size_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    let mut group = c.benchmark_group("route_table_size");
    
    for size in [10, 100, 500, 1000].iter() {
        let route_table = rt.block_on(async {
            let table = RouteTable::new();
            for i in 0..*size {
                let action = format!("action_{}", i);
                let entry = RouteEntry::exact(format!("module_{}", i % 10), &action);
                table.register_action_route(&action, entry).await.unwrap();
            }
            table
        });
        
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            size,
            |b, &size| {
                b.to_async(&rt).iter(|| async {
                    // 随机查找一个存在的路由
                    let idx = (size / 2) as usize;
                    let action = format!("action_{}", idx);
                    let request = RouteRequest::new("test", &action, json!({}));
                    route_table.find_route(black_box(&request)).await
                });
            },
        );
    }
    
    group.finish();
}

// ============================================================================
// 路由请求创建基准测试
// ============================================================================

/// 路由请求创建性能
fn route_request_creation_benchmark(c: &mut Criterion) {
    c.bench_function("route_request_new", |b| {
        b.iter(|| {
            RouteRequest::new(
                black_box("sender"),
                black_box("test.action"),
                black_box(json!({"key": "value"})),
            )
        });
    });
    
    c.bench_function("route_request_builder", |b| {
        b.iter(|| {
            RouteRequest::builder(black_box("sender"), black_box("test.action"))
                .params(black_box(json!({"key": "value"})))
                .build()
        });
    });
    
    // 测试复杂参数的请求创建
    c.bench_function("route_request_complex_params", |b| {
        b.iter(|| {
            RouteRequest::new(
                black_box("sender"),
                black_box("test.action"),
                black_box(json!({
                    "user": {
                        "id": "12345",
                        "name": "Test User",
                        "email": "test@example.com"
                    },
                    "metadata": {
                        "timestamp": "2026-02-01T00:00:00Z",
                        "source": "benchmark"
                    },
                    "items": [1, 2, 3, 4, 5]
                })),
            )
        });
    });
}

// ============================================================================
// 并发路由查找基准测试
// ============================================================================

/// 并发路由表查找基准
fn concurrent_route_lookup_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    let route_table = rt.block_on(async {
        let table = RouteTable::new();
        for i in 0..100 {
            let action = format!("action_{}", i);
            let entry = RouteEntry::exact(format!("module_{}", i), &action);
            table.register_action_route(&action, entry).await.unwrap();
        }
        Arc::new(table)
    });
    
    let mut group = c.benchmark_group("concurrent_lookup");
    group.measurement_time(Duration::from_secs(10));
    
    for concurrency in [10, 50, 100, 500].iter() {
        group.throughput(Throughput::Elements(*concurrency as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(concurrency),
            concurrency,
            |b, &concurrency| {
                b.to_async(&rt).iter(|| async {
                    let mut handles = Vec::with_capacity(concurrency);
                    for i in 0..concurrency {
                        let table = route_table.clone();
                        handles.push(tokio::spawn(async move {
                            let action = format!("action_{}", i % 100);
                            let request = RouteRequest::new("test", &action, json!({}));
                            table.find_route(&request).await
                        }));
                    }
                    for handle in handles {
                        let _ = handle.await;
                    }
                });
            },
        );
    }
    
    group.finish();
}

// ============================================================================
// 完整路由流程基准测试
// ============================================================================

/// 完整路由流程基准（包含路由器处理）
fn router_complete_flow_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    let router = rt.block_on(async {
        let config = RouterConfig {
            worker_count: 4,
            max_concurrent: 100,
            queue_size: 1000,
            default_timeout_ms: 5000,
            enable_validation: true,
        };
        let router = Router::new(config);
        
        // 注册处理器
        let handler = Arc::new(BenchModuleHandler::new("bench_module"));
        router.register_handler(handler).await.unwrap();
        
        // 注册路由
        router
            .register_route("bench.action", RouteEntry::exact("bench_module", "bench.action"))
            .await
            .unwrap();
        
        router.start().await.unwrap();
        Arc::new(router)
    });
    
    let mut group = c.benchmark_group("router_complete_flow");
    group.measurement_time(Duration::from_secs(15));
    
    // 单请求延迟
    group.bench_function("single_request", |b| {
        b.to_async(&rt).iter(|| async {
            let request = RouteRequest::new("bench_client", "bench.action", json!({}));
            router.route(black_box(request)).await
        });
    });
    
    // 批量请求
    for batch_size in [10, 50, 100].iter() {
        group.throughput(Throughput::Elements(*batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("batch_requests", batch_size),
            batch_size,
            |b, &batch_size| {
                b.to_async(&rt).iter(|| async {
                    let mut handles = Vec::with_capacity(batch_size);
                    for _ in 0..batch_size {
                        let router = router.clone();
                        handles.push(tokio::spawn(async move {
                            let request = RouteRequest::new("bench_client", "bench.action", json!({}));
                            router.route(request).await
                        }));
                    }
                    for handle in handles {
                        let _ = handle.await;
                    }
                });
            },
        );
    }
    
    group.finish();
    
    // 清理
    rt.block_on(async {
        router.stop().await.unwrap();
    });
}

/// 并发路由基准（模拟实际负载）
fn router_concurrent_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    let router = rt.block_on(async {
        let config = RouterConfig {
            worker_count: 8,
            max_concurrent: 500,
            queue_size: 5000,
            default_timeout_ms: 5000,
            enable_validation: true,
        };
        let router = Router::new(config);
        
        // 注册多个模块
        for i in 0..5 {
            let handler = Arc::new(BenchModuleHandler::new(&format!("module_{}", i)));
            router.register_handler(handler).await.unwrap();
            
            for j in 0..10 {
                let action = format!("module_{}.action_{}", i, j);
                router
                    .register_route(&action, RouteEntry::exact(&format!("module_{}", i), &action))
                    .await
                    .unwrap();
            }
        }
        
        router.start().await.unwrap();
        Arc::new(router)
    });
    
    let mut group = c.benchmark_group("router_concurrent");
    group.measurement_time(Duration::from_secs(20));
    
    for concurrency in [100, 200, 500, 1000].iter() {
        group.throughput(Throughput::Elements(*concurrency as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(concurrency),
            concurrency,
            |b, &concurrency| {
                b.to_async(&rt).iter(|| async {
                    let mut handles = Vec::with_capacity(concurrency);
                    for i in 0..concurrency {
                        let router = router.clone();
                        let module_idx = i % 5;
                        let action_idx = i % 10;
                        handles.push(tokio::spawn(async move {
                            let action = format!("module_{}.action_{}", module_idx, action_idx);
                            let request = RouteRequest::new("bench_client", &action, json!({}));
                            router.route(request).await
                        }));
                    }
                    for handle in handles {
                        let _ = handle.await;
                    }
                });
            },
        );
    }
    
    group.finish();
    
    rt.block_on(async {
        router.stop().await.unwrap();
    });
}

// ============================================================================
// 事件发布基准测试
// ============================================================================

/// 事件创建基准
fn event_creation_benchmark(c: &mut Criterion) {
    c.bench_function("event_new", |b| {
        b.iter(|| {
            Event::new(
                black_box("test.event"),
                black_box("sender"),
                black_box(json!({"key": "value"})),
            )
        });
    });
    
    c.bench_function("event_builder", |b| {
        b.iter(|| {
            Event::builder(black_box("test.event"), black_box("sender"))
                .data(black_box(json!({"key": "value"})))
                .build()
        });
    });
}

/// 事件发布基准
fn event_publish_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    let event_bus = rt.block_on(async {
        let bus = EventBus::new();
        
        // 注册一些订阅者
        for i in 0..10 {
            bus.subscribe(
                &format!("subscriber_{}", i),
                "bench.event",
                None,
                Arc::new(|_| {}),
            )
            .await
            .unwrap();
        }
        
        Arc::new(bus)
    });
    
    let mut group = c.benchmark_group("event_publish");
    group.measurement_time(Duration::from_secs(10));
    
    // 异步发布
    group.bench_function("async_publish", |b| {
        b.to_async(&rt).iter(|| async {
            let event = Event::new("bench.event", "bench_sender", json!({}));
            event_bus.publish(black_box(event)).await
        });
    });
    
    // 同步发布
    group.bench_function("sync_publish", |b| {
        b.to_async(&rt).iter(|| async {
            let event = Event::new("bench.event", "bench_sender", json!({}));
            event_bus.publish_sync(black_box(event)).await
        });
    });
    
    group.finish();
}

/// 并发事件发布基准
fn event_concurrent_publish_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    let event_bus = rt.block_on(async {
        let bus = EventBus::new();
        
        // 注册订阅者
        for i in 0..5 {
            bus.subscribe(
                &format!("subscriber_{}", i),
                "concurrent.event",
                None,
                Arc::new(|_| {}),
            )
            .await
            .unwrap();
        }
        
        Arc::new(bus)
    });
    
    let mut group = c.benchmark_group("event_concurrent_publish");
    group.measurement_time(Duration::from_secs(15));
    
    for concurrency in [10, 50, 100, 500].iter() {
        group.throughput(Throughput::Elements(*concurrency as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(concurrency),
            concurrency,
            |b, &concurrency| {
                b.to_async(&rt).iter(|| async {
                    let mut handles = Vec::with_capacity(concurrency);
                    for i in 0..concurrency {
                        let bus = event_bus.clone();
                        handles.push(tokio::spawn(async move {
                            let event = Event::new(
                                "concurrent.event",
                                &format!("publisher_{}", i),
                                json!({}),
                            );
                            bus.publish(event).await
                        }));
                    }
                    for handle in handles {
                        let _ = handle.await;
                    }
                });
            },
        );
    }
    
    group.finish();
}

/// 不同订阅者数量的事件发布性能
fn event_subscriber_count_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    let mut group = c.benchmark_group("event_subscriber_count");
    group.measurement_time(Duration::from_secs(10));
    
    for subscriber_count in [1, 10, 50, 100].iter() {
        let event_bus = rt.block_on(async {
            let bus = EventBus::new();
            
            for i in 0..*subscriber_count {
                bus.subscribe(
                    &format!("subscriber_{}", i),
                    "scale.event",
                    None,
                    Arc::new(|_| {}),
                )
                .await
                .unwrap();
            }
            
            Arc::new(bus)
        });
        
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(subscriber_count),
            subscriber_count,
            |b, _| {
                b.to_async(&rt).iter(|| async {
                    let event = Event::new("scale.event", "bench_sender", json!({}));
                    event_bus.publish_sync(black_box(event)).await
                });
            },
        );
    }
    
    group.finish();
}

// ============================================================================
// 基准测试组
// ============================================================================

criterion_group!(
    name = route_table_benches;
    config = Criterion::default().sample_size(100);
    targets = route_table_lookup_benchmark, route_table_size_benchmark
);

criterion_group!(
    name = route_request_benches;
    config = Criterion::default().sample_size(200);
    targets = route_request_creation_benchmark
);

criterion_group!(
    name = concurrent_benches;
    config = Criterion::default().sample_size(50);
    targets = concurrent_route_lookup_benchmark
);

criterion_group!(
    name = router_benches;
    config = Criterion::default().sample_size(50);
    targets = router_complete_flow_benchmark, router_concurrent_benchmark
);

criterion_group!(
    name = event_benches;
    config = Criterion::default().sample_size(100);
    targets = event_creation_benchmark, event_publish_benchmark, 
              event_concurrent_publish_benchmark, event_subscriber_count_benchmark
);

criterion_main!(
    route_table_benches,
    route_request_benches,
    concurrent_benches,
    router_benches,
    event_benches
);
