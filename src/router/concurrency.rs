//! 并发控制和超时处理模块
//!
//! 本模块提供路由系统的并发控制和超时处理机制：
//! - `ConcurrencyLimiter`: 基于信号量的并发数限制
//! - `TimeoutManager`: 请求超时处理

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Semaphore, SemaphorePermit, OwnedSemaphorePermit};
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::utils::{status_code, CoreError, Result};
use super::request::{ErrorInfo, RouteResponse};

// ==================== 并发控制器 ====================

/// 并发限制器
///
/// 使用 tokio 的 Semaphore 实现对并发请求数量的控制。
/// 默认并发数为 CPU 核心数 × 2。
///
/// # Example
///
/// ```rust,no_run
/// use chips_core::router::ConcurrencyLimiter;
///
/// #[tokio::main]
/// async fn main() {
///     let limiter = ConcurrencyLimiter::new(10);
///     
///     // 获取许可（等待直到有可用许可）
///     let permit = limiter.acquire().await;
///     
///     // 执行并发受限的操作
///     do_something().await;
///     
///     // 许可在 drop 时自动释放
///     drop(permit);
/// }
///
/// async fn do_something() {}
/// ```
#[derive(Debug)]
pub struct ConcurrencyLimiter {
    /// 内部信号量
    semaphore: Arc<Semaphore>,
    /// 最大并发数
    max_concurrency: usize,
}

impl ConcurrencyLimiter {
    /// 创建新的并发限制器
    ///
    /// # Arguments
    ///
    /// * `max_concurrency` - 最大并发数
    ///
    /// # Example
    ///
    /// ```rust
    /// use chips_core::router::ConcurrencyLimiter;
    ///
    /// let limiter = ConcurrencyLimiter::new(100);
    /// assert_eq!(limiter.max_concurrency(), 100);
    /// ```
    pub fn new(max_concurrency: usize) -> Self {
        debug!(
            max_concurrency = max_concurrency,
            "Creating concurrency limiter"
        );
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
            max_concurrency,
        }
    }

    /// 创建具有默认并发数的限制器
    ///
    /// 默认并发数 = CPU 核心数 × 2
    ///
    /// # Example
    ///
    /// ```rust
    /// use chips_core::router::ConcurrencyLimiter;
    ///
    /// let limiter = ConcurrencyLimiter::default_concurrency();
    /// // 并发数根据 CPU 核心数自动设置
    /// ```
    pub fn default_concurrency() -> Self {
        let cpu_count = num_cpus();
        let max_concurrency = cpu_count * 2;
        debug!(
            cpu_count = cpu_count,
            max_concurrency = max_concurrency,
            "Creating concurrency limiter with default settings"
        );
        Self::new(max_concurrency)
    }

    /// 获取一个许可（等待直到有可用许可）
    ///
    /// 此方法会阻塞直到获取到一个许可。许可在被 drop 时自动释放。
    ///
    /// # Returns
    ///
    /// 返回一个许可，当许可被 drop 时会自动释放
    ///
    /// # Panics
    ///
    /// 如果信号量被关闭，此方法会 panic
    pub async fn acquire(&self) -> SemaphorePermit<'_> {
        self.semaphore
            .acquire()
            .await
            .expect("Semaphore should not be closed")
    }

    /// 尝试获取一个许可（非阻塞）
    ///
    /// 如果没有可用许可，立即返回 None。
    ///
    /// # Returns
    ///
    /// - `Some(permit)`: 成功获取许可
    /// - `None`: 没有可用许可
    ///
    /// # Example
    ///
    /// ```rust
    /// use chips_core::router::ConcurrencyLimiter;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let limiter = ConcurrencyLimiter::new(1);
    ///     
    ///     // 获取第一个许可
    ///     let permit1 = limiter.try_acquire();
    ///     assert!(permit1.is_some());
    ///     
    ///     // 尝试获取第二个许可（应该失败）
    ///     let permit2 = limiter.try_acquire();
    ///     assert!(permit2.is_none());
    /// }
    /// ```
    pub fn try_acquire(&self) -> Option<SemaphorePermit<'_>> {
        self.semaphore.try_acquire().ok()
    }

    /// 获取一个拥有所有权的许可（等待直到有可用许可）
    ///
    /// 与 `acquire()` 不同，返回的许可拥有信号量的所有权，
    /// 可以跨 await 点移动。
    ///
    /// # Returns
    ///
    /// 返回一个拥有所有权的许可
    pub async fn acquire_owned(&self) -> OwnedSemaphorePermit {
        Arc::clone(&self.semaphore)
            .acquire_owned()
            .await
            .expect("Semaphore should not be closed")
    }

    /// 尝试获取一个拥有所有权的许可（非阻塞）
    ///
    /// # Returns
    ///
    /// - `Some(permit)`: 成功获取许可
    /// - `None`: 没有可用许可
    pub fn try_acquire_owned(&self) -> Option<OwnedSemaphorePermit> {
        Arc::clone(&self.semaphore).try_acquire_owned().ok()
    }

    /// 获取当前正在使用的并发数
    ///
    /// # Example
    ///
    /// ```rust
    /// use chips_core::router::ConcurrencyLimiter;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let limiter = ConcurrencyLimiter::new(10);
    ///     assert_eq!(limiter.current_concurrency(), 0);
    ///     
    ///     let _permit = limiter.acquire().await;
    ///     assert_eq!(limiter.current_concurrency(), 1);
    /// }
    /// ```
    pub fn current_concurrency(&self) -> usize {
        self.max_concurrency - self.semaphore.available_permits()
    }

    /// 获取可用的许可数
    ///
    /// # Example
    ///
    /// ```rust
    /// use chips_core::router::ConcurrencyLimiter;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let limiter = ConcurrencyLimiter::new(10);
    ///     assert_eq!(limiter.available_permits(), 10);
    ///     
    ///     let _permit = limiter.acquire().await;
    ///     assert_eq!(limiter.available_permits(), 9);
    /// }
    /// ```
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// 获取最大并发数
    pub fn max_concurrency(&self) -> usize {
        self.max_concurrency
    }

    /// 获取内部信号量的克隆（用于共享）
    pub fn semaphore(&self) -> Arc<Semaphore> {
        Arc::clone(&self.semaphore)
    }

    /// 检查是否已达到最大并发数
    ///
    /// # Example
    ///
    /// ```rust
    /// use chips_core::router::ConcurrencyLimiter;
    ///
    /// let limiter = ConcurrencyLimiter::new(1);
    /// assert!(!limiter.is_at_capacity());
    /// ```
    pub fn is_at_capacity(&self) -> bool {
        self.available_permits() == 0
    }

    /// 获取并发使用率（0.0 - 1.0）
    ///
    /// # Example
    ///
    /// ```rust
    /// use chips_core::router::ConcurrencyLimiter;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let limiter = ConcurrencyLimiter::new(10);
    ///     assert_eq!(limiter.utilization(), 0.0);
    ///     
    ///     let _permit = limiter.acquire().await;
    ///     assert!((limiter.utilization() - 0.1).abs() < 0.001);
    /// }
    /// ```
    pub fn utilization(&self) -> f64 {
        self.current_concurrency() as f64 / self.max_concurrency as f64
    }
}

impl Default for ConcurrencyLimiter {
    fn default() -> Self {
        Self::default_concurrency()
    }
}

impl Clone for ConcurrencyLimiter {
    fn clone(&self) -> Self {
        Self {
            semaphore: Arc::clone(&self.semaphore),
            max_concurrency: self.max_concurrency,
        }
    }
}

// ==================== 超时管理器 ====================

/// 超时管理器
///
/// 提供请求超时处理功能，支持可配置的默认超时时间。
///
/// # Example
///
/// ```rust,no_run
/// use chips_core::router::TimeoutManager;
/// use std::time::Duration;
///
/// #[tokio::main]
/// async fn main() {
///     let manager = TimeoutManager::new(Duration::from_secs(30));
///     
///     let result = manager.with_timeout(
///         Duration::from_secs(5),
///         async { 42 }
///     ).await;
///     
///     assert_eq!(result.unwrap(), 42);
/// }
/// ```
#[derive(Debug, Clone)]
pub struct TimeoutManager {
    /// 默认超时时间
    default_timeout: Duration,
}

impl TimeoutManager {
    /// 创建新的超时管理器
    ///
    /// # Arguments
    ///
    /// * `default_timeout` - 默认超时时间
    pub fn new(default_timeout: Duration) -> Self {
        debug!(
            default_timeout_ms = default_timeout.as_millis(),
            "Creating timeout manager"
        );
        Self { default_timeout }
    }

    /// 创建具有默认超时时间（30秒）的管理器
    pub fn default_manager() -> Self {
        Self::new(Duration::from_secs(30))
    }

    /// 获取默认超时时间
    pub fn default_timeout(&self) -> Duration {
        self.default_timeout
    }

    /// 设置默认超时时间
    pub fn set_default_timeout(&mut self, timeout: Duration) {
        self.default_timeout = timeout;
    }

    /// 使用指定超时时间执行 Future
    ///
    /// # Arguments
    ///
    /// * `timeout_duration` - 超时时间
    /// * `future` - 要执行的 Future
    ///
    /// # Returns
    ///
    /// - `Ok(result)`: Future 在超时前完成
    /// - `Err(CoreError::Timeout)`: 超时
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use chips_core::router::TimeoutManager;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let manager = TimeoutManager::default_manager();
    ///     
    ///     // 正常完成
    ///     let result = manager.with_timeout(
    ///         Duration::from_secs(5),
    ///         async { "success" }
    ///     ).await;
    ///     assert!(result.is_ok());
    ///     
    ///     // 超时
    ///     let result = manager.with_timeout(
    ///         Duration::from_millis(1),
    ///         async {
    ///             tokio::time::sleep(Duration::from_secs(10)).await;
    ///             "never reached"
    ///         }
    ///     ).await;
    ///     assert!(result.is_err());
    /// }
    /// ```
    pub async fn with_timeout<F, T>(&self, timeout_duration: Duration, future: F) -> Result<T>
    where
        F: Future<Output = T>,
    {
        match timeout(timeout_duration, future).await {
            Ok(result) => Ok(result),
            Err(_elapsed) => {
                warn!(
                    timeout_ms = timeout_duration.as_millis(),
                    "Request timed out"
                );
                Err(CoreError::Timeout(format!(
                    "Operation timed out after {}ms",
                    timeout_duration.as_millis()
                )))
            }
        }
    }

    /// 使用默认超时时间执行 Future
    ///
    /// # Arguments
    ///
    /// * `future` - 要执行的 Future
    ///
    /// # Returns
    ///
    /// - `Ok(result)`: Future 在超时前完成
    /// - `Err(CoreError::Timeout)`: 超时
    pub async fn with_default_timeout<F, T>(&self, future: F) -> Result<T>
    where
        F: Future<Output = T>,
    {
        self.with_timeout(self.default_timeout, future).await
    }

    /// 使用超时执行 Future，超时时返回路由响应
    ///
    /// 这是专门为路由系统设计的方法，超时时直接返回格式化的错误响应。
    ///
    /// # Arguments
    ///
    /// * `request_id` - 请求 ID
    /// * `timeout_ms` - 超时时间（毫秒）
    /// * `future` - 返回 RouteResponse 的 Future
    ///
    /// # Returns
    ///
    /// 无论成功还是超时，都返回 RouteResponse
    pub async fn route_with_timeout<F>(
        &self,
        request_id: &str,
        timeout_ms: u64,
        future: F,
    ) -> RouteResponse
    where
        F: Future<Output = RouteResponse>,
    {
        let timeout_duration = Duration::from_millis(timeout_ms);

        match timeout(timeout_duration, future).await {
            Ok(response) => response,
            Err(_elapsed) => {
                warn!(
                    request_id = request_id,
                    timeout_ms = timeout_ms,
                    "Route request timed out"
                );
                RouteResponse::error(
                    request_id,
                    status_code::TIMEOUT,
                    ErrorInfo::new("TIMEOUT", format!("Request timed out after {}ms", timeout_ms)),
                    timeout_ms,
                )
            }
        }
    }
}

impl Default for TimeoutManager {
    fn default() -> Self {
        Self::default_manager()
    }
}

// ==================== 辅助函数 ====================

/// 获取 CPU 核心数
///
/// 使用 std::thread::available_parallelism 获取可用的并行度。
/// 如果无法获取，默认返回 4。
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

/// 使用超时执行 Future（独立函数版本）
///
/// # Arguments
///
/// * `timeout_duration` - 超时时间
/// * `future` - 要执行的 Future
///
/// # Returns
///
/// - `Ok(result)`: Future 在超时前完成
/// - `Err(CoreError::Timeout)`: 超时
///
/// # Example
///
/// ```rust,no_run
/// use chips_core::router::with_timeout;
/// use std::time::Duration;
///
/// #[tokio::main]
/// async fn main() {
///     let result = with_timeout(
///         Duration::from_secs(5),
///         async { 42 }
///     ).await;
///     
///     assert_eq!(result.unwrap(), 42);
/// }
/// ```
pub async fn with_timeout<F, T>(timeout_duration: Duration, future: F) -> Result<T>
where
    F: Future<Output = T>,
{
    match timeout(timeout_duration, future).await {
        Ok(result) => Ok(result),
        Err(_elapsed) => {
            warn!(
                timeout_ms = timeout_duration.as_millis(),
                "Operation timed out"
            );
            Err(CoreError::Timeout(format!(
                "Operation timed out after {}ms",
                timeout_duration.as_millis()
            )))
        }
    }
}

/// 使用超时执行路由请求（独立函数版本）
///
/// # Arguments
///
/// * `request_id` - 请求 ID
/// * `timeout_ms` - 超时时间（毫秒）
/// * `future` - 返回 RouteResponse 的 Future
///
/// # Returns
///
/// 无论成功还是超时，都返回 RouteResponse
pub async fn route_with_timeout<F>(request_id: &str, timeout_ms: u64, future: F) -> RouteResponse
where
    F: Future<Output = RouteResponse>,
{
    let timeout_duration = Duration::from_millis(timeout_ms);

    match timeout(timeout_duration, future).await {
        Ok(response) => response,
        Err(_elapsed) => {
            warn!(
                request_id = request_id,
                timeout_ms = timeout_ms,
                "Route request timed out"
            );
            RouteResponse::error(
                request_id,
                status_code::TIMEOUT,
                ErrorInfo::new("TIMEOUT", format!("Request timed out after {}ms", timeout_ms)),
                timeout_ms,
            )
        }
    }
}

// ==================== 单元测试 ====================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::time::sleep;

    // ==================== ConcurrencyLimiter 测试 ====================

    #[tokio::test]
    async fn test_concurrency_limiter_new() {
        let limiter = ConcurrencyLimiter::new(10);
        assert_eq!(limiter.max_concurrency(), 10);
        assert_eq!(limiter.available_permits(), 10);
        assert_eq!(limiter.current_concurrency(), 0);
    }

    #[tokio::test]
    async fn test_concurrency_limiter_default() {
        let limiter = ConcurrencyLimiter::default_concurrency();
        let expected = num_cpus() * 2;
        assert_eq!(limiter.max_concurrency(), expected);
    }

    #[tokio::test]
    async fn test_concurrency_limiter_acquire() {
        let limiter = ConcurrencyLimiter::new(2);
        assert_eq!(limiter.available_permits(), 2);
        assert_eq!(limiter.current_concurrency(), 0);

        let permit1 = limiter.acquire().await;
        assert_eq!(limiter.available_permits(), 1);
        assert_eq!(limiter.current_concurrency(), 1);

        let permit2 = limiter.acquire().await;
        assert_eq!(limiter.available_permits(), 0);
        assert_eq!(limiter.current_concurrency(), 2);

        // 释放 permit1
        drop(permit1);
        assert_eq!(limiter.available_permits(), 1);
        assert_eq!(limiter.current_concurrency(), 1);

        // 释放 permit2
        drop(permit2);
        assert_eq!(limiter.available_permits(), 2);
        assert_eq!(limiter.current_concurrency(), 0);
    }

    #[tokio::test]
    async fn test_concurrency_limiter_try_acquire() {
        let limiter = ConcurrencyLimiter::new(1);

        // 第一次尝试应该成功
        let permit1 = limiter.try_acquire();
        assert!(permit1.is_some());
        assert_eq!(limiter.available_permits(), 0);

        // 第二次尝试应该失败
        let permit2 = limiter.try_acquire();
        assert!(permit2.is_none());

        // 释放后再尝试应该成功
        drop(permit1);
        let permit3 = limiter.try_acquire();
        assert!(permit3.is_some());
    }

    #[tokio::test]
    async fn test_concurrency_limiter_acquire_owned() {
        let limiter = ConcurrencyLimiter::new(2);

        let permit1 = limiter.acquire_owned().await;
        assert_eq!(limiter.available_permits(), 1);

        let permit2 = limiter.acquire_owned().await;
        assert_eq!(limiter.available_permits(), 0);

        drop(permit1);
        assert_eq!(limiter.available_permits(), 1);

        drop(permit2);
        assert_eq!(limiter.available_permits(), 2);
    }

    #[tokio::test]
    async fn test_concurrency_limiter_try_acquire_owned() {
        let limiter = ConcurrencyLimiter::new(1);

        let permit1 = limiter.try_acquire_owned();
        assert!(permit1.is_some());

        let permit2 = limiter.try_acquire_owned();
        assert!(permit2.is_none());

        drop(permit1);
        let permit3 = limiter.try_acquire_owned();
        assert!(permit3.is_some());
    }

    #[tokio::test]
    async fn test_concurrency_limiter_is_at_capacity() {
        let limiter = ConcurrencyLimiter::new(1);
        assert!(!limiter.is_at_capacity());

        let _permit = limiter.acquire().await;
        assert!(limiter.is_at_capacity());
    }

    #[tokio::test]
    async fn test_concurrency_limiter_utilization() {
        let limiter = ConcurrencyLimiter::new(10);
        assert_eq!(limiter.utilization(), 0.0);

        let _permit1 = limiter.acquire().await;
        assert!((limiter.utilization() - 0.1).abs() < 0.001);

        let _permit2 = limiter.acquire().await;
        assert!((limiter.utilization() - 0.2).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_concurrency_limiter_clone() {
        let limiter1 = ConcurrencyLimiter::new(2);
        let limiter2 = limiter1.clone();

        // 两个 limiter 应该共享同一个信号量
        let _permit = limiter1.acquire().await;
        assert_eq!(limiter1.available_permits(), 1);
        assert_eq!(limiter2.available_permits(), 1);
    }

    #[tokio::test]
    async fn test_concurrency_limiter_concurrent_access() {
        let limiter = ConcurrencyLimiter::new(5);
        let counter = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));

        let mut handles = vec![];

        for _ in 0..20 {
            let limiter = limiter.clone();
            let counter = Arc::clone(&counter);
            let max_concurrent = Arc::clone(&max_concurrent);

            let handle = tokio::spawn(async move {
                let _permit = limiter.acquire().await;
                
                // 增加计数器
                let current = counter.fetch_add(1, Ordering::SeqCst) + 1;
                
                // 更新最大并发数
                loop {
                    let max = max_concurrent.load(Ordering::SeqCst);
                    if current <= max || max_concurrent.compare_exchange(
                        max,
                        current,
                        Ordering::SeqCst,
                        Ordering::SeqCst
                    ).is_ok() {
                        break;
                    }
                }
                
                // 模拟一些工作
                sleep(Duration::from_millis(10)).await;
                
                // 减少计数器
                counter.fetch_sub(1, Ordering::SeqCst);
            });

            handles.push(handle);
        }

        // 等待所有任务完成
        for handle in handles {
            handle.await.unwrap();
        }

        // 最大并发数不应超过限制
        assert!(max_concurrent.load(Ordering::SeqCst) <= 5);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    // ==================== TimeoutManager 测试 ====================

    #[tokio::test]
    async fn test_timeout_manager_new() {
        let manager = TimeoutManager::new(Duration::from_secs(10));
        assert_eq!(manager.default_timeout(), Duration::from_secs(10));
    }

    #[tokio::test]
    async fn test_timeout_manager_default() {
        let manager = TimeoutManager::default_manager();
        assert_eq!(manager.default_timeout(), Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_timeout_manager_set_default() {
        let mut manager = TimeoutManager::new(Duration::from_secs(10));
        manager.set_default_timeout(Duration::from_secs(20));
        assert_eq!(manager.default_timeout(), Duration::from_secs(20));
    }

    #[tokio::test]
    async fn test_timeout_manager_with_timeout_success() {
        let manager = TimeoutManager::default_manager();

        let result = manager
            .with_timeout(Duration::from_secs(5), async { 42 })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_timeout_manager_with_timeout_timeout() {
        let manager = TimeoutManager::default_manager();

        let result = manager
            .with_timeout(Duration::from_millis(10), async {
                sleep(Duration::from_secs(10)).await;
                42
            })
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::Timeout(_)));
    }

    #[tokio::test]
    async fn test_timeout_manager_with_default_timeout_success() {
        let manager = TimeoutManager::new(Duration::from_secs(5));

        let result = manager.with_default_timeout(async { "success" }).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
    }

    #[tokio::test]
    async fn test_timeout_manager_route_with_timeout_success() {
        let manager = TimeoutManager::default_manager();

        let response = manager
            .route_with_timeout("req-123", 5000, async {
                RouteResponse::success("req-123", serde_json::json!({"result": "ok"}), 10)
            })
            .await;

        assert!(response.is_success());
        assert_eq!(response.request_id, "req-123");
    }

    #[tokio::test]
    async fn test_timeout_manager_route_with_timeout_timeout() {
        let manager = TimeoutManager::default_manager();

        let response = manager
            .route_with_timeout("req-456", 10, async {
                sleep(Duration::from_secs(10)).await;
                RouteResponse::success("req-456", serde_json::json!({"result": "ok"}), 10)
            })
            .await;

        assert!(response.is_error());
        assert_eq!(response.status, status_code::TIMEOUT);
        assert_eq!(response.request_id, "req-456");
        assert!(response.error.is_some());
        let error = response.error.unwrap();
        assert_eq!(error.code, "TIMEOUT");
    }

    // ==================== 独立函数测试 ====================

    #[tokio::test]
    async fn test_with_timeout_function_success() {
        let result = with_timeout(Duration::from_secs(5), async { 100 }).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 100);
    }

    #[tokio::test]
    async fn test_with_timeout_function_timeout() {
        let result = with_timeout(Duration::from_millis(10), async {
            sleep(Duration::from_secs(10)).await;
            100
        })
        .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::Timeout(_)));
    }

    #[tokio::test]
    async fn test_route_with_timeout_function_success() {
        let response = route_with_timeout("req-789", 5000, async {
            RouteResponse::success("req-789", serde_json::json!({"data": 123}), 5)
        })
        .await;

        assert!(response.is_success());
        assert_eq!(response.request_id, "req-789");
    }

    #[tokio::test]
    async fn test_route_with_timeout_function_timeout() {
        let response = route_with_timeout("req-000", 10, async {
            sleep(Duration::from_secs(10)).await;
            RouteResponse::success("req-000", serde_json::json!({}), 0)
        })
        .await;

        assert!(response.is_error());
        assert_eq!(response.status, status_code::TIMEOUT);
    }

    // ==================== 集成测试 ====================

    #[tokio::test]
    async fn test_concurrency_with_timeout() {
        let limiter = ConcurrencyLimiter::new(2);
        let manager = TimeoutManager::new(Duration::from_millis(100));

        let results = Arc::new(tokio::sync::Mutex::new(vec![]));

        let mut handles = vec![];

        for i in 0..5 {
            let limiter = limiter.clone();
            let manager = manager.clone();
            let results = Arc::clone(&results);

            let handle = tokio::spawn(async move {
                let _permit = limiter.acquire().await;
                
                let result = manager
                    .with_timeout(Duration::from_millis(50), async move {
                        // 模拟快速操作
                        i
                    })
                    .await;

                results.lock().await.push(result);
            });

            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let final_results = results.lock().await;
        assert_eq!(final_results.len(), 5);
        
        // 所有结果都应该成功（因为操作很快）
        for result in final_results.iter() {
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_num_cpus() {
        let cpus = num_cpus();
        assert!(cpus > 0);
    }
}
