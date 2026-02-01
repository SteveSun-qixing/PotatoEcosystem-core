//! 优先级请求队列
//!
//! 实现基于优先级的请求队列，支持阻塞和非阻塞操作。

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{oneshot, Mutex, Notify};

use super::request::{Priority, RouteRequest, RouteResponse};
use crate::utils::{CoreError, Result};

/// 队列项
///
/// 包含请求及其相关元数据，用于在队列中排序和处理。
pub struct QueueItem {
    /// 路由请求
    pub request: RouteRequest,

    /// 目标模块 ID
    pub target_module: String,

    /// 响应发送通道
    pub response_tx: oneshot::Sender<RouteResponse>,

    /// 入队时间
    pub enqueued_at: Instant,
}

impl QueueItem {
    /// 创建新的队列项
    pub fn new(
        request: RouteRequest,
        target_module: impl Into<String>,
        response_tx: oneshot::Sender<RouteResponse>,
    ) -> Self {
        Self {
            request,
            target_module: target_module.into(),
            response_tx,
            enqueued_at: Instant::now(),
        }
    }

    /// 获取请求优先级
    pub fn priority(&self) -> Priority {
        self.request.priority
    }

    /// 获取等待时间（毫秒）
    pub fn wait_time_ms(&self) -> u64 {
        self.enqueued_at.elapsed().as_millis() as u64
    }
}

impl std::fmt::Debug for QueueItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueueItem")
            .field("request_id", &self.request.request_id)
            .field("target_module", &self.target_module)
            .field("priority", &self.request.priority)
            .field("enqueued_at", &self.enqueued_at)
            .finish()
    }
}

/// 优先级队列项包装器
///
/// 用于实现 Ord trait，使 BinaryHeap 按优先级排序。
/// 优先级相同时，先入队的项优先（FIFO）。
struct PriorityQueueItem {
    item: QueueItem,
    /// 序列号，用于在优先级相同时保持 FIFO 顺序
    sequence: u64,
}

impl PriorityQueueItem {
    fn new(item: QueueItem, sequence: u64) -> Self {
        Self { item, sequence }
    }
}

impl PartialEq for PriorityQueueItem {
    fn eq(&self, other: &Self) -> bool {
        self.item.priority() == other.item.priority() && self.sequence == other.sequence
    }
}

impl Eq for PriorityQueueItem {}

impl PartialOrd for PriorityQueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityQueueItem {
    fn cmp(&self, other: &Self) -> Ordering {
        // 首先按优先级排序（高优先级先出）
        match self.item.priority().cmp(&other.item.priority()) {
            Ordering::Equal => {
                // 优先级相同时，序列号小的先出（FIFO）
                // 注意：这里反转顺序是因为 BinaryHeap 是最大堆
                other.sequence.cmp(&self.sequence)
            }
            other_ord => other_ord,
        }
    }
}

/// 优先级请求队列
///
/// 使用 BinaryHeap 实现的优先级队列，支持并发访问和阻塞等待。
///
/// # 特性
///
/// - 基于优先级排序（高优先级先出）
/// - 相同优先级时保持 FIFO 顺序
/// - 支持阻塞和非阻塞出队操作
/// - 队列大小限制
///
/// # 示例
///
/// ```ignore
/// use chips_core::router::queue::RequestQueue;
///
/// let queue = RequestQueue::new(100);
/// // 入队和出队操作...
/// ```
pub struct RequestQueue {
    /// 优先级队列
    queue: Arc<Mutex<BinaryHeap<PriorityQueueItem>>>,

    /// 最大队列大小
    max_size: usize,

    /// 当前队列大小（使用原子操作快速访问）
    current_size: Arc<AtomicUsize>,

    /// 通知器（用于唤醒等待的消费者）
    notify: Arc<Notify>,

    /// 序列号计数器（用于 FIFO 排序）
    sequence_counter: Arc<AtomicUsize>,
}

impl RequestQueue {
    /// 创建新的请求队列
    ///
    /// # Arguments
    ///
    /// * `max_size` - 队列最大容量
    ///
    /// # Returns
    ///
    /// 新创建的 RequestQueue 实例
    pub fn new(max_size: usize) -> Self {
        Self {
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
            max_size,
            current_size: Arc::new(AtomicUsize::new(0)),
            notify: Arc::new(Notify::new()),
            sequence_counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// 入队操作
    ///
    /// 将请求项添加到队列中。如果队列已满，返回错误。
    ///
    /// # Arguments
    ///
    /// * `item` - 要入队的队列项
    ///
    /// # Returns
    ///
    /// - `Ok(())` - 入队成功
    /// - `Err(CoreError::QueueFull)` - 队列已满
    pub async fn enqueue(&self, item: QueueItem) -> Result<()> {
        // 检查队列是否已满
        let current = self.current_size.load(AtomicOrdering::SeqCst);
        if current >= self.max_size {
            return Err(CoreError::QueueFull {
                max_size: self.max_size,
                message: format!(
                    "Request queue is full (max_size: {})",
                    self.max_size
                ),
            });
        }

        // 获取序列号
        let sequence = self.sequence_counter.fetch_add(1, AtomicOrdering::SeqCst) as u64;

        // 添加到队列
        {
            let mut queue = self.queue.lock().await;
            queue.push(PriorityQueueItem::new(item, sequence));
        }

        // 更新大小
        self.current_size.fetch_add(1, AtomicOrdering::SeqCst);

        // 通知等待的消费者
        self.notify.notify_one();

        Ok(())
    }

    /// 阻塞出队操作
    ///
    /// 从队列中取出最高优先级的项。如果队列为空，则阻塞等待。
    ///
    /// # Returns
    ///
    /// 队列中优先级最高的项
    pub async fn dequeue(&self) -> QueueItem {
        loop {
            // 尝试获取项
            if let Some(item) = self.try_dequeue().await {
                return item;
            }

            // 等待通知
            self.notify.notified().await;
        }
    }

    /// 非阻塞出队操作
    ///
    /// 尝试从队列中取出最高优先级的项。如果队列为空，返回 None。
    ///
    /// # Returns
    ///
    /// - `Some(QueueItem)` - 成功取出项
    /// - `None` - 队列为空
    pub async fn try_dequeue(&self) -> Option<QueueItem> {
        let mut queue = self.queue.lock().await;

        if let Some(priority_item) = queue.pop() {
            // 更新大小
            self.current_size.fetch_sub(1, AtomicOrdering::SeqCst);
            Some(priority_item.item)
        } else {
            None
        }
    }

    /// 获取当前队列大小
    ///
    /// # Returns
    ///
    /// 当前队列中的项数
    pub fn size(&self) -> usize {
        self.current_size.load(AtomicOrdering::SeqCst)
    }

    /// 检查队列是否已满
    ///
    /// # Returns
    ///
    /// - `true` - 队列已满
    /// - `false` - 队列未满
    pub fn is_full(&self) -> bool {
        self.size() >= self.max_size
    }

    /// 检查队列是否为空
    ///
    /// # Returns
    ///
    /// - `true` - 队列为空
    /// - `false` - 队列不为空
    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// 获取队列最大容量
    ///
    /// # Returns
    ///
    /// 队列的最大容量
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// 清空队列
    ///
    /// 移除队列中的所有项。注意：这会丢弃所有待处理的请求。
    pub async fn clear(&self) {
        let mut queue = self.queue.lock().await;
        queue.clear();
        self.current_size.store(0, AtomicOrdering::SeqCst);
    }

    /// 获取队列统计信息
    ///
    /// # Returns
    ///
    /// 队列的统计信息
    pub fn stats(&self) -> QueueStats {
        QueueStats {
            current_size: self.size(),
            max_size: self.max_size,
            is_full: self.is_full(),
            is_empty: self.is_empty(),
        }
    }
}

impl Clone for RequestQueue {
    fn clone(&self) -> Self {
        Self {
            queue: Arc::clone(&self.queue),
            max_size: self.max_size,
            current_size: Arc::clone(&self.current_size),
            notify: Arc::clone(&self.notify),
            sequence_counter: Arc::clone(&self.sequence_counter),
        }
    }
}

impl std::fmt::Debug for RequestQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestQueue")
            .field("max_size", &self.max_size)
            .field("current_size", &self.size())
            .finish()
    }
}

/// 队列统计信息
#[derive(Debug, Clone, serde::Serialize)]
pub struct QueueStats {
    /// 当前队列大小
    pub current_size: usize,
    /// 最大队列大小
    pub max_size: usize,
    /// 是否已满
    pub is_full: bool,
    /// 是否为空
    pub is_empty: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::time::{timeout, Duration};

    fn create_test_request(priority: Priority) -> RouteRequest {
        RouteRequest::new("test_sender", "test.action", json!({})).with_priority(priority)
    }

    fn create_test_item(priority: Priority) -> (QueueItem, oneshot::Receiver<RouteResponse>) {
        let request = create_test_request(priority);
        let (tx, rx) = oneshot::channel();
        let item = QueueItem::new(request, "target_module", tx);
        (item, rx)
    }

    #[tokio::test]
    async fn test_queue_new() {
        let queue = RequestQueue::new(100);
        assert_eq!(queue.max_size(), 100);
        assert_eq!(queue.size(), 0);
        assert!(queue.is_empty());
        assert!(!queue.is_full());
    }

    #[tokio::test]
    async fn test_enqueue_dequeue() {
        let queue = RequestQueue::new(10);

        let (item, _rx) = create_test_item(Priority::Normal);
        let request_id = item.request.request_id.clone();

        queue.enqueue(item).await.unwrap();
        assert_eq!(queue.size(), 1);

        let dequeued = queue.try_dequeue().await.unwrap();
        assert_eq!(dequeued.request.request_id, request_id);
        assert_eq!(queue.size(), 0);
    }

    #[tokio::test]
    async fn test_priority_ordering() {
        let queue = RequestQueue::new(10);

        // 按顺序入队：Normal, High, Low, Urgent
        let (item_normal, _) = create_test_item(Priority::Normal);
        let (item_high, _) = create_test_item(Priority::High);
        let (item_low, _) = create_test_item(Priority::Low);
        let (item_urgent, _) = create_test_item(Priority::Urgent);

        queue.enqueue(item_normal).await.unwrap();
        queue.enqueue(item_high).await.unwrap();
        queue.enqueue(item_low).await.unwrap();
        queue.enqueue(item_urgent).await.unwrap();

        assert_eq!(queue.size(), 4);

        // 出队顺序应该是：Urgent > High > Normal > Low
        let d1 = queue.try_dequeue().await.unwrap();
        assert_eq!(d1.priority(), Priority::Urgent);

        let d2 = queue.try_dequeue().await.unwrap();
        assert_eq!(d2.priority(), Priority::High);

        let d3 = queue.try_dequeue().await.unwrap();
        assert_eq!(d3.priority(), Priority::Normal);

        let d4 = queue.try_dequeue().await.unwrap();
        assert_eq!(d4.priority(), Priority::Low);

        assert!(queue.is_empty());
    }

    #[tokio::test]
    async fn test_fifo_same_priority() {
        let queue = RequestQueue::new(10);

        // 创建多个相同优先级的请求
        let (item1, _) = create_test_item(Priority::Normal);
        let id1 = item1.request.request_id.clone();

        let (item2, _) = create_test_item(Priority::Normal);
        let id2 = item2.request.request_id.clone();

        let (item3, _) = create_test_item(Priority::Normal);
        let id3 = item3.request.request_id.clone();

        queue.enqueue(item1).await.unwrap();
        queue.enqueue(item2).await.unwrap();
        queue.enqueue(item3).await.unwrap();

        // 出队顺序应该与入队顺序相同（FIFO）
        let d1 = queue.try_dequeue().await.unwrap();
        assert_eq!(d1.request.request_id, id1);

        let d2 = queue.try_dequeue().await.unwrap();
        assert_eq!(d2.request.request_id, id2);

        let d3 = queue.try_dequeue().await.unwrap();
        assert_eq!(d3.request.request_id, id3);
    }

    #[tokio::test]
    async fn test_queue_full() {
        let queue = RequestQueue::new(2);

        let (item1, _) = create_test_item(Priority::Normal);
        let (item2, _) = create_test_item(Priority::Normal);
        let (item3, _) = create_test_item(Priority::Normal);

        queue.enqueue(item1).await.unwrap();
        queue.enqueue(item2).await.unwrap();

        assert!(queue.is_full());

        // 第三个应该失败
        let result = queue.enqueue(item3).await;
        assert!(result.is_err());

        match result {
            Err(CoreError::QueueFull { max_size, .. }) => {
                assert_eq!(max_size, 2);
            }
            _ => panic!("Expected QueueFull error"),
        }
    }

    #[tokio::test]
    async fn test_try_dequeue_empty() {
        let queue = RequestQueue::new(10);
        let result = queue.try_dequeue().await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_blocking_dequeue() {
        let queue = RequestQueue::new(10);
        let queue_clone = queue.clone();

        // 在后台任务中延迟入队
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let (item, _) = create_test_item(Priority::High);
            queue_clone.enqueue(item).await.unwrap();
        });

        // 阻塞等待出队
        let result = timeout(Duration::from_secs(1), queue.dequeue()).await;
        assert!(result.is_ok());

        let item = result.unwrap();
        assert_eq!(item.priority(), Priority::High);
    }

    #[tokio::test]
    async fn test_clear() {
        let queue = RequestQueue::new(10);

        let (item1, _) = create_test_item(Priority::Normal);
        let (item2, _) = create_test_item(Priority::High);

        queue.enqueue(item1).await.unwrap();
        queue.enqueue(item2).await.unwrap();
        assert_eq!(queue.size(), 2);

        queue.clear().await;
        assert_eq!(queue.size(), 0);
        assert!(queue.is_empty());
    }

    #[tokio::test]
    async fn test_queue_stats() {
        let queue = RequestQueue::new(5);

        let (item1, _) = create_test_item(Priority::Normal);
        let (item2, _) = create_test_item(Priority::High);

        queue.enqueue(item1).await.unwrap();
        queue.enqueue(item2).await.unwrap();

        let stats = queue.stats();
        assert_eq!(stats.current_size, 2);
        assert_eq!(stats.max_size, 5);
        assert!(!stats.is_full);
        assert!(!stats.is_empty);
    }

    #[tokio::test]
    async fn test_queue_item_wait_time() {
        let (item, _) = create_test_item(Priority::Normal);

        // 等待一小段时间
        tokio::time::sleep(Duration::from_millis(10)).await;

        // 等待时间应该大于 0
        assert!(item.wait_time_ms() >= 10);
    }

    #[tokio::test]
    async fn test_concurrent_enqueue() {
        let queue = Arc::new(RequestQueue::new(100));
        let mut handles = vec![];

        // 并发入队 50 个请求
        for _ in 0..50 {
            let q = Arc::clone(&queue);
            handles.push(tokio::spawn(async move {
                let (item, _) = create_test_item(Priority::Normal);
                q.enqueue(item).await.unwrap();
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(queue.size(), 50);
    }

    #[tokio::test]
    async fn test_concurrent_dequeue() {
        let queue = Arc::new(RequestQueue::new(100));

        // 先入队 50 个请求
        for _ in 0..50 {
            let (item, _) = create_test_item(Priority::Normal);
            queue.enqueue(item).await.unwrap();
        }

        let mut handles = vec![];

        // 并发出队
        for _ in 0..50 {
            let q = Arc::clone(&queue);
            handles.push(tokio::spawn(async move {
                q.try_dequeue().await
            }));
        }

        let mut success_count = 0;
        for handle in handles {
            if handle.await.unwrap().is_some() {
                success_count += 1;
            }
        }

        assert_eq!(success_count, 50);
        assert!(queue.is_empty());
    }

    #[tokio::test]
    async fn test_mixed_priorities_concurrent() {
        let queue = Arc::new(RequestQueue::new(100));

        // 入队不同优先级的请求
        for priority in [Priority::Low, Priority::Normal, Priority::High, Priority::Urgent] {
            for _ in 0..5 {
                let (item, _) = create_test_item(priority);
                queue.enqueue(item).await.unwrap();
            }
        }

        assert_eq!(queue.size(), 20);

        // 验证出队顺序
        let mut last_priority = Priority::Urgent;
        let mut count_per_priority = std::collections::HashMap::new();

        while let Some(item) = queue.try_dequeue().await {
            let p = item.priority();
            *count_per_priority.entry(p).or_insert(0) += 1;

            // 优先级应该非递增（可能相等）
            assert!(p <= last_priority || p == last_priority);
            last_priority = p;
        }

        // 每个优先级应该有 5 个
        assert_eq!(count_per_priority.get(&Priority::Low), Some(&5));
        assert_eq!(count_per_priority.get(&Priority::Normal), Some(&5));
        assert_eq!(count_per_priority.get(&Priority::High), Some(&5));
        assert_eq!(count_per_priority.get(&Priority::Urgent), Some(&5));
    }

    #[tokio::test]
    async fn test_queue_clone() {
        let queue1 = RequestQueue::new(10);
        let queue2 = queue1.clone();

        let (item, _) = create_test_item(Priority::Normal);
        queue1.enqueue(item).await.unwrap();

        // 两个队列应该共享状态
        assert_eq!(queue1.size(), 1);
        assert_eq!(queue2.size(), 1);

        let _ = queue2.try_dequeue().await;
        assert_eq!(queue1.size(), 0);
        assert_eq!(queue2.size(), 0);
    }
}
