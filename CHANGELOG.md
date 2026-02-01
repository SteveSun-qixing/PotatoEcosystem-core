# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-02-01

### Added

#### 中心路由系统
- 路由表管理（精确匹配、正则匹配、前缀匹配）
- 请求验证器
- 优先级请求队列
- 并发控制和超时处理
- 路由统计
- LRU 路由缓存
- 快速路径优化（Foundation 模块）

#### 模块管理系统
- 模块元数据解析 (module.yaml)
- 模块注册表
- 依赖图（循环依赖检测、拓扑排序）
- 模块加载/卸载
- 生命周期管理（onInit/onStart/onStop/onCleanup）
- 热插拔支持
- 健康检查

#### 事件总线
- 事件发布（异步/同步）
- 事件订阅（支持过滤器）
- 订阅者隔离
- 超时控制
- 分发统计

#### 配置管理
- 配置文件加载（YAML/JSON）
- 配置层级覆盖
- 配置读写 API
- 配置变更监听
- 配置热更新骨架

#### 日志系统
- 结构化日志（JSON 格式）
- 日志文件输出
- 日志轮转（Daily/Hourly）
- 日志过滤（EnvFilter）

#### 性能监控
- 性能指标收集
- 百分位数计算（P50/P95/P99）
- 延迟统计
- 吞吐量监控
- 资源监控

#### SDK 与 API
- ChipsCore 主结构体
- 便捷路由 API
- 便捷事件 API
- CLI 命令行接口
- IPC 进程间通信（Unix Socket / TCP）
- Builder 模式配置

### Performance

- 路由查找延迟: ~1 µs
- 路由延迟 < 10ms (P95)
- 路由延迟 < 20ms (P99)
- 支持 1000+ 并发请求
- 内存占用 < 30MB
- 启动时间 < 500ms

### Tests

- 单元测试: 367+
- 集成测试: 65+
- 文档测试: 51+
- E2E 测试: 24
- 总计: 483+ 测试全部通过

### Documentation

- API 文档注释
- SDK 接口对接规范
- 开发计划文档
- 各阶段完成总结

---

## [0.1.0] - 2026-02-01

### Added
- 项目初始化
- 基础架构搭建
- 开发计划制定
