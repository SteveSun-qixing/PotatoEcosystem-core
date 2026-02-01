//! 路由请求和响应数据结构
//!
//! 定义薯片协议规范的请求和响应消息格式。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::utils::{generate_uuid, status_code};

/// 请求优先级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// 低优先级
    Low = 0,
    /// 普通优先级（默认）
    Normal = 1,
    /// 高优先级
    High = 2,
    /// 紧急优先级
    Urgent = 3,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

impl From<&str> for Priority {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "low" => Priority::Low,
            "high" => Priority::High,
            "urgent" => Priority::Urgent,
            _ => Priority::Normal,
        }
    }
}

/// 路由请求
///
/// 遵循薯片协议规范的请求消息格式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRequest {
    /// 请求唯一标识（UUID v4）
    pub request_id: String,

    /// 发送方模块 ID
    pub sender: String,

    /// 目标操作（格式: module.action 或 service.action）
    pub action: String,

    /// 目标模块 ID（可选，用于直接指定目标）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,

    /// 请求参数
    #[serde(default)]
    pub params: Value,

    /// 请求优先级
    #[serde(default)]
    pub priority: Priority,

    /// 超时时间（毫秒），默认 30000ms
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,

    /// 请求时间戳
    pub timestamp: DateTime<Utc>,

    /// 扩展元数据
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, Value>,
}

fn default_timeout() -> u64 {
    30000
}

impl RouteRequest {
    /// 创建新的路由请求
    ///
    /// # Arguments
    ///
    /// * `sender` - 发送方模块 ID
    /// * `action` - 目标操作
    /// * `params` - 请求参数
    pub fn new(sender: impl Into<String>, action: impl Into<String>, params: Value) -> Self {
        Self {
            request_id: generate_uuid(),
            sender: sender.into(),
            action: action.into(),
            target: None,
            params,
            priority: Priority::Normal,
            timeout_ms: 30000,
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    /// 使用 Builder 模式构建请求
    pub fn builder(sender: impl Into<String>, action: impl Into<String>) -> RouteRequestBuilder {
        RouteRequestBuilder::new(sender, action)
    }

    /// 设置目标模块
    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// 设置优先级
    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// 设置超时时间
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// 添加元数据
    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

/// 路由请求构建器
#[derive(Debug, Clone)]
pub struct RouteRequestBuilder {
    sender: String,
    action: String,
    target: Option<String>,
    params: Value,
    priority: Priority,
    timeout_ms: u64,
    metadata: HashMap<String, Value>,
}

impl RouteRequestBuilder {
    pub fn new(sender: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            sender: sender.into(),
            action: action.into(),
            target: None,
            params: Value::Null,
            priority: Priority::Normal,
            timeout_ms: 30000,
            metadata: HashMap::new(),
        }
    }

    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    pub fn params(mut self, params: Value) -> Self {
        self.params = params;
        self
    }

    pub fn priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    pub fn timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    pub fn metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    pub fn build(self) -> RouteRequest {
        RouteRequest {
            request_id: generate_uuid(),
            sender: self.sender,
            action: self.action,
            target: self.target,
            params: self.params,
            priority: self.priority,
            timeout_ms: self.timeout_ms,
            timestamp: Utc::now(),
            metadata: self.metadata,
        }
    }
}

/// 错误信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    /// 错误码
    pub code: String,
    /// 错误消息
    pub message: String,
    /// 详细信息（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl ErrorInfo {
    /// 创建新的错误信息
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    /// 添加详细信息
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}

/// 路由响应
///
/// 遵循薯片协议规范的响应消息格式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteResponse {
    /// 对应的请求 ID
    pub request_id: String,

    /// 状态码（HTTP 风格）
    pub status: u16,

    /// 响应数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,

    /// 错误信息（当 status >= 400 时）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,

    /// 处理耗时（毫秒）
    pub elapsed_ms: u64,

    /// 响应时间戳
    pub timestamp: DateTime<Utc>,
}

impl RouteResponse {
    /// 创建成功响应
    pub fn success(request_id: &str, data: Value, elapsed_ms: u64) -> Self {
        Self {
            request_id: request_id.to_string(),
            status: status_code::OK,
            data: Some(data),
            error: None,
            elapsed_ms,
            timestamp: Utc::now(),
        }
    }

    /// 创建成功响应（无数据）
    pub fn success_empty(request_id: &str, elapsed_ms: u64) -> Self {
        Self {
            request_id: request_id.to_string(),
            status: status_code::OK,
            data: None,
            error: None,
            elapsed_ms,
            timestamp: Utc::now(),
        }
    }

    /// 创建接受响应（异步处理中）
    pub fn accepted(request_id: &str, elapsed_ms: u64) -> Self {
        Self {
            request_id: request_id.to_string(),
            status: status_code::ACCEPTED,
            data: None,
            error: None,
            elapsed_ms,
            timestamp: Utc::now(),
        }
    }

    /// 创建错误响应
    pub fn error(request_id: &str, status: u16, error: ErrorInfo, elapsed_ms: u64) -> Self {
        Self {
            request_id: request_id.to_string(),
            status,
            data: None,
            error: Some(error),
            elapsed_ms,
            timestamp: Utc::now(),
        }
    }

    /// 是否成功
    pub fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    /// 是否错误
    pub fn is_error(&self) -> bool {
        self.status >= 400
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_route_request_new() {
        let request = RouteRequest::new("editor", "file.open", json!({"path": "/test.txt"}));

        assert!(!request.request_id.is_empty());
        assert_eq!(request.sender, "editor");
        assert_eq!(request.action, "file.open");
        assert_eq!(request.priority, Priority::Normal);
        assert_eq!(request.timeout_ms, 30000);
    }

    #[test]
    fn test_route_request_builder() {
        let request = RouteRequest::builder("editor", "file.open")
            .target("filesystem")
            .params(json!({"path": "/test.txt"}))
            .priority(Priority::High)
            .timeout_ms(5000)
            .metadata("trace_id", json!("trace-123"))
            .build();

        assert_eq!(request.target, Some("filesystem".to_string()));
        assert_eq!(request.priority, Priority::High);
        assert_eq!(request.timeout_ms, 5000);
        assert!(request.metadata.contains_key("trace_id"));
    }

    #[test]
    fn test_route_response_success() {
        let response = RouteResponse::success("req-123", json!({"result": "ok"}), 10);

        assert_eq!(response.status, status_code::OK);
        assert!(response.is_success());
        assert!(!response.is_error());
        assert!(response.data.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_route_response_error() {
        let error = ErrorInfo::new("NOT_FOUND", "Resource not found");
        let response = RouteResponse::error("req-123", status_code::NOT_FOUND, error, 5);

        assert_eq!(response.status, status_code::NOT_FOUND);
        assert!(!response.is_success());
        assert!(response.is_error());
        assert!(response.data.is_none());
        assert!(response.error.is_some());
    }

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Low < Priority::Normal);
        assert!(Priority::Normal < Priority::High);
        assert!(Priority::High < Priority::Urgent);
    }

    #[test]
    fn test_request_serialization() {
        let request = RouteRequest::new("editor", "file.open", json!({"path": "/test.txt"}));
        let json = serde_json::to_string(&request).unwrap();
        let parsed: RouteRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.sender, request.sender);
        assert_eq!(parsed.action, request.action);
    }

    #[test]
    fn test_response_serialization() {
        let response = RouteResponse::success("req-123", json!({"result": "ok"}), 10);
        let json = serde_json::to_string(&response).unwrap();
        let parsed: RouteResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.status, response.status);
        assert_eq!(parsed.elapsed_ms, response.elapsed_ms);
    }
}
