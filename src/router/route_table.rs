//! 路由表数据结构
//!
//! 管理 action 到模块的映射关系。

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::request::RouteRequest;
use crate::utils::Result;

/// 路由类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteType {
    /// 精确匹配
    Exact,
    /// 正则匹配
    Regex,
    /// 前缀匹配
    Prefix,
}

/// 路由条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteEntry {
    /// 目标模块 ID
    pub module_id: String,

    /// 路由类型
    pub route_type: RouteType,

    /// 匹配模式（action 名称或正则表达式）
    pub pattern: String,

    /// 优先级（数值越大优先级越高）
    #[serde(default)]
    pub priority: i32,

    /// 描述信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// 是否启用
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl RouteEntry {
    /// 创建精确匹配路由
    pub fn exact(module_id: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            module_id: module_id.into(),
            route_type: RouteType::Exact,
            pattern: action.into(),
            priority: 0,
            description: None,
            enabled: true,
        }
    }

    /// 创建正则匹配路由
    pub fn regex(module_id: impl Into<String>, pattern: impl Into<String>) -> Self {
        Self {
            module_id: module_id.into(),
            route_type: RouteType::Regex,
            pattern: pattern.into(),
            priority: 10, // 正则路由默认更高优先级
            description: None,
            enabled: true,
        }
    }

    /// 创建前缀匹配路由
    pub fn prefix(module_id: impl Into<String>, prefix: impl Into<String>) -> Self {
        Self {
            module_id: module_id.into(),
            route_type: RouteType::Prefix,
            pattern: prefix.into(),
            priority: 5,
            description: None,
            enabled: true,
        }
    }

    /// 设置优先级
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// 设置描述
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// 检查是否匹配给定的 action
    pub fn matches(&self, action: &str) -> bool {
        if !self.enabled {
            return false;
        }

        match self.route_type {
            RouteType::Exact => self.pattern == action,
            RouteType::Prefix => action.starts_with(&self.pattern),
            RouteType::Regex => {
                if let Ok(re) = Regex::new(&self.pattern) {
                    re.is_match(action)
                } else {
                    false
                }
            }
        }
    }
}

/// 编译后的正则路由（用于性能优化）
struct CompiledRegexRoute {
    entry: RouteEntry,
    regex: Regex,
}

/// 路由表
///
/// 管理 action 到模块的映射关系，支持精确匹配和正则匹配。
pub struct RouteTable {
    /// 精确匹配路由表 (action -> [RouteEntry])
    action_routes: Arc<RwLock<HashMap<String, Vec<RouteEntry>>>>,

    /// 自定义正则路由（优先级最高）
    custom_routes: Arc<RwLock<Vec<CompiledRegexRoute>>>,

    /// 前缀匹配路由
    prefix_routes: Arc<RwLock<Vec<RouteEntry>>>,

    /// 默认路由（当没有匹配时使用）
    default_route: Arc<RwLock<Option<RouteEntry>>>,
}

impl RouteTable {
    /// 创建空路由表
    pub fn new() -> Self {
        Self {
            action_routes: Arc::new(RwLock::new(HashMap::new())),
            custom_routes: Arc::new(RwLock::new(Vec::new())),
            prefix_routes: Arc::new(RwLock::new(Vec::new())),
            default_route: Arc::new(RwLock::new(None)),
        }
    }

    /// 注册精确匹配路由
    pub async fn register_action_route(&self, action: &str, entry: RouteEntry) -> Result<()> {
        let mut routes = self.action_routes.write().await;
        routes.entry(action.to_string()).or_default().push(entry);

        // 按优先级排序（高到低）
        if let Some(entries) = routes.get_mut(action) {
            entries.sort_by(|a, b| b.priority.cmp(&a.priority));
        }

        Ok(())
    }

    /// 注册正则匹配路由
    pub async fn register_custom_route(&self, entry: RouteEntry) -> Result<()> {
        let regex = Regex::new(&entry.pattern).map_err(|e| {
            crate::utils::CoreError::InvalidRequest(format!("Invalid regex pattern: {}", e))
        })?;

        let mut routes = self.custom_routes.write().await;
        routes.push(CompiledRegexRoute { entry, regex });

        // 按优先级排序（高到低）
        routes.sort_by(|a, b| b.entry.priority.cmp(&a.entry.priority));

        Ok(())
    }

    /// 注册前缀匹配路由
    pub async fn register_prefix_route(&self, entry: RouteEntry) -> Result<()> {
        let mut routes = self.prefix_routes.write().await;
        routes.push(entry);

        // 按优先级排序（高到低）
        routes.sort_by(|a, b| b.priority.cmp(&a.priority));

        Ok(())
    }

    /// 设置默认路由
    pub async fn set_default_route(&self, entry: RouteEntry) {
        let mut default = self.default_route.write().await;
        *default = Some(entry);
    }

    /// 查找匹配的路由
    ///
    /// 查找顺序：
    /// 1. 自定义正则路由（优先级最高）
    /// 2. 精确匹配路由
    /// 3. 前缀匹配路由
    /// 4. 默认路由
    pub async fn find_route(&self, request: &RouteRequest) -> Option<RouteEntry> {
        let action = &request.action;

        // 1. 查找自定义正则路由
        {
            let custom_routes = self.custom_routes.read().await;
            for compiled in custom_routes.iter() {
                if compiled.entry.enabled && compiled.regex.is_match(action) {
                    return Some(compiled.entry.clone());
                }
            }
        }

        // 2. 查找精确匹配路由
        {
            let action_routes = self.action_routes.read().await;
            if let Some(entries) = action_routes.get(action) {
                for entry in entries {
                    if entry.enabled {
                        return Some(entry.clone());
                    }
                }
            }
        }

        // 3. 查找前缀匹配路由
        {
            let prefix_routes = self.prefix_routes.read().await;
            for entry in prefix_routes.iter() {
                if entry.matches(action) {
                    return Some(entry.clone());
                }
            }
        }

        // 4. 返回默认路由
        let default = self.default_route.read().await;
        default.clone()
    }

    /// 移除指定模块的所有路由
    pub async fn remove_module_routes(&self, module_id: &str) {
        // 移除 action 路由
        {
            let mut routes = self.action_routes.write().await;
            for entries in routes.values_mut() {
                entries.retain(|e| e.module_id != module_id);
            }
            // 清理空的 action 条目
            routes.retain(|_, entries| !entries.is_empty());
        }

        // 移除自定义路由
        {
            let mut routes = self.custom_routes.write().await;
            routes.retain(|c| c.entry.module_id != module_id);
        }

        // 移除前缀路由
        {
            let mut routes = self.prefix_routes.write().await;
            routes.retain(|e| e.module_id != module_id);
        }

        // 如果默认路由属于该模块，也移除
        {
            let mut default = self.default_route.write().await;
            if let Some(ref entry) = *default {
                if entry.module_id == module_id {
                    *default = None;
                }
            }
        }
    }

    /// 获取所有已注册的 action 列表
    pub async fn list_actions(&self) -> Vec<String> {
        let routes = self.action_routes.read().await;
        routes.keys().cloned().collect()
    }

    /// 导出路由表（用于调试）
    pub async fn export(&self) -> RouteTableExport {
        let action_routes = self.action_routes.read().await;
        let custom_routes = self.custom_routes.read().await;
        let prefix_routes = self.prefix_routes.read().await;
        let default_route = self.default_route.read().await;

        RouteTableExport {
            action_routes: action_routes.clone(),
            custom_routes: custom_routes.iter().map(|c| c.entry.clone()).collect(),
            prefix_routes: prefix_routes.clone(),
            default_route: default_route.clone(),
        }
    }

    /// 获取路由统计信息
    pub async fn stats(&self) -> RouteTableStats {
        let action_routes = self.action_routes.read().await;
        let custom_routes = self.custom_routes.read().await;
        let prefix_routes = self.prefix_routes.read().await;
        let default_route = self.default_route.read().await;

        RouteTableStats {
            action_route_count: action_routes.values().map(|v| v.len()).sum(),
            custom_route_count: custom_routes.len(),
            prefix_route_count: prefix_routes.len(),
            has_default_route: default_route.is_some(),
        }
    }
}

impl Default for RouteTable {
    fn default() -> Self {
        Self::new()
    }
}

/// 路由表导出数据
#[derive(Debug, Clone, Serialize)]
pub struct RouteTableExport {
    pub action_routes: HashMap<String, Vec<RouteEntry>>,
    pub custom_routes: Vec<RouteEntry>,
    pub prefix_routes: Vec<RouteEntry>,
    pub default_route: Option<RouteEntry>,
}

/// 路由表统计信息
#[derive(Debug, Clone, Serialize)]
pub struct RouteTableStats {
    pub action_route_count: usize,
    pub custom_route_count: usize,
    pub prefix_route_count: usize,
    pub has_default_route: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_exact_route_match() {
        let table = RouteTable::new();
        let entry = RouteEntry::exact("filesystem", "file.open");
        table.register_action_route("file.open", entry).await.unwrap();

        let request = RouteRequest::new("editor", "file.open", json!({}));
        let result = table.find_route(&request).await;

        assert!(result.is_some());
        assert_eq!(result.unwrap().module_id, "filesystem");
    }

    #[tokio::test]
    async fn test_regex_route_match() {
        let table = RouteTable::new();
        let entry = RouteEntry::regex("filesystem", r"^file\..*");
        table.register_custom_route(entry).await.unwrap();

        let request = RouteRequest::new("editor", "file.save", json!({}));
        let result = table.find_route(&request).await;

        assert!(result.is_some());
        assert_eq!(result.unwrap().module_id, "filesystem");
    }

    #[tokio::test]
    async fn test_prefix_route_match() {
        let table = RouteTable::new();
        let entry = RouteEntry::prefix("network", "http.");
        table.register_prefix_route(entry).await.unwrap();

        let request = RouteRequest::new("app", "http.get", json!({}));
        let result = table.find_route(&request).await;

        assert!(result.is_some());
        assert_eq!(result.unwrap().module_id, "network");
    }

    #[tokio::test]
    async fn test_default_route() {
        let table = RouteTable::new();
        table
            .set_default_route(RouteEntry::exact("default_handler", "*"))
            .await;

        let request = RouteRequest::new("app", "unknown.action", json!({}));
        let result = table.find_route(&request).await;

        assert!(result.is_some());
        assert_eq!(result.unwrap().module_id, "default_handler");
    }

    #[tokio::test]
    async fn test_no_route_found() {
        let table = RouteTable::new();

        let request = RouteRequest::new("app", "unknown.action", json!({}));
        let result = table.find_route(&request).await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_priority_ordering() {
        let table = RouteTable::new();

        let low_priority = RouteEntry::exact("low_handler", "test.action").with_priority(1);
        let high_priority = RouteEntry::exact("high_handler", "test.action").with_priority(10);

        table
            .register_action_route("test.action", low_priority)
            .await
            .unwrap();
        table
            .register_action_route("test.action", high_priority)
            .await
            .unwrap();

        let request = RouteRequest::new("app", "test.action", json!({}));
        let result = table.find_route(&request).await;

        assert!(result.is_some());
        assert_eq!(result.unwrap().module_id, "high_handler");
    }

    #[tokio::test]
    async fn test_remove_module_routes() {
        let table = RouteTable::new();

        table
            .register_action_route("test.a", RouteEntry::exact("mod_a", "test.a"))
            .await
            .unwrap();
        table
            .register_action_route("test.b", RouteEntry::exact("mod_a", "test.b"))
            .await
            .unwrap();
        table
            .register_action_route("test.c", RouteEntry::exact("mod_b", "test.c"))
            .await
            .unwrap();

        table.remove_module_routes("mod_a").await;

        let stats = table.stats().await;
        assert_eq!(stats.action_route_count, 1);
    }

    #[tokio::test]
    async fn test_disabled_route() {
        let table = RouteTable::new();

        let mut entry = RouteEntry::exact("handler", "test.action");
        entry.enabled = false;
        table.register_action_route("test.action", entry).await.unwrap();

        let request = RouteRequest::new("app", "test.action", json!({}));
        let result = table.find_route(&request).await;

        assert!(result.is_none());
    }
}
