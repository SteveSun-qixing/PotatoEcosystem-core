//! 性能监控系统
//!
//! 本模块提供性能指标收集、资源监控和监控数据导出功能。
//!
//! # 功能特性
//!
//! - **性能指标收集**: 收集路由延迟、吞吐量、成功/失败率等指标
//! - **资源监控**: 监控内存使用、线程数，支持告警阈值
//! - **数据导出**: 支持 JSON 格式导出监控数据
//!
//! # 示例
//!
//! ```rust
//! use chips_core::utils::metrics::{MetricsCollector, ResourceMonitor};
//!
//! // 创建指标收集器
//! let collector = MetricsCollector::new();
//!
//! // 记录路由请求
//! collector.record_route(true, 1500); // 成功，耗时 1500 微秒
//!
//! // 获取性能指标
//! let p95 = collector.get_percentile(0.95);
//! let avg = collector.get_average_latency();
//!
//! // 导出报告
//! let report = collector.export();
//! let json = serde_json::to_string_pretty(&report).unwrap();
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tracing::warn;

// ============================================================================
// 常量定义
// ============================================================================

/// 延迟样本的最大数量（防止内存无限增长）
const MAX_LATENCY_SAMPLES: usize = 10_000;

/// 默认内存告警阈值（MB）
const DEFAULT_MEMORY_THRESHOLD_MB: u64 = 512;

/// 默认线程数告警阈值
const DEFAULT_THREAD_THRESHOLD: u64 = 100;

// ============================================================================
// MetricsCollector - 性能指标收集器
// ============================================================================

/// 性能指标收集器
///
/// 使用原子操作和锁来确保线程安全。收集路由延迟、请求吞吐量、成功/失败率等指标。
///
/// # 线程安全
///
/// 所有计数器使用 `AtomicU64`，延迟样本使用 `Mutex<Vec<u64>>` 保护。
#[derive(Debug)]
pub struct MetricsCollector {
    // ==================== 路由指标 ====================
    /// 路由请求总数
    route_total_count: AtomicU64,

    /// 成功的路由请求数
    route_success_count: AtomicU64,

    /// 失败的路由请求数
    route_failure_count: AtomicU64,

    /// 路由总延迟（微秒）
    route_total_latency_us: AtomicU64,

    /// 最小延迟（微秒）
    route_min_latency_us: AtomicU64,

    /// 最大延迟（微秒）
    route_max_latency_us: AtomicU64,

    // ==================== 延迟分布 ====================
    /// 延迟样本 - 用于计算百分位数
    latency_samples: Arc<Mutex<Vec<u64>>>,

    // ==================== 时间戳 ====================
    /// 收集开始时间
    start_time: DateTime<Utc>,
}

impl MetricsCollector {
    /// 创建新的指标收集器
    ///
    /// # 示例
    ///
    /// ```rust
    /// use chips_core::utils::metrics::MetricsCollector;
    ///
    /// let collector = MetricsCollector::new();
    /// ```
    pub fn new() -> Self {
        Self {
            route_total_count: AtomicU64::new(0),
            route_success_count: AtomicU64::new(0),
            route_failure_count: AtomicU64::new(0),
            route_total_latency_us: AtomicU64::new(0),
            route_min_latency_us: AtomicU64::new(u64::MAX),
            route_max_latency_us: AtomicU64::new(0),
            latency_samples: Arc::new(Mutex::new(Vec::with_capacity(1000))),
            start_time: Utc::now(),
        }
    }

    /// 记录一次路由请求
    ///
    /// # 参数
    ///
    /// * `success` - 请求是否成功
    /// * `latency_us` - 请求延迟（微秒）
    ///
    /// # 示例
    ///
    /// ```rust
    /// use chips_core::utils::metrics::MetricsCollector;
    ///
    /// let collector = MetricsCollector::new();
    /// collector.record_route(true, 1500);  // 成功请求，1.5ms
    /// collector.record_route(false, 5000); // 失败请求，5ms
    /// ```
    pub fn record_route(&self, success: bool, latency_us: u64) {
        // 更新计数器
        self.route_total_count.fetch_add(1, Ordering::Relaxed);
        self.route_total_latency_us
            .fetch_add(latency_us, Ordering::Relaxed);

        if success {
            self.route_success_count.fetch_add(1, Ordering::Relaxed);
        } else {
            self.route_failure_count.fetch_add(1, Ordering::Relaxed);
        }

        // 更新最小延迟
        let mut current_min = self.route_min_latency_us.load(Ordering::Relaxed);
        while latency_us < current_min {
            match self.route_min_latency_us.compare_exchange_weak(
                current_min,
                latency_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_min = x,
            }
        }

        // 更新最大延迟
        let mut current_max = self.route_max_latency_us.load(Ordering::Relaxed);
        while latency_us > current_max {
            match self.route_max_latency_us.compare_exchange_weak(
                current_max,
                latency_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_max = x,
            }
        }

        // 记录到延迟样本（用于百分位数计算）
        if let Ok(mut samples) = self.latency_samples.lock() {
            // 限制样本数量，防止内存无限增长
            if samples.len() >= MAX_LATENCY_SAMPLES {
                // 采用滑动窗口策略：移除前半部分
                let half = samples.len() / 2;
                samples.drain(0..half);
            }
            samples.push(latency_us);
        }
    }

    /// 计算指定百分位数的延迟
    ///
    /// # 参数
    ///
    /// * `p` - 百分位数（0.0 到 1.0 之间）
    ///
    /// # 返回
    ///
    /// 返回指定百分位数的延迟值（微秒），如果没有样本则返回 0
    ///
    /// # 示例
    ///
    /// ```rust
    /// use chips_core::utils::metrics::MetricsCollector;
    ///
    /// let collector = MetricsCollector::new();
    /// for i in 1..=100 {
    ///     collector.record_route(true, i * 100);
    /// }
    ///
    /// let p50 = collector.get_percentile(0.50); // 中位数
    /// let p95 = collector.get_percentile(0.95); // 95 百分位
    /// let p99 = collector.get_percentile(0.99); // 99 百分位
    /// ```
    pub fn get_percentile(&self, p: f64) -> u64 {
        let p = p.clamp(0.0, 1.0);

        if let Ok(mut samples) = self.latency_samples.lock() {
            if samples.is_empty() {
                return 0;
            }

            // 对样本排序
            samples.sort_unstable();

            // 计算索引（使用最近等级法）
            let idx = ((samples.len() as f64 * p).ceil() as usize).saturating_sub(1);
            let idx = idx.min(samples.len() - 1);

            samples[idx]
        } else {
            0
        }
    }

    /// 获取 P50（中位数）延迟
    pub fn get_p50_latency(&self) -> u64 {
        self.get_percentile(0.50)
    }

    /// 获取 P95 延迟
    pub fn get_p95_latency(&self) -> u64 {
        self.get_percentile(0.95)
    }

    /// 获取 P99 延迟
    pub fn get_p99_latency(&self) -> u64 {
        self.get_percentile(0.99)
    }

    /// 获取平均延迟（微秒）
    pub fn get_average_latency(&self) -> u64 {
        let total = self.route_total_count.load(Ordering::Relaxed);
        if total == 0 {
            return 0;
        }
        self.route_total_latency_us.load(Ordering::Relaxed) / total
    }

    /// 获取最小延迟（微秒）
    pub fn get_min_latency(&self) -> u64 {
        let min = self.route_min_latency_us.load(Ordering::Relaxed);
        if min == u64::MAX {
            0
        } else {
            min
        }
    }

    /// 获取最大延迟（微秒）
    pub fn get_max_latency(&self) -> u64 {
        self.route_max_latency_us.load(Ordering::Relaxed)
    }

    /// 获取总请求数
    pub fn get_total_requests(&self) -> u64 {
        self.route_total_count.load(Ordering::Relaxed)
    }

    /// 获取成功请求数
    pub fn get_success_count(&self) -> u64 {
        self.route_success_count.load(Ordering::Relaxed)
    }

    /// 获取失败请求数
    pub fn get_failure_count(&self) -> u64 {
        self.route_failure_count.load(Ordering::Relaxed)
    }

    /// 获取成功率（0.0 到 1.0）
    pub fn get_success_rate(&self) -> f64 {
        let total = self.route_total_count.load(Ordering::Relaxed);
        if total == 0 {
            return 1.0; // 没有请求时返回 100% 成功率
        }
        let success = self.route_success_count.load(Ordering::Relaxed);
        success as f64 / total as f64
    }

    /// 获取失败率（0.0 到 1.0）
    pub fn get_failure_rate(&self) -> f64 {
        1.0 - self.get_success_rate()
    }

    /// 获取吞吐量（每秒请求数）
    pub fn get_throughput(&self) -> f64 {
        let total = self.route_total_count.load(Ordering::Relaxed);
        let duration = Utc::now()
            .signed_duration_since(self.start_time)
            .num_seconds();
        if duration <= 0 {
            return total as f64;
        }
        total as f64 / duration as f64
    }

    /// 导出指标报告
    ///
    /// # 示例
    ///
    /// ```rust
    /// use chips_core::utils::metrics::MetricsCollector;
    ///
    /// let collector = MetricsCollector::new();
    /// collector.record_route(true, 1000);
    ///
    /// let report = collector.export();
    /// let json = serde_json::to_string_pretty(&report).unwrap();
    /// println!("{}", json);
    /// ```
    pub fn export(&self) -> MetricsReport {
        MetricsReport {
            timestamp: Utc::now(),
            uptime_seconds: Utc::now()
                .signed_duration_since(self.start_time)
                .num_seconds() as u64,
            route_metrics: RouteMetrics {
                total_requests: self.get_total_requests(),
                successful_requests: self.get_success_count(),
                failed_requests: self.get_failure_count(),
                success_rate: self.get_success_rate(),
                failure_rate: self.get_failure_rate(),
                throughput_per_second: self.get_throughput(),
            },
            latency_metrics: LatencyMetrics {
                avg_latency_us: self.get_average_latency(),
                min_latency_us: self.get_min_latency(),
                max_latency_us: self.get_max_latency(),
                p50_latency_us: self.get_p50_latency(),
                p95_latency_us: self.get_p95_latency(),
                p99_latency_us: self.get_p99_latency(),
            },
            resource_metrics: None, // 由 ResourceMonitor 填充
        }
    }

    /// 重置所有统计数据
    ///
    /// # 示例
    ///
    /// ```rust
    /// use chips_core::utils::metrics::MetricsCollector;
    ///
    /// let collector = MetricsCollector::new();
    /// collector.record_route(true, 1000);
    /// assert!(collector.get_total_requests() > 0);
    ///
    /// collector.reset();
    /// assert_eq!(collector.get_total_requests(), 0);
    /// ```
    pub fn reset(&self) {
        self.route_total_count.store(0, Ordering::Relaxed);
        self.route_success_count.store(0, Ordering::Relaxed);
        self.route_failure_count.store(0, Ordering::Relaxed);
        self.route_total_latency_us.store(0, Ordering::Relaxed);
        self.route_min_latency_us.store(u64::MAX, Ordering::Relaxed);
        self.route_max_latency_us.store(0, Ordering::Relaxed);

        if let Ok(mut samples) = self.latency_samples.lock() {
            samples.clear();
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MetricsCollector {
    fn clone(&self) -> Self {
        let samples = self
            .latency_samples
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default();

        Self {
            route_total_count: AtomicU64::new(self.route_total_count.load(Ordering::Relaxed)),
            route_success_count: AtomicU64::new(self.route_success_count.load(Ordering::Relaxed)),
            route_failure_count: AtomicU64::new(self.route_failure_count.load(Ordering::Relaxed)),
            route_total_latency_us: AtomicU64::new(
                self.route_total_latency_us.load(Ordering::Relaxed),
            ),
            route_min_latency_us: AtomicU64::new(
                self.route_min_latency_us.load(Ordering::Relaxed),
            ),
            route_max_latency_us: AtomicU64::new(
                self.route_max_latency_us.load(Ordering::Relaxed),
            ),
            latency_samples: Arc::new(Mutex::new(samples)),
            start_time: self.start_time,
        }
    }
}

// ============================================================================
// ResourceMonitor - 资源监控器
// ============================================================================

/// 资源监控配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMonitorConfig {
    /// 内存告警阈值（MB）
    pub memory_threshold_mb: u64,

    /// 线程数告警阈值
    pub thread_threshold: u64,

    /// 是否启用告警
    pub alerts_enabled: bool,
}

impl Default for ResourceMonitorConfig {
    fn default() -> Self {
        Self {
            memory_threshold_mb: DEFAULT_MEMORY_THRESHOLD_MB,
            thread_threshold: DEFAULT_THREAD_THRESHOLD,
            alerts_enabled: true,
        }
    }
}

/// 资源状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceStatus {
    /// 内存使用量（MB）
    pub memory_mb: u64,

    /// 是否超过内存阈值
    pub memory_warning: bool,

    /// 线程数
    pub thread_count: u64,

    /// 是否超过线程数阈值
    pub thread_warning: bool,

    /// 检查时间
    pub checked_at: DateTime<Utc>,
}

/// 资源监控器
///
/// 监控内存使用和线程数，支持设置告警阈值。
///
/// # 注意
///
/// 由于跨平台兼容性考虑，当前实现使用估算方法获取资源使用情况。
/// 如需精确监控，可集成 `sysinfo` crate。
#[derive(Debug)]
pub struct ResourceMonitor {
    /// 配置
    config: RwLock<ResourceMonitorConfig>,

    /// 上次检查的资源状态
    last_status: RwLock<Option<ResourceStatus>>,

    /// 告警计数器
    alert_count: AtomicU64,
}

impl ResourceMonitor {
    /// 创建新的资源监控器
    pub fn new() -> Self {
        Self::with_config(ResourceMonitorConfig::default())
    }

    /// 使用配置创建资源监控器
    pub fn with_config(config: ResourceMonitorConfig) -> Self {
        Self {
            config: RwLock::new(config),
            last_status: RwLock::new(None),
            alert_count: AtomicU64::new(0),
        }
    }

    /// 更新配置
    pub fn update_config(&self, config: ResourceMonitorConfig) {
        if let Ok(mut cfg) = self.config.write() {
            *cfg = config;
        }
    }

    /// 获取当前配置
    pub fn get_config(&self) -> ResourceMonitorConfig {
        self.config
            .read()
            .map(|c| c.clone())
            .unwrap_or_default()
    }

    /// 设置内存告警阈值
    pub fn set_memory_threshold(&self, threshold_mb: u64) {
        if let Ok(mut config) = self.config.write() {
            config.memory_threshold_mb = threshold_mb;
        }
    }

    /// 设置线程数告警阈值
    pub fn set_thread_threshold(&self, threshold: u64) {
        if let Ok(mut config) = self.config.write() {
            config.thread_threshold = threshold;
        }
    }

    /// 启用/禁用告警
    pub fn set_alerts_enabled(&self, enabled: bool) {
        if let Ok(mut config) = self.config.write() {
            config.alerts_enabled = enabled;
        }
    }

    /// 检查资源使用情况
    ///
    /// 获取当前内存和线程使用情况，并检查是否超过阈值。
    /// 如果超过阈值且告警已启用，会记录警告日志。
    pub fn check_resources(&self) -> ResourceStatus {
        let config = self.get_config();

        // 获取内存使用情况（估算）
        let memory_mb = self.estimate_memory_usage();

        // 获取线程数（估算）
        let thread_count = self.estimate_thread_count();

        // 检查是否超过阈值
        let memory_warning = memory_mb > config.memory_threshold_mb;
        let thread_warning = thread_count > config.thread_threshold;

        // 如果告警已启用，记录警告
        if config.alerts_enabled {
            if memory_warning {
                self.alert_count.fetch_add(1, Ordering::Relaxed);
                warn!(
                    memory_mb = memory_mb,
                    threshold = config.memory_threshold_mb,
                    "内存使用超过告警阈值"
                );
            }

            if thread_warning {
                self.alert_count.fetch_add(1, Ordering::Relaxed);
                warn!(
                    thread_count = thread_count,
                    threshold = config.thread_threshold,
                    "线程数超过告警阈值"
                );
            }
        }

        let status = ResourceStatus {
            memory_mb,
            memory_warning,
            thread_count,
            thread_warning,
            checked_at: Utc::now(),
        };

        // 保存最新状态
        if let Ok(mut last) = self.last_status.write() {
            *last = Some(status.clone());
        }

        status
    }

    /// 获取上次检查的资源状态
    pub fn get_last_status(&self) -> Option<ResourceStatus> {
        self.last_status.read().ok().and_then(|s| s.clone())
    }

    /// 获取告警计数
    pub fn get_alert_count(&self) -> u64 {
        self.alert_count.load(Ordering::Relaxed)
    }

    /// 重置告警计数
    pub fn reset_alert_count(&self) {
        self.alert_count.store(0, Ordering::Relaxed);
    }

    /// 估算内存使用量（MB）
    ///
    /// 注意：这是一个简化实现，返回估算值。
    /// 如需精确值，应使用 sysinfo crate。
    fn estimate_memory_usage(&self) -> u64 {
        // 尝试从 /proc/self/statm 读取（Linux）
        #[cfg(target_os = "linux")]
        {
            if let Ok(content) = std::fs::read_to_string("/proc/self/statm") {
                let parts: Vec<&str> = content.split_whitespace().collect();
                if let Some(rss) = parts.get(1) {
                    if let Ok(pages) = rss.parse::<u64>() {
                        // 页面大小通常是 4KB
                        return pages * 4 / 1024;
                    }
                }
            }
        }

        // macOS 和其他平台返回估算值
        // 实际项目中应使用 sysinfo crate
        10 // 返回默认估算值（10MB）
    }

    /// 估算线程数
    ///
    /// 注意：这是一个简化实现，返回估算值。
    fn estimate_thread_count(&self) -> u64 {
        // 尝试从 /proc/self/status 读取（Linux）
        #[cfg(target_os = "linux")]
        {
            if let Ok(content) = std::fs::read_to_string("/proc/self/status") {
                for line in content.lines() {
                    if line.starts_with("Threads:") {
                        if let Some(count) = line.split_whitespace().nth(1) {
                            if let Ok(n) = count.parse::<u64>() {
                                return n;
                            }
                        }
                    }
                }
            }
        }

        // 其他平台返回估算值
        // 实际项目中应使用 sysinfo crate
        std::thread::available_parallelism()
            .map(|p| p.get() as u64)
            .unwrap_or(4)
    }
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ResourceMonitor {
    fn clone(&self) -> Self {
        Self {
            config: RwLock::new(self.get_config()),
            last_status: RwLock::new(self.get_last_status()),
            alert_count: AtomicU64::new(self.alert_count.load(Ordering::Relaxed)),
        }
    }
}

// ============================================================================
// 监控数据报告结构
// ============================================================================

/// 路由性能指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteMetrics {
    /// 总请求数
    pub total_requests: u64,

    /// 成功请求数
    pub successful_requests: u64,

    /// 失败请求数
    pub failed_requests: u64,

    /// 成功率（0.0 到 1.0）
    pub success_rate: f64,

    /// 失败率（0.0 到 1.0）
    pub failure_rate: f64,

    /// 每秒请求数
    pub throughput_per_second: f64,
}

/// 延迟统计指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyMetrics {
    /// 平均延迟（微秒）
    pub avg_latency_us: u64,

    /// 最小延迟（微秒）
    pub min_latency_us: u64,

    /// 最大延迟（微秒）
    pub max_latency_us: u64,

    /// P50 延迟（微秒）
    pub p50_latency_us: u64,

    /// P95 延迟（微秒）
    pub p95_latency_us: u64,

    /// P99 延迟（微秒）
    pub p99_latency_us: u64,
}

/// 资源使用指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMetrics {
    /// 内存使用量（MB）
    pub memory_mb: u64,

    /// 是否超过内存阈值
    pub memory_warning: bool,

    /// 线程数
    pub thread_count: u64,

    /// 是否超过线程数阈值
    pub thread_warning: bool,
}

/// 完整的监控报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsReport {
    /// 报告生成时间
    pub timestamp: DateTime<Utc>,

    /// 运行时长（秒）
    pub uptime_seconds: u64,

    /// 路由性能指标
    pub route_metrics: RouteMetrics,

    /// 延迟统计指标
    pub latency_metrics: LatencyMetrics,

    /// 资源使用指标（可选）
    pub resource_metrics: Option<ResourceMetrics>,
}

impl MetricsReport {
    /// 将报告序列化为 JSON 字符串
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// 将报告序列化为格式化的 JSON 字符串
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// 从 JSON 字符串反序列化
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

// ============================================================================
// 综合监控系统
// ============================================================================

/// 综合监控系统
///
/// 整合性能指标收集和资源监控功能。
#[derive(Debug, Clone)]
pub struct MonitoringSystem {
    /// 性能指标收集器
    pub metrics: Arc<MetricsCollector>,

    /// 资源监控器
    pub resources: Arc<ResourceMonitor>,
}

impl MonitoringSystem {
    /// 创建新的监控系统
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(MetricsCollector::new()),
            resources: Arc::new(ResourceMonitor::new()),
        }
    }

    /// 使用配置创建监控系统
    pub fn with_config(resource_config: ResourceMonitorConfig) -> Self {
        Self {
            metrics: Arc::new(MetricsCollector::new()),
            resources: Arc::new(ResourceMonitor::with_config(resource_config)),
        }
    }

    /// 记录路由请求
    pub fn record_route(&self, success: bool, latency_us: u64) {
        self.metrics.record_route(success, latency_us);
    }

    /// 检查资源使用情况
    pub fn check_resources(&self) -> ResourceStatus {
        self.resources.check_resources()
    }

    /// 导出完整的监控报告
    pub fn export(&self) -> MetricsReport {
        let mut report = self.metrics.export();

        // 添加资源指标
        if let Some(status) = self.resources.get_last_status() {
            report.resource_metrics = Some(ResourceMetrics {
                memory_mb: status.memory_mb,
                memory_warning: status.memory_warning,
                thread_count: status.thread_count,
                thread_warning: status.thread_warning,
            });
        }

        report
    }

    /// 重置所有统计
    pub fn reset(&self) {
        self.metrics.reset();
        self.resources.reset_alert_count();
    }
}

impl Default for MonitoringSystem {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    // ==================== MetricsCollector 测试 ====================

    #[test]
    fn test_metrics_collector_new() {
        let collector = MetricsCollector::new();
        assert_eq!(collector.get_total_requests(), 0);
        assert_eq!(collector.get_success_count(), 0);
        assert_eq!(collector.get_failure_count(), 0);
    }

    #[test]
    fn test_record_route_success() {
        let collector = MetricsCollector::new();

        collector.record_route(true, 1000);
        collector.record_route(true, 2000);

        assert_eq!(collector.get_total_requests(), 2);
        assert_eq!(collector.get_success_count(), 2);
        assert_eq!(collector.get_failure_count(), 0);
        assert_eq!(collector.get_success_rate(), 1.0);
    }

    #[test]
    fn test_record_route_failure() {
        let collector = MetricsCollector::new();

        collector.record_route(false, 1000);
        collector.record_route(true, 2000);

        assert_eq!(collector.get_total_requests(), 2);
        assert_eq!(collector.get_success_count(), 1);
        assert_eq!(collector.get_failure_count(), 1);
        assert!((collector.get_success_rate() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_average_latency() {
        let collector = MetricsCollector::new();

        collector.record_route(true, 1000);
        collector.record_route(true, 2000);
        collector.record_route(true, 3000);

        let avg = collector.get_average_latency();
        assert_eq!(avg, 2000);
    }

    #[test]
    fn test_min_max_latency() {
        let collector = MetricsCollector::new();

        collector.record_route(true, 1000);
        collector.record_route(true, 5000);
        collector.record_route(true, 3000);

        assert_eq!(collector.get_min_latency(), 1000);
        assert_eq!(collector.get_max_latency(), 5000);
    }

    #[test]
    fn test_percentile_calculation() {
        let collector = MetricsCollector::new();

        // 记录 100 个样本：100, 200, 300, ..., 10000
        for i in 1..=100 {
            collector.record_route(true, i * 100);
        }

        let p50 = collector.get_p50_latency();
        let p95 = collector.get_p95_latency();
        let p99 = collector.get_p99_latency();

        // P50 应该接近 5000
        assert!(p50 >= 4900 && p50 <= 5100, "P50={} 应该接近 5000", p50);

        // P95 应该接近 9500
        assert!(p95 >= 9400 && p95 <= 9600, "P95={} 应该接近 9500", p95);

        // P99 应该接近 9900
        assert!(p99 >= 9800 && p99 <= 10000, "P99={} 应该接近 9900", p99);
    }

    #[test]
    fn test_percentile_empty() {
        let collector = MetricsCollector::new();

        assert_eq!(collector.get_p50_latency(), 0);
        assert_eq!(collector.get_p95_latency(), 0);
        assert_eq!(collector.get_p99_latency(), 0);
    }

    #[test]
    fn test_percentile_single_sample() {
        let collector = MetricsCollector::new();
        collector.record_route(true, 1000);

        assert_eq!(collector.get_p50_latency(), 1000);
        assert_eq!(collector.get_p95_latency(), 1000);
        assert_eq!(collector.get_p99_latency(), 1000);
    }

    #[test]
    fn test_success_rate_no_requests() {
        let collector = MetricsCollector::new();
        assert_eq!(collector.get_success_rate(), 1.0);
    }

    #[test]
    fn test_reset() {
        let collector = MetricsCollector::new();

        collector.record_route(true, 1000);
        collector.record_route(false, 2000);

        assert_eq!(collector.get_total_requests(), 2);

        collector.reset();

        assert_eq!(collector.get_total_requests(), 0);
        assert_eq!(collector.get_success_count(), 0);
        assert_eq!(collector.get_failure_count(), 0);
        assert_eq!(collector.get_p50_latency(), 0);
    }

    #[test]
    fn test_thread_safety() {
        let collector = Arc::new(MetricsCollector::new());
        let mut handles = vec![];

        // 启动多个线程并发记录
        for _ in 0..10 {
            let collector_clone = Arc::clone(&collector);
            handles.push(thread::spawn(move || {
                for i in 0..100 {
                    collector_clone.record_route(i % 2 == 0, (i + 1) * 100);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // 验证总数
        assert_eq!(collector.get_total_requests(), 1000);
    }

    #[test]
    fn test_export_report() {
        let collector = MetricsCollector::new();

        collector.record_route(true, 1000);
        collector.record_route(true, 2000);
        collector.record_route(false, 3000);

        let report = collector.export();

        assert_eq!(report.route_metrics.total_requests, 3);
        assert_eq!(report.route_metrics.successful_requests, 2);
        assert_eq!(report.route_metrics.failed_requests, 1);
        assert_eq!(report.latency_metrics.avg_latency_us, 2000);
    }

    #[test]
    fn test_report_json_serialization() {
        let collector = MetricsCollector::new();
        collector.record_route(true, 1000);

        let report = collector.export();
        let json = report.to_json().unwrap();

        assert!(json.contains("total_requests"));
        assert!(json.contains("avg_latency_us"));

        // 验证可以反序列化
        let parsed = MetricsReport::from_json(&json).unwrap();
        assert_eq!(parsed.route_metrics.total_requests, 1);
    }

    #[test]
    fn test_clone() {
        let collector = MetricsCollector::new();
        collector.record_route(true, 1000);
        collector.record_route(true, 2000);

        let cloned = collector.clone();

        assert_eq!(cloned.get_total_requests(), 2);
        assert_eq!(cloned.get_average_latency(), 1500);
    }

    // ==================== ResourceMonitor 测试 ====================

    #[test]
    fn test_resource_monitor_new() {
        let monitor = ResourceMonitor::new();
        let config = monitor.get_config();

        assert_eq!(config.memory_threshold_mb, DEFAULT_MEMORY_THRESHOLD_MB);
        assert_eq!(config.thread_threshold, DEFAULT_THREAD_THRESHOLD);
        assert!(config.alerts_enabled);
    }

    #[test]
    fn test_resource_monitor_with_config() {
        let config = ResourceMonitorConfig {
            memory_threshold_mb: 256,
            thread_threshold: 50,
            alerts_enabled: false,
        };

        let monitor = ResourceMonitor::with_config(config.clone());
        let retrieved = monitor.get_config();

        assert_eq!(retrieved.memory_threshold_mb, 256);
        assert_eq!(retrieved.thread_threshold, 50);
        assert!(!retrieved.alerts_enabled);
    }

    #[test]
    fn test_update_config() {
        let monitor = ResourceMonitor::new();

        monitor.set_memory_threshold(128);
        monitor.set_thread_threshold(25);
        monitor.set_alerts_enabled(false);

        let config = monitor.get_config();
        assert_eq!(config.memory_threshold_mb, 128);
        assert_eq!(config.thread_threshold, 25);
        assert!(!config.alerts_enabled);
    }

    #[test]
    fn test_check_resources() {
        let monitor = ResourceMonitor::new();
        let status = monitor.check_resources();

        // 基本检查
        assert!(status.memory_mb > 0 || status.memory_mb == 10); // 估算值或实际值
        assert!(status.thread_count > 0);

        // 检查时间戳
        assert!(status.checked_at <= Utc::now());
    }

    #[test]
    fn test_get_last_status() {
        let monitor = ResourceMonitor::new();

        // 初始状态为 None
        assert!(monitor.get_last_status().is_none());

        // 检查后有状态
        monitor.check_resources();
        assert!(monitor.get_last_status().is_some());
    }

    #[test]
    fn test_alert_count() {
        let config = ResourceMonitorConfig {
            memory_threshold_mb: 0, // 设置为 0，肯定会触发告警
            thread_threshold: 0,
            alerts_enabled: true,
        };

        let monitor = ResourceMonitor::with_config(config);

        // 初始告警计数为 0
        assert_eq!(monitor.get_alert_count(), 0);

        // 检查资源会触发告警
        monitor.check_resources();
        assert!(monitor.get_alert_count() > 0);

        // 重置告警计数
        monitor.reset_alert_count();
        assert_eq!(monitor.get_alert_count(), 0);
    }

    #[test]
    fn test_alerts_disabled() {
        let config = ResourceMonitorConfig {
            memory_threshold_mb: 0,
            thread_threshold: 0,
            alerts_enabled: false,
        };

        let monitor = ResourceMonitor::with_config(config);
        monitor.check_resources();

        // 告警禁用时不应增加计数
        assert_eq!(monitor.get_alert_count(), 0);
    }

    // ==================== MonitoringSystem 测试 ====================

    #[test]
    fn test_monitoring_system_new() {
        let system = MonitoringSystem::new();

        assert_eq!(system.metrics.get_total_requests(), 0);
    }

    #[test]
    fn test_monitoring_system_record_route() {
        let system = MonitoringSystem::new();

        system.record_route(true, 1000);
        system.record_route(false, 2000);

        assert_eq!(system.metrics.get_total_requests(), 2);
    }

    #[test]
    fn test_monitoring_system_export() {
        let system = MonitoringSystem::new();

        system.record_route(true, 1000);
        system.check_resources();

        let report = system.export();

        assert_eq!(report.route_metrics.total_requests, 1);
        assert!(report.resource_metrics.is_some());
    }

    #[test]
    fn test_monitoring_system_reset() {
        let system = MonitoringSystem::new();

        system.record_route(true, 1000);

        system.reset();

        assert_eq!(system.metrics.get_total_requests(), 0);
    }

    // ==================== 边界条件测试 ====================

    #[test]
    fn test_percentile_boundary_values() {
        let collector = MetricsCollector::new();

        for i in 1..=10 {
            collector.record_route(true, i * 100);
        }

        // 测试边界值
        assert_eq!(collector.get_percentile(0.0), 100);
        assert_eq!(collector.get_percentile(1.0), 1000);
        assert_eq!(collector.get_percentile(-0.5), 100); // 被 clamp 到 0
        assert_eq!(collector.get_percentile(1.5), 1000); // 被 clamp 到 1
    }

    #[test]
    fn test_large_latency_values() {
        let collector = MetricsCollector::new();

        // 测试大延迟值
        collector.record_route(true, u64::MAX / 2);
        collector.record_route(true, 1000);

        assert_eq!(collector.get_total_requests(), 2);
        assert_eq!(collector.get_max_latency(), u64::MAX / 2);
    }

    #[test]
    fn test_sample_limit() {
        let collector = MetricsCollector::new();

        // 记录超过最大样本数的请求
        for i in 0..(MAX_LATENCY_SAMPLES + 100) {
            collector.record_route(true, (i as u64) * 100);
        }

        // 确保样本数被限制
        let samples = collector.latency_samples.lock().unwrap();
        assert!(samples.len() <= MAX_LATENCY_SAMPLES);
    }

    // ==================== JSON 序列化测试 ====================

    #[test]
    fn test_metrics_report_json_roundtrip() {
        let collector = MetricsCollector::new();

        for i in 1..=100 {
            collector.record_route(i % 10 != 0, i * 100);
        }

        let original = collector.export();
        let json = original.to_json_pretty().unwrap();
        let parsed = MetricsReport::from_json(&json).unwrap();

        assert_eq!(original.route_metrics.total_requests, parsed.route_metrics.total_requests);
        assert_eq!(
            original.latency_metrics.p95_latency_us,
            parsed.latency_metrics.p95_latency_us
        );
    }

    #[test]
    fn test_resource_config_serialization() {
        let config = ResourceMonitorConfig {
            memory_threshold_mb: 1024,
            thread_threshold: 200,
            alerts_enabled: true,
        };

        let json = serde_json::to_string(&config).unwrap();
        let parsed: ResourceMonitorConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.memory_threshold_mb, parsed.memory_threshold_mb);
        assert_eq!(config.thread_threshold, parsed.thread_threshold);
        assert_eq!(config.alerts_enabled, parsed.alerts_enabled);
    }
}
