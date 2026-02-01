# Chips Core - 薯片微内核

<p align="center">
  <strong>薯片生态的核心微内核，提供中心路由、模块管理、事件总线和配置管理</strong>
</p>

<p align="center">
  <a href="#特性">特性</a> •
  <a href="#快速开始">快速开始</a> •
  <a href="#文档">文档</a> •
  <a href="#架构">架构</a> •
  <a href="#性能">性能</a> •
  <a href="#许可证">许可证</a>
</p>

---

## 特性

- **高性能中心路由** - 路由延迟 < 10ms (P95)，支持精确匹配、正则匹配、前缀匹配
- **灵活的模块管理** - 热插拔支持、依赖解析、生命周期管理
- **事件发布订阅** - 异步/同步发布、事件过滤、订阅者隔离
- **统一配置管理** - 多层级配置、热更新、变更监听
- **结构化日志系统** - JSON 格式、日志轮转、级别过滤
- **性能监控** - 延迟统计、吞吐量监控、资源监控
- **IPC 进程间通信** - Unix Socket / TCP 支持

---

## 快速开始

### 安装

在 `Cargo.toml` 中添加依赖：

```toml
[dependencies]
chips-core = "1.0"
tokio = { version = "1.35", features = ["full"] }
```

### 基本使用

```rust
use chips_core::{ChipsCore, CoreConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建配置
    let config = CoreConfig::default();
    
    // 初始化内核
    let mut core = ChipsCore::new(config).await?;
    
    // 启动内核
    core.start().await?;
    
    // 使用内核...
    println!("薯片微内核已启动");
    
    // 关闭内核
    core.shutdown().await?;
    
    Ok(())
}
```

### CLI 使用

```bash
# 启动内核
chips-core start

# 查看版本
chips-core version

# 查看统计信息
chips-core stats

# 检查配置
chips-core check-config -c config.yaml
```

---

## 文档

- [开发计划](开发计划/00-开发计划总览.md) - 详细的开发阶段和任务
- [SDK 接口对接规范](技术文档/SDK接口对接规范.md) - 与 Chips-SDK 的接口协议
- [API 文档](https://docs.rs/chips-core) - Rust API 文档

---

## 架构

```
┌─────────────────────────────────────────────────────────┐
│                    ChipsCore (SDK)                      │
├─────────────────────────────────────────────────────────┤
│  Router      │  EventBus   │  ConfigManager            │
│  路由系统    │  事件总线   │  配置管理                  │
├─────────────────────────────────────────────────────────┤
│  ModuleManager              │  MetricsCollector         │
│  模块管理                   │  性能监控                  │
├─────────────────────────────────────────────────────────┤
│  Logger      │  IPC Server │  Utils                    │
│  日志系统    │  IPC 服务   │  工具函数                  │
└─────────────────────────────────────────────────────────┘
```

### 核心组件

| 组件 | 说明 |
|------|------|
| **Router** | 中心路由系统，支持精确/正则/前缀匹配，带 LRU 缓存 |
| **ModuleManager** | 模块生命周期管理，依赖解析，热插拔 |
| **EventBus** | 事件发布订阅，过滤器，订阅者隔离 |
| **ConfigManager** | 配置加载/保存，层级覆盖，变更监听 |
| **Logger** | 结构化日志，文件输出，日志轮转 |
| **MetricsCollector** | 性能指标收集，P50/P95/P99 延迟 |

---

## 性能

### 基准测试结果

| 指标 | 数值 |
|------|------|
| 路由查找延迟 | **~1 µs** |
| 请求创建 | ~1 µs |
| 事件发布 | ~100 µs |
| 并发支持 | **1000+ req/s** |
| 内存占用 | **< 30 MB** |

### 性能目标

- ✅ 路由延迟 < 10ms (P95)
- ✅ 路由延迟 < 20ms (P99)
- ✅ 支持 ≥1000 并发请求
- ✅ 内存占用 < 30MB
- ✅ 启动时间 < 500ms

---

## 测试

```bash
# 运行所有测试
cargo test

# 运行端到端测试
cargo test --test e2e_test

# 运行性能基准测试
cargo bench
```

### 测试覆盖

- 单元测试: 367+
- 集成测试: 65+
- 文档测试: 51+
- **总计: 483+ 测试**

---

## 开发

### 构建

```bash
# 开发构建
cargo build

# 发布构建
cargo build --release

# 运行检查
cargo check
cargo clippy
cargo fmt
```

### 项目结构

```
Chips-core/
├── src/
│   ├── api/          # SDK 和 IPC 接口
│   ├── core/         # 核心配置
│   ├── module/       # 模块管理系统
│   ├── router/       # 路由系统
│   └── utils/        # 工具函数
├── tests/            # 集成测试
├── benches/          # 性能基准测试
├── examples/         # 使用示例
└── 开发计划/          # 开发文档
```

---

## 与生态的关系

Chips-core 是薯片生态的第一层基础：

```
第一层：Chips-core（薯片微内核）     ← 当前项目
    ↓
第二层：Chips-Foundation（公共基础层）
    ↓
第三层：Chips-SDK / Chips-ComponentLibrary
    ↓
第四层：应用层（Editor、Viewer 等）
```

---

## 许可证

MIT License

---

## 贡献

欢迎贡献代码！请参阅 [CONTRIBUTING.md](CONTRIBUTING.md) 了解详情。
