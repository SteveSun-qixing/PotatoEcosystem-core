//! 事件总线系统
//!
//! 提供模块间的松耦合通信机制，支持事件的发布、订阅和异步分发。
//!
//! # 主要功能
//!
//! - **事件发布**: 支持异步和同步发布模式
//! - **事件订阅**: 支持按事件类型订阅，可附加过滤器
//! - **并发分发**: 使用 tokio 进行并发事件分发
//! - **订阅者隔离**: 单个订阅者的错误不影响其他订阅者
//! - **超时控制**: 防止单个订阅者阻塞整个系统
//!
//! # 使用示例
//!
//! ```ignore
//! use chips_core::router::{EventBus, Event, EventFilter};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() {
//!     let event_bus = EventBus::new();
//!
//!     // 订阅事件
//!     let subscription_id = event_bus.subscribe(
//!         "my_module",
//!         "user.*",
//!         None,
//!         Arc::new(|event| {
//!             println!("收到事件: {:?}", event);
//!         }),
//!     ).await.unwrap();
//!
//!     // 发布事件
//!     let event = Event::new("user.login", "auth_module", serde_json::json!({"user_id": "123"}));
//!     event_bus.publish(event).await.unwrap();
//!
//!     // 取消订阅
//!     event_bus.unsubscribe(&subscription_id).await.unwrap();
//! }
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use tokio::time::timeout;
use tracing::{debug, info, trace, warn};

use crate::router::event::{Event, EventFilter};
use crate::utils::{generate_id, CoreError, Result};

/// 默认订阅者处理超时时间（秒）
const DEFAULT_HANDLER_TIMEOUT_SECS: u64 = 5;

/// 事件回调函数类型
///
/// 回调函数必须是线程安全的，可以在多个线程中并发调用。
pub type EventCallback = Arc<dyn Fn(Event) + Send + Sync>;

/// 内部订阅条目
///
/// 包含订阅的完整信息，包括回调函数。
/// 这是一个内部结构，不对外暴露。
#[derive(Clone)]
struct SubscriptionEntry {
    /// 订阅唯一标识
    subscription_id: String,

    /// 订阅者模块 ID（用于按模块管理订阅）
    #[allow(dead_code)]
    subscriber_id: String,

    /// 订阅的事件类型（支持通配符）
    event_type: String,

    /// 事件过滤器
    filter: Option<EventFilter>,

    /// 事件回调函数
    callback: EventCallback,

    /// 订阅时间（用于调试和审计）
    #[allow(dead_code)]
    subscribed_at: DateTime<Utc>,

    /// 是否启用
    enabled: bool,
}

impl SubscriptionEntry {
    /// 创建新的订阅条目
    fn new(
        subscriber_id: impl Into<String>,
        event_type: impl Into<String>,
        filter: Option<EventFilter>,
        callback: EventCallback,
    ) -> Self {
        Self {
            subscription_id: generate_id(),
            subscriber_id: subscriber_id.into(),
            event_type: event_type.into(),
            filter,
            callback,
            subscribed_at: Utc::now(),
            enabled: true,
        }
    }

    /// 检查事件是否匹配此订阅
    fn matches(&self, event: &Event) -> bool {
        if !self.enabled {
            return false;
        }

        // 检查事件类型匹配
        if !Self::matches_pattern(&self.event_type, &event.event_type) {
            return false;
        }

        // 应用过滤器
        if let Some(ref filter) = self.filter {
            return filter.matches(event);
        }

        true
    }

    /// 匹配模式（支持 * 通配符）
    fn matches_pattern(pattern: &str, value: &str) -> bool {
        if pattern == "*" {
            return true;
        }

        if pattern.ends_with(".*") {
            let prefix = &pattern[..pattern.len() - 2];
            return value.starts_with(prefix) && value.len() > prefix.len();
        }

        pattern == value
    }
}

/// 分发统计信息
#[derive(Debug, Clone, Default)]
pub struct DispatchStats {
    /// 总分发次数
    pub total_dispatched: u64,

    /// 成功分发次数
    pub successful: u64,

    /// 失败分发次数
    pub failed: u64,

    /// 超时次数
    pub timeouts: u64,

    /// 最后分发时间
    pub last_dispatch_at: Option<DateTime<Utc>>,
}

/// 事件总线配置
#[derive(Debug, Clone)]
pub struct EventBusConfig {
    /// 订阅者处理超时时间
    pub handler_timeout: Duration,

    /// 是否启用并发分发
    pub concurrent_dispatch: bool,

    /// 最大并发分发数
    pub max_concurrent_handlers: usize,
}

impl Default for EventBusConfig {
    fn default() -> Self {
        Self {
            handler_timeout: Duration::from_secs(DEFAULT_HANDLER_TIMEOUT_SECS),
            concurrent_dispatch: true,
            max_concurrent_handlers: 100,
        }
    }
}

/// 事件总线
///
/// 事件总线是模块间通信的核心组件，提供发布-订阅模式的事件机制。
/// 使用 `Arc<RwLock>` 保证线程安全，支持高并发的事件发布和订阅。
#[derive(Clone)]
pub struct EventBus {
    /// 订阅列表：事件类型 -> 订阅条目列表
    subscriptions: Arc<RwLock<HashMap<String, Vec<SubscriptionEntry>>>>,

    /// 所有订阅的快速查找表：订阅 ID -> (事件类型, 订阅者 ID)
    subscription_index: Arc<RwLock<HashMap<String, (String, String)>>>,

    /// 分发统计
    stats: Arc<RwLock<DispatchStats>>,

    /// 配置
    config: EventBusConfig,
}

impl EventBus {
    /// 创建新的事件总线
    ///
    /// # 示例
    ///
    /// ```ignore
    /// use chips_core::router::EventBus;
    ///
    /// let event_bus = EventBus::new();
    /// ```
    pub fn new() -> Self {
        Self::with_config(EventBusConfig::default())
    }

    /// 使用自定义配置创建事件总线
    ///
    /// # 参数
    ///
    /// * `config` - 事件总线配置
    ///
    /// # 示例
    ///
    /// ```ignore
    /// use chips_core::router::{EventBus, EventBusConfig};
    /// use std::time::Duration;
    ///
    /// let config = EventBusConfig {
    ///     handler_timeout: Duration::from_secs(10),
    ///     ..Default::default()
    /// };
    /// let event_bus = EventBus::with_config(config);
    /// ```
    pub fn with_config(config: EventBusConfig) -> Self {
        info!(
            "创建事件总线: timeout={:?}, concurrent={}",
            config.handler_timeout, config.concurrent_dispatch
        );
        Self {
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            subscription_index: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(DispatchStats::default())),
            config,
        }
    }

    /// 订阅事件
    ///
    /// # 参数
    ///
    /// * `subscriber_id` - 订阅者模块 ID
    /// * `event_type` - 要订阅的事件类型（支持通配符，如 `module.*`）
    /// * `filter` - 可选的事件过滤器
    /// * `callback` - 事件回调函数
    ///
    /// # 返回
    ///
    /// 返回订阅 ID，用于后续取消订阅
    ///
    /// # 示例
    ///
    /// ```ignore
    /// let subscription_id = event_bus.subscribe(
    ///     "my_module",
    ///     "user.*",
    ///     None,
    ///     Arc::new(|event| {
    ///         println!("收到事件: {:?}", event);
    ///     }),
    /// ).await?;
    /// ```
    pub async fn subscribe(
        &self,
        subscriber_id: impl Into<String>,
        event_type: impl Into<String>,
        filter: Option<EventFilter>,
        callback: EventCallback,
    ) -> Result<String> {
        let subscriber_id = subscriber_id.into();
        let event_type = event_type.into();

        let entry = SubscriptionEntry::new(
            subscriber_id.clone(),
            event_type.clone(),
            filter,
            callback,
        );
        let subscription_id = entry.subscription_id.clone();

        // 添加到订阅列表
        {
            let mut subscriptions = self.subscriptions.write().await;
            subscriptions
                .entry(event_type.clone())
                .or_insert_with(Vec::new)
                .push(entry);
        }

        // 添加到索引
        {
            let mut index = self.subscription_index.write().await;
            index.insert(
                subscription_id.clone(),
                (event_type.clone(), subscriber_id.clone()),
            );
        }

        info!(
            subscription_id = %subscription_id,
            subscriber_id = %subscriber_id,
            event_type = %event_type,
            "事件订阅成功"
        );

        Ok(subscription_id)
    }

    /// 取消订阅
    ///
    /// # 参数
    ///
    /// * `subscription_id` - 要取消的订阅 ID
    ///
    /// # 返回
    ///
    /// 如果订阅存在并成功取消，返回 `Ok(())`
    ///
    /// # 错误
    ///
    /// 如果订阅不存在，返回 `CoreError::SubscriptionNotFound`
    pub async fn unsubscribe(&self, subscription_id: &str) -> Result<()> {
        // 从索引中获取事件类型
        let (event_type, subscriber_id) = {
            let index = self.subscription_index.read().await;
            index
                .get(subscription_id)
                .cloned()
                .ok_or_else(|| CoreError::SubscriptionNotFound(subscription_id.to_string()))?
        };

        // 从订阅列表中移除
        {
            let mut subscriptions = self.subscriptions.write().await;
            if let Some(subs) = subscriptions.get_mut(&event_type) {
                subs.retain(|s| s.subscription_id != subscription_id);
                // 如果该事件类型没有订阅了，移除整个条目
                if subs.is_empty() {
                    subscriptions.remove(&event_type);
                }
            }
        }

        // 从索引中移除
        {
            let mut index = self.subscription_index.write().await;
            index.remove(subscription_id);
        }

        info!(
            subscription_id = %subscription_id,
            subscriber_id = %subscriber_id,
            event_type = %event_type,
            "取消订阅成功"
        );

        Ok(())
    }

    /// 取消模块的所有订阅
    ///
    /// # 参数
    ///
    /// * `subscriber_id` - 要取消订阅的模块 ID
    ///
    /// # 返回
    ///
    /// 返回取消的订阅数量
    pub async fn unsubscribe_all(&self, subscriber_id: &str) -> Result<usize> {
        let mut removed_count = 0;

        // 收集要移除的订阅 ID
        let subscription_ids_to_remove: Vec<String> = {
            let index = self.subscription_index.read().await;
            index
                .iter()
                .filter(|(_, (_, sub_id))| sub_id == subscriber_id)
                .map(|(id, _)| id.clone())
                .collect()
        };

        // 移除订阅
        for subscription_id in subscription_ids_to_remove {
            if self.unsubscribe(&subscription_id).await.is_ok() {
                removed_count += 1;
            }
        }

        info!(
            subscriber_id = %subscriber_id,
            removed_count = removed_count,
            "取消模块所有订阅"
        );

        Ok(removed_count)
    }

    /// 异步发布事件
    ///
    /// 事件将被异步分发给所有匹配的订阅者，不等待处理完成。
    /// 适用于不需要等待结果的场景。
    ///
    /// # 参数
    ///
    /// * `event` - 要发布的事件
    ///
    /// # 返回
    ///
    /// 返回匹配的订阅者数量
    pub async fn publish(&self, event: Event) -> Result<usize> {
        let event_type = &event.event_type;
        trace!(
            event_id = %event.event_id,
            event_type = %event_type,
            sender = %event.sender,
            "发布事件"
        );

        // 收集所有匹配的订阅
        let matching_subscriptions = self.find_matching_subscriptions(&event).await;

        if matching_subscriptions.is_empty() {
            debug!(
                event_id = %event.event_id,
                event_type = %event_type,
                "没有匹配的订阅者"
            );
            return Ok(0);
        }

        let subscriber_count = matching_subscriptions.len();

        // 异步分发事件
        if self.config.concurrent_dispatch {
            self.dispatch_concurrent(event, matching_subscriptions)
                .await;
        } else {
            self.dispatch_sequential(event, matching_subscriptions)
                .await;
        }

        Ok(subscriber_count)
    }

    /// 同步发布事件
    ///
    /// 事件将被分发给所有匹配的订阅者，等待所有处理完成后返回。
    /// 适用于需要确保事件处理完成的场景。
    ///
    /// # 参数
    ///
    /// * `event` - 要发布的事件
    ///
    /// # 返回
    ///
    /// 返回 `(成功数, 失败数, 超时数)` 元组
    pub async fn publish_sync(&self, event: Event) -> Result<(usize, usize, usize)> {
        let event_type = &event.event_type;
        debug!(
            event_id = %event.event_id,
            event_type = %event_type,
            sender = %event.sender,
            "同步发布事件"
        );

        // 收集所有匹配的订阅
        let matching_subscriptions = self.find_matching_subscriptions(&event).await;

        if matching_subscriptions.is_empty() {
            debug!(
                event_id = %event.event_id,
                event_type = %event_type,
                "没有匹配的订阅者"
            );
            return Ok((0, 0, 0));
        }

        // 同步分发并收集结果
        let results = self
            .dispatch_concurrent_with_results(event, matching_subscriptions)
            .await;

        let successful = results.iter().filter(|r| matches!(r, DispatchResult::Success)).count();
        let failed = results.iter().filter(|r| matches!(r, DispatchResult::Failed(_))).count();
        let timeouts = results.iter().filter(|r| matches!(r, DispatchResult::Timeout)).count();

        // 更新统计
        {
            let mut stats = self.stats.write().await;
            stats.total_dispatched += results.len() as u64;
            stats.successful += successful as u64;
            stats.failed += failed as u64;
            stats.timeouts += timeouts as u64;
            stats.last_dispatch_at = Some(Utc::now());
        }

        info!(
            successful = successful,
            failed = failed,
            timeouts = timeouts,
            "同步事件分发完成"
        );

        Ok((successful, failed, timeouts))
    }

    /// 查找所有匹配的订阅
    async fn find_matching_subscriptions(&self, event: &Event) -> Vec<SubscriptionEntry> {
        let subscriptions = self.subscriptions.read().await;
        let mut matching = Vec::new();

        // 遍历所有订阅，找出匹配的
        for (event_type_pattern, subs) in subscriptions.iter() {
            // 检查事件类型模式是否匹配
            if SubscriptionEntry::matches_pattern(event_type_pattern, &event.event_type) {
                for sub in subs {
                    if sub.matches(event) {
                        matching.push(sub.clone());
                    }
                }
            }
        }

        // 也检查通配符订阅
        if let Some(wildcard_subs) = subscriptions.get("*") {
            for sub in wildcard_subs {
                if sub.matches(event) {
                    matching.push(sub.clone());
                }
            }
        }

        matching
    }

    /// 并发分发事件（不等待结果）
    async fn dispatch_concurrent(&self, event: Event, subscriptions: Vec<SubscriptionEntry>) {
        for sub in subscriptions {
            let event_clone = event.clone();
            let callback = sub.callback.clone();
            let timeout_duration = self.config.handler_timeout;
            let subscription_id = sub.subscription_id.clone();
            let stats = self.stats.clone();

            tokio::spawn(async move {
                let result = Self::invoke_callback_with_timeout(
                    callback,
                    event_clone,
                    timeout_duration,
                )
                .await;

                // 更新统计
                let mut stats = stats.write().await;
                stats.total_dispatched += 1;
                stats.last_dispatch_at = Some(Utc::now());

                match result {
                    DispatchResult::Success => {
                        stats.successful += 1;
                        trace!(subscription_id = %subscription_id, "事件处理成功");
                    }
                    DispatchResult::Failed(e) => {
                        stats.failed += 1;
                        warn!(
                            subscription_id = %subscription_id,
                            error = %e,
                            "事件处理失败"
                        );
                    }
                    DispatchResult::Timeout => {
                        stats.timeouts += 1;
                        warn!(subscription_id = %subscription_id, "事件处理超时");
                    }
                }
            });
        }
    }

    /// 顺序分发事件
    async fn dispatch_sequential(&self, event: Event, subscriptions: Vec<SubscriptionEntry>) {
        for sub in subscriptions {
            let event_clone = event.clone();
            let callback = sub.callback.clone();
            let timeout_duration = self.config.handler_timeout;

            let result =
                Self::invoke_callback_with_timeout(callback, event_clone, timeout_duration).await;

            // 更新统计
            let mut stats = self.stats.write().await;
            stats.total_dispatched += 1;
            stats.last_dispatch_at = Some(Utc::now());

            match result {
                DispatchResult::Success => {
                    stats.successful += 1;
                }
                DispatchResult::Failed(e) => {
                    stats.failed += 1;
                    warn!(
                        subscription_id = %sub.subscription_id,
                        error = %e,
                        "事件处理失败"
                    );
                }
                DispatchResult::Timeout => {
                    stats.timeouts += 1;
                    warn!(subscription_id = %sub.subscription_id, "事件处理超时");
                }
            }
        }
    }

    /// 并发分发事件并等待结果
    async fn dispatch_concurrent_with_results(
        &self,
        event: Event,
        subscriptions: Vec<SubscriptionEntry>,
    ) -> Vec<DispatchResult> {
        let timeout_duration = self.config.handler_timeout;

        let tasks: Vec<_> = subscriptions
            .into_iter()
            .map(|sub| {
                let event_clone = event.clone();
                let callback = sub.callback.clone();
                let subscription_id = sub.subscription_id.clone();

                tokio::spawn(async move {
                    let result = Self::invoke_callback_with_timeout(
                        callback,
                        event_clone,
                        timeout_duration,
                    )
                    .await;

                    match &result {
                        DispatchResult::Success => {
                            trace!(subscription_id = %subscription_id, "事件处理成功");
                        }
                        DispatchResult::Failed(e) => {
                            warn!(
                                subscription_id = %subscription_id,
                                error = %e,
                                "事件处理失败"
                            );
                        }
                        DispatchResult::Timeout => {
                            warn!(subscription_id = %subscription_id, "事件处理超时");
                        }
                    }

                    result
                })
            })
            .collect();

        // 等待所有任务完成
        let results = futures::future::join_all(tasks).await;

        // 处理 JoinError
        results
            .into_iter()
            .map(|r| match r {
                Ok(result) => result,
                Err(e) => DispatchResult::Failed(format!("任务执行失败: {}", e)),
            })
            .collect()
    }

    /// 带超时的回调调用
    ///
    /// 由于回调函数是同步的，我们使用 `spawn_blocking` 在专用线程中执行，
    /// 然后用 `timeout` 包装来实现超时控制。如果超时，回调可能仍在运行，
    /// 但我们不再等待其结果。
    async fn invoke_callback_with_timeout(
        callback: EventCallback,
        event: Event,
        timeout_duration: Duration,
    ) -> DispatchResult {
        let result = timeout(timeout_duration, async move {
            // 使用 spawn_blocking 在专用线程中执行同步回调
            tokio::task::spawn_blocking(move || {
                // 使用 catch_unwind 捕获 panic
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    callback(event);
                }))
            })
            .await
        })
        .await;

        match result {
            // 正常完成且没有 panic
            Ok(Ok(Ok(()))) => DispatchResult::Success,
            // 回调 panic
            Ok(Ok(Err(_))) => DispatchResult::Failed("回调函数 panic".to_string()),
            // spawn_blocking 任务失败（JoinError）
            Ok(Err(e)) => DispatchResult::Failed(format!("任务执行失败: {}", e)),
            // 超时
            Err(_) => DispatchResult::Timeout,
        }
    }

    /// 获取订阅数量
    ///
    /// # 返回
    ///
    /// 返回当前的总订阅数量
    pub async fn subscription_count(&self) -> usize {
        let index = self.subscription_index.read().await;
        index.len()
    }

    /// 获取指定事件类型的订阅数量
    ///
    /// # 参数
    ///
    /// * `event_type` - 事件类型
    ///
    /// # 返回
    ///
    /// 返回该事件类型的订阅数量
    pub async fn subscription_count_for_type(&self, event_type: &str) -> usize {
        let subscriptions = self.subscriptions.read().await;
        subscriptions.get(event_type).map_or(0, |v| v.len())
    }

    /// 获取分发统计信息
    ///
    /// # 返回
    ///
    /// 返回当前的分发统计信息副本
    pub async fn stats(&self) -> DispatchStats {
        self.stats.read().await.clone()
    }

    /// 重置统计信息
    pub async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        *stats = DispatchStats::default();
    }

    /// 检查是否有订阅者订阅了指定的事件类型
    ///
    /// # 参数
    ///
    /// * `event_type` - 要检查的事件类型
    ///
    /// # 返回
    ///
    /// 如果有订阅者订阅了该事件类型，返回 `true`
    pub async fn has_subscribers(&self, event_type: &str) -> bool {
        let subscriptions = self.subscriptions.read().await;

        // 检查精确匹配
        if subscriptions.get(event_type).map_or(false, |v| !v.is_empty()) {
            return true;
        }

        // 检查通配符匹配
        for (pattern, subs) in subscriptions.iter() {
            if !subs.is_empty() && SubscriptionEntry::matches_pattern(pattern, event_type) {
                return true;
            }
        }

        false
    }

    /// 获取模块的所有订阅 ID
    ///
    /// # 参数
    ///
    /// * `subscriber_id` - 模块 ID
    ///
    /// # 返回
    ///
    /// 返回该模块的所有订阅 ID 列表
    pub async fn get_subscriptions_for_module(&self, subscriber_id: &str) -> Vec<String> {
        let index = self.subscription_index.read().await;
        index
            .iter()
            .filter(|(_, (_, sub_id))| sub_id == subscriber_id)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// 启用或禁用订阅
    ///
    /// # 参数
    ///
    /// * `subscription_id` - 订阅 ID
    /// * `enabled` - 是否启用
    ///
    /// # 返回
    ///
    /// 如果订阅存在并成功修改，返回 `Ok(())`
    pub async fn set_subscription_enabled(
        &self,
        subscription_id: &str,
        enabled: bool,
    ) -> Result<()> {
        // 从索引中获取事件类型
        let (event_type, _) = {
            let index = self.subscription_index.read().await;
            index
                .get(subscription_id)
                .cloned()
                .ok_or_else(|| CoreError::SubscriptionNotFound(subscription_id.to_string()))?
        };

        // 修改订阅状态
        {
            let mut subscriptions = self.subscriptions.write().await;
            if let Some(subs) = subscriptions.get_mut(&event_type) {
                for sub in subs.iter_mut() {
                    if sub.subscription_id == subscription_id {
                        sub.enabled = enabled;
                        debug!(
                            subscription_id = %subscription_id,
                            enabled = enabled,
                            "订阅状态已修改"
                        );
                        return Ok(());
                    }
                }
            }
        }

        Err(CoreError::SubscriptionNotFound(subscription_id.to_string()))
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// 分发结果
#[derive(Debug, Clone)]
enum DispatchResult {
    /// 成功
    Success,
    /// 失败
    Failed(String),
    /// 超时
    Timeout,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_event_bus_creation() {
        let bus = EventBus::new();
        assert_eq!(bus.subscription_count().await, 0);
    }

    #[tokio::test]
    async fn test_event_bus_with_config() {
        let config = EventBusConfig {
            handler_timeout: Duration::from_secs(10),
            concurrent_dispatch: false,
            max_concurrent_handlers: 50,
        };
        let bus = EventBus::with_config(config);
        assert_eq!(bus.config.handler_timeout, Duration::from_secs(10));
        assert!(!bus.config.concurrent_dispatch);
    }

    #[tokio::test]
    async fn test_subscribe_and_unsubscribe() {
        let bus = EventBus::new();

        // 订阅
        let sub_id = bus
            .subscribe("test_module", "test.event", None, Arc::new(|_| {}))
            .await
            .unwrap();

        assert_eq!(bus.subscription_count().await, 1);

        // 取消订阅
        bus.unsubscribe(&sub_id).await.unwrap();
        assert_eq!(bus.subscription_count().await, 0);
    }

    #[tokio::test]
    async fn test_unsubscribe_not_found() {
        let bus = EventBus::new();
        let result = bus.unsubscribe("non_existent").await;
        assert!(matches!(result, Err(CoreError::SubscriptionNotFound(_))));
    }

    #[tokio::test]
    async fn test_unsubscribe_all() {
        let bus = EventBus::new();

        // 订阅多个事件
        bus.subscribe("module_a", "event1", None, Arc::new(|_| {}))
            .await
            .unwrap();
        bus.subscribe("module_a", "event2", None, Arc::new(|_| {}))
            .await
            .unwrap();
        bus.subscribe("module_b", "event1", None, Arc::new(|_| {}))
            .await
            .unwrap();

        assert_eq!(bus.subscription_count().await, 3);

        // 取消 module_a 的所有订阅
        let removed = bus.unsubscribe_all("module_a").await.unwrap();
        assert_eq!(removed, 2);
        assert_eq!(bus.subscription_count().await, 1);
    }

    #[tokio::test]
    async fn test_publish_event() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // 订阅
        bus.subscribe(
            "test_module",
            "test.event",
            None,
            Arc::new(move |_| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .await
        .unwrap();

        // 发布
        let event = Event::new("test.event", "sender", json!({}));
        let matched = bus.publish(event).await.unwrap();

        assert_eq!(matched, 1);

        // 等待异步处理完成
        sleep(Duration::from_millis(100)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_publish_sync() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // 订阅
        bus.subscribe(
            "test_module",
            "test.event",
            None,
            Arc::new(move |_| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .await
        .unwrap();

        // 同步发布
        let event = Event::new("test.event", "sender", json!({}));
        let (successful, failed, timeouts) = bus.publish_sync(event).await.unwrap();

        assert_eq!(successful, 1);
        assert_eq!(failed, 0);
        assert_eq!(timeouts, 0);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_wildcard_subscription() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // 订阅通配符
        bus.subscribe(
            "test_module",
            "module.*",
            None,
            Arc::new(move |_| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .await
        .unwrap();

        // 发布匹配的事件
        let event1 = Event::new("module.loaded", "sender", json!({}));
        bus.publish_sync(event1).await.unwrap();

        let event2 = Event::new("module.started", "sender", json!({}));
        bus.publish_sync(event2).await.unwrap();

        // 发布不匹配的事件
        let event3 = Event::new("config.changed", "sender", json!({}));
        bus.publish_sync(event3).await.unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_event_filter() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // 订阅带过滤器
        let filter = EventFilter::by_sender("allowed_sender");
        bus.subscribe(
            "test_module",
            "test.event",
            Some(filter),
            Arc::new(move |_| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .await
        .unwrap();

        // 发布匹配的事件
        let event1 = Event::new("test.event", "allowed_sender", json!({}));
        bus.publish_sync(event1).await.unwrap();

        // 发布不匹配的事件
        let event2 = Event::new("test.event", "other_sender", json!({}));
        bus.publish_sync(event2).await.unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = EventBus::new();
        let counter1 = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::new(AtomicUsize::new(0));
        let counter1_clone = counter1.clone();
        let counter2_clone = counter2.clone();

        // 多个订阅者
        bus.subscribe(
            "module_a",
            "test.event",
            None,
            Arc::new(move |_| {
                counter1_clone.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .await
        .unwrap();

        bus.subscribe(
            "module_b",
            "test.event",
            None,
            Arc::new(move |_| {
                counter2_clone.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .await
        .unwrap();

        // 发布
        let event = Event::new("test.event", "sender", json!({}));
        let (successful, _, _) = bus.publish_sync(event).await.unwrap();

        assert_eq!(successful, 2);
        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_subscriber_isolation() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // 第一个订阅者会 panic
        bus.subscribe(
            "module_a",
            "test.event",
            None,
            Arc::new(|_| {
                panic!("Intentional panic for test");
            }),
        )
        .await
        .unwrap();

        // 第二个订阅者正常处理
        bus.subscribe(
            "module_b",
            "test.event",
            None,
            Arc::new(move |_| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .await
        .unwrap();

        // 发布事件
        let event = Event::new("test.event", "sender", json!({}));
        let (successful, failed, _) = bus.publish_sync(event).await.unwrap();

        // 一个成功，一个失败（panic 被捕获）
        assert_eq!(successful, 1);
        assert_eq!(failed, 1);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_handler_timeout() {
        let config = EventBusConfig {
            handler_timeout: Duration::from_millis(50),
            ..Default::default()
        };
        let bus = EventBus::with_config(config);

        // 订阅一个会超时的处理器
        bus.subscribe(
            "test_module",
            "test.event",
            None,
            Arc::new(|_| {
                std::thread::sleep(Duration::from_millis(200));
            }),
        )
        .await
        .unwrap();

        // 发布事件
        let event = Event::new("test.event", "sender", json!({}));
        let (_, _, timeouts) = bus.publish_sync(event).await.unwrap();

        assert_eq!(timeouts, 1);
    }

    #[tokio::test]
    async fn test_stats() {
        let bus = EventBus::new();

        bus.subscribe("test_module", "test.event", None, Arc::new(|_| {}))
            .await
            .unwrap();

        // 发布多个事件
        for _ in 0..5 {
            let event = Event::new("test.event", "sender", json!({}));
            bus.publish_sync(event).await.unwrap();
        }

        let stats = bus.stats().await;
        assert_eq!(stats.total_dispatched, 5);
        assert_eq!(stats.successful, 5);
        assert!(stats.last_dispatch_at.is_some());
    }

    #[tokio::test]
    async fn test_reset_stats() {
        let bus = EventBus::new();

        bus.subscribe("test_module", "test.event", None, Arc::new(|_| {}))
            .await
            .unwrap();

        let event = Event::new("test.event", "sender", json!({}));
        bus.publish_sync(event).await.unwrap();

        bus.reset_stats().await;

        let stats = bus.stats().await;
        assert_eq!(stats.total_dispatched, 0);
    }

    #[tokio::test]
    async fn test_has_subscribers() {
        let bus = EventBus::new();

        assert!(!bus.has_subscribers("test.event").await);

        bus.subscribe("test_module", "test.event", None, Arc::new(|_| {}))
            .await
            .unwrap();

        assert!(bus.has_subscribers("test.event").await);
    }

    #[tokio::test]
    async fn test_has_subscribers_wildcard() {
        let bus = EventBus::new();

        bus.subscribe("test_module", "module.*", None, Arc::new(|_| {}))
            .await
            .unwrap();

        assert!(bus.has_subscribers("module.loaded").await);
        assert!(bus.has_subscribers("module.started").await);
        assert!(!bus.has_subscribers("config.changed").await);
    }

    #[tokio::test]
    async fn test_get_subscriptions_for_module() {
        let bus = EventBus::new();

        let sub1 = bus
            .subscribe("module_a", "event1", None, Arc::new(|_| {}))
            .await
            .unwrap();
        let sub2 = bus
            .subscribe("module_a", "event2", None, Arc::new(|_| {}))
            .await
            .unwrap();
        bus.subscribe("module_b", "event1", None, Arc::new(|_| {}))
            .await
            .unwrap();

        let subs = bus.get_subscriptions_for_module("module_a").await;
        assert_eq!(subs.len(), 2);
        assert!(subs.contains(&sub1));
        assert!(subs.contains(&sub2));
    }

    #[tokio::test]
    async fn test_set_subscription_enabled() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let sub_id = bus
            .subscribe(
                "test_module",
                "test.event",
                None,
                Arc::new(move |_| {
                    counter_clone.fetch_add(1, Ordering::SeqCst);
                }),
            )
            .await
            .unwrap();

        // 禁用订阅
        bus.set_subscription_enabled(&sub_id, false).await.unwrap();

        // 发布事件（不应触发回调）
        let event = Event::new("test.event", "sender", json!({}));
        bus.publish_sync(event).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // 启用订阅
        bus.set_subscription_enabled(&sub_id, true).await.unwrap();

        // 再次发布事件（应触发回调）
        let event = Event::new("test.event", "sender", json!({}));
        bus.publish_sync(event).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_subscription_count_for_type() {
        let bus = EventBus::new();

        bus.subscribe("module_a", "event1", None, Arc::new(|_| {}))
            .await
            .unwrap();
        bus.subscribe("module_b", "event1", None, Arc::new(|_| {}))
            .await
            .unwrap();
        bus.subscribe("module_a", "event2", None, Arc::new(|_| {}))
            .await
            .unwrap();

        assert_eq!(bus.subscription_count_for_type("event1").await, 2);
        assert_eq!(bus.subscription_count_for_type("event2").await, 1);
        assert_eq!(bus.subscription_count_for_type("event3").await, 0);
    }

    #[tokio::test]
    async fn test_no_matching_subscribers() {
        let bus = EventBus::new();

        bus.subscribe("test_module", "other.event", None, Arc::new(|_| {}))
            .await
            .unwrap();

        let event = Event::new("test.event", "sender", json!({}));
        let matched = bus.publish(event).await.unwrap();

        assert_eq!(matched, 0);
    }

    #[tokio::test]
    async fn test_publish_without_subscribers() {
        let bus = EventBus::new();

        let event = Event::new("test.event", "sender", json!({}));
        let matched = bus.publish(event).await.unwrap();

        assert_eq!(matched, 0);
    }

    #[tokio::test]
    async fn test_concurrent_publish() {
        let bus = Arc::new(EventBus::new());
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        bus.subscribe(
            "test_module",
            "test.event",
            None,
            Arc::new(move |_| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .await
        .unwrap();

        // 并发发布多个事件
        let mut handles = vec![];
        for _ in 0..10 {
            let bus_clone = bus.clone();
            handles.push(tokio::spawn(async move {
                let event = Event::new("test.event", "sender", json!({}));
                bus_clone.publish_sync(event).await.unwrap();
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn test_subscription_entry_matches_pattern() {
        // 精确匹配
        assert!(SubscriptionEntry::matches_pattern("test.event", "test.event"));
        assert!(!SubscriptionEntry::matches_pattern("test.event", "other.event"));

        // 通配符 *
        assert!(SubscriptionEntry::matches_pattern("*", "any.event"));
        assert!(SubscriptionEntry::matches_pattern("*", "module.loaded"));

        // 后缀通配符 .*
        assert!(SubscriptionEntry::matches_pattern("module.*", "module.loaded"));
        assert!(SubscriptionEntry::matches_pattern("module.*", "module.started"));
        assert!(!SubscriptionEntry::matches_pattern("module.*", "config.changed"));
        assert!(!SubscriptionEntry::matches_pattern("module.*", "module")); // 需要有后缀
    }
}
