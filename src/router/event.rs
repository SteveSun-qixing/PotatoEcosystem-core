//! 事件系统数据结构
//!
//! 定义事件发布订阅系统的核心数据结构。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::utils::generate_uuid;

/// 事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// 事件唯一标识
    pub event_id: String,

    /// 事件类型（格式: category.name，如 module.loaded）
    pub event_type: String,

    /// 发送方模块 ID
    pub sender: String,

    /// 事件数据
    #[serde(default)]
    pub data: Value,

    /// 事件时间戳
    pub timestamp: DateTime<Utc>,

    /// 是否传播到其他模块
    #[serde(default = "default_propagate")]
    pub propagate: bool,

    /// 扩展元数据
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, Value>,
}

fn default_propagate() -> bool {
    true
}

impl Event {
    /// 创建新事件
    pub fn new(event_type: impl Into<String>, sender: impl Into<String>, data: Value) -> Self {
        Self {
            event_id: generate_uuid(),
            event_type: event_type.into(),
            sender: sender.into(),
            data,
            timestamp: Utc::now(),
            propagate: true,
            metadata: HashMap::new(),
        }
    }

    /// 使用 Builder 模式构建事件
    pub fn builder(event_type: impl Into<String>, sender: impl Into<String>) -> EventBuilder {
        EventBuilder::new(event_type, sender)
    }

    /// 设置是否传播
    pub fn with_propagate(mut self, propagate: bool) -> Self {
        self.propagate = propagate;
        self
    }

    /// 添加元数据
    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

impl Default for Event {
    fn default() -> Self {
        Self {
            event_id: generate_uuid(),
            event_type: String::new(),
            sender: String::new(),
            data: Value::Null,
            timestamp: Utc::now(),
            propagate: true,
            metadata: HashMap::new(),
        }
    }
}

/// 事件构建器
#[derive(Debug)]
pub struct EventBuilder {
    event_type: String,
    sender: String,
    data: Value,
    propagate: bool,
    metadata: HashMap<String, Value>,
}

impl EventBuilder {
    pub fn new(event_type: impl Into<String>, sender: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
            sender: sender.into(),
            data: Value::Null,
            propagate: true,
            metadata: HashMap::new(),
        }
    }

    pub fn data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }

    pub fn propagate(mut self, propagate: bool) -> Self {
        self.propagate = propagate;
        self
    }

    pub fn metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    pub fn build(self) -> Event {
        Event {
            event_id: generate_uuid(),
            event_type: self.event_type,
            sender: self.sender,
            data: self.data,
            timestamp: Utc::now(),
            propagate: self.propagate,
            metadata: self.metadata,
        }
    }
}

/// 事件过滤器
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFilter {
    /// 事件类型过滤（支持通配符 *）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,

    /// 发送方过滤
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,

    /// 数据字段过滤（JSON Path 表达式）
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub data_filters: HashMap<String, Value>,
}

impl EventFilter {
    /// 创建空过滤器（匹配所有）
    pub fn new() -> Self {
        Self {
            event_type: None,
            sender: None,
            data_filters: HashMap::new(),
        }
    }

    /// 按事件类型过滤
    pub fn by_type(event_type: impl Into<String>) -> Self {
        Self {
            event_type: Some(event_type.into()),
            sender: None,
            data_filters: HashMap::new(),
        }
    }

    /// 按发送方过滤
    pub fn by_sender(sender: impl Into<String>) -> Self {
        Self {
            event_type: None,
            sender: Some(sender.into()),
            data_filters: HashMap::new(),
        }
    }

    /// 添加事件类型过滤
    pub fn with_type(mut self, event_type: impl Into<String>) -> Self {
        self.event_type = Some(event_type.into());
        self
    }

    /// 添加发送方过滤
    pub fn with_sender(mut self, sender: impl Into<String>) -> Self {
        self.sender = Some(sender.into());
        self
    }

    /// 检查事件是否匹配过滤器
    pub fn matches(&self, event: &Event) -> bool {
        // 检查事件类型
        if let Some(ref filter_type) = self.event_type {
            if !Self::matches_pattern(filter_type, &event.event_type) {
                return false;
            }
        }

        // 检查发送方
        if let Some(ref filter_sender) = self.sender {
            if filter_sender != &event.sender {
                return false;
            }
        }

        // 检查数据字段（简化实现）
        for (key, expected_value) in &self.data_filters {
            if let Some(actual_value) = event.data.get(key) {
                if actual_value != expected_value {
                    return false;
                }
            } else {
                return false;
            }
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
            return value.starts_with(prefix);
        }

        pattern == value
    }
}

impl Default for EventFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// 订阅信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    /// 订阅 ID
    pub subscription_id: String,

    /// 订阅者模块 ID
    pub subscriber_id: String,

    /// 订阅的事件类型
    pub event_type: String,

    /// 事件过滤器
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<EventFilter>,

    /// 订阅时间
    pub subscribed_at: DateTime<Utc>,

    /// 是否启用
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Subscription {
    /// 创建新订阅
    pub fn new(subscriber_id: impl Into<String>, event_type: impl Into<String>) -> Self {
        Self {
            subscription_id: generate_uuid(),
            subscriber_id: subscriber_id.into(),
            event_type: event_type.into(),
            filter: None,
            subscribed_at: Utc::now(),
            enabled: true,
        }
    }

    /// 添加过滤器
    pub fn with_filter(mut self, filter: EventFilter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// 检查事件是否匹配此订阅
    pub fn matches(&self, event: &Event) -> bool {
        if !self.enabled {
            return false;
        }

        // 检查事件类型
        if !EventFilter::matches_pattern(&self.event_type, &event.event_type) {
            return false;
        }

        // 应用过滤器
        if let Some(ref filter) = self.filter {
            return filter.matches(event);
        }

        true
    }
}

/// 预定义的系统事件类型
pub mod system_events {
    /// 模块加载完成
    pub const MODULE_LOADED: &str = "system.module.loaded";
    /// 模块卸载完成
    pub const MODULE_UNLOADED: &str = "system.module.unloaded";
    /// 模块启动
    pub const MODULE_STARTED: &str = "system.module.started";
    /// 模块停止
    pub const MODULE_STOPPED: &str = "system.module.stopped";
    /// 配置变更
    pub const CONFIG_CHANGED: &str = "system.config.changed";
    /// 内核启动
    pub const CORE_STARTED: &str = "system.core.started";
    /// 内核关闭
    pub const CORE_SHUTDOWN: &str = "system.core.shutdown";
    /// 健康检查
    pub const HEALTH_CHECK: &str = "system.health.check";
    /// 错误发生
    pub const ERROR_OCCURRED: &str = "system.error.occurred";
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_event_creation() {
        let event = Event::new("test.event", "test_module", json!({"key": "value"}));

        assert!(!event.event_id.is_empty());
        assert_eq!(event.event_type, "test.event");
        assert_eq!(event.sender, "test_module");
        assert!(event.propagate);
    }

    #[test]
    fn test_event_builder() {
        let event = Event::builder("test.event", "test_module")
            .data(json!({"key": "value"}))
            .propagate(false)
            .metadata("trace_id", json!("trace-123"))
            .build();

        assert!(!event.propagate);
        assert!(event.metadata.contains_key("trace_id"));
    }

    #[test]
    fn test_event_filter_type() {
        let filter = EventFilter::by_type("module.*");
        
        let event1 = Event::new("module.loaded", "core", json!({}));
        let event2 = Event::new("config.changed", "core", json!({}));

        assert!(filter.matches(&event1));
        assert!(!filter.matches(&event2));
    }

    #[test]
    fn test_event_filter_sender() {
        let filter = EventFilter::by_sender("filesystem");

        let event1 = Event::new("file.changed", "filesystem", json!({}));
        let event2 = Event::new("file.changed", "editor", json!({}));

        assert!(filter.matches(&event1));
        assert!(!filter.matches(&event2));
    }

    #[test]
    fn test_event_filter_wildcard() {
        let filter = EventFilter::by_type("*");

        let event = Event::new("any.event", "any_module", json!({}));
        assert!(filter.matches(&event));
    }

    #[test]
    fn test_subscription_matches() {
        let sub = Subscription::new("handler", "module.*");

        let event1 = Event::new("module.loaded", "core", json!({}));
        let event2 = Event::new("config.changed", "core", json!({}));

        assert!(sub.matches(&event1));
        assert!(!sub.matches(&event2));
    }

    #[test]
    fn test_subscription_disabled() {
        let mut sub = Subscription::new("handler", "module.*");
        sub.enabled = false;

        let event = Event::new("module.loaded", "core", json!({}));
        assert!(!sub.matches(&event));
    }

    #[test]
    fn test_event_serialization() {
        let event = Event::new("test.event", "test_module", json!({"key": "value"}));
        let json = serde_json::to_string(&event).unwrap();
        let parsed: Event = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.event_type, event.event_type);
        assert_eq!(parsed.sender, event.sender);
    }
}
