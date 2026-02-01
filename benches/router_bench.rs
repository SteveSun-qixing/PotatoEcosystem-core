//! 路由系统性能基准测试

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use chips_core::{RouteRequest, RouteTable, RouteEntry};
use serde_json::json;

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
}

fn concurrent_route_lookup_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    let route_table = rt.block_on(async {
        let table = RouteTable::new();
        for i in 0..100 {
            let action = format!("action_{}", i);
            let entry = RouteEntry::exact(format!("module_{}", i), &action);
            table.register_action_route(&action, entry).await.unwrap();
        }
        std::sync::Arc::new(table)
    });
    
    let mut group = c.benchmark_group("concurrent_lookup");
    
    for concurrency in [10, 50, 100, 500].iter() {
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

criterion_group!(
    benches,
    route_table_lookup_benchmark,
    route_request_creation_benchmark,
    concurrent_route_lookup_benchmark,
);

criterion_main!(benches);
