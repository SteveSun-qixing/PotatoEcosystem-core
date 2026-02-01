//! 路由表数据结构
//!
//! 管理 action 到模块的映射关系。
//! 包含 LRU 缓存优化，提升路由查找性能。

use lru::LruCache;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

use super::request::RouteRequest;
use crate::utils::Result;

// ============================================================================
// 路由缓存
// ============================================================================

/// 默认缓存容量
const DEFAULT_CACHE_CAPACITY: usize = 256;

/// 路由缓存统计信息
#[derive(Debug, Clone, Serialize)]
pub struct RouteCacheStats {
    /// 缓存命中次数
    pub hits: u64,
    /// 缓存未命中次数
    pub misses: u64,
    /// 缓存条目数量
    pub size: usize,
    /// 缓存容量
    pub capacity: usize,
    /// 命中率（百分比）
    pub hit_rate: f64,
}

/// 路由缓存
///
/// 使用 LRU 算法缓存最近使用的路由结果，提升路由查找性能。
pub struct RouteCache {
    /// LRU 缓存（action -> RouteEntry）
    cache: Mutex<LruCache<String, RouteEntry>>,
    /// 缓存命中次数
    hits: AtomicU64,
    /// 缓存未命中次数
    misses: AtomicU64,
    /// 缓存容量
    capacity: usize,
}

impl RouteCache {
    /// 创建新的路由缓存
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1); // 确保至少为 1
        Self {
            cache: Mutex::new(LruCache::new(NonZeroUsize::new(capacity).unwrap())),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            capacity,
        }
    }

    /// 使用默认容量创建缓存
    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_CACHE_CAPACITY)
    }

    /// 从缓存获取路由条目
    pub fn get(&self, action: &str) -> Option<RouteEntry> {
        let mut cache = self.cache.lock().unwrap();
        if let Some(entry) = cache.get(action).cloned() {
            self.hits.fetch_add(1, Ordering::Relaxed);
            Some(entry)
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    /// 将路由条目放入缓存
    pub fn put(&self, action: String, entry: RouteEntry) {
        let mut cache = self.cache.lock().unwrap();
        cache.put(action, entry);
    }

    /// 失效指定的缓存条目
    pub fn invalidate(&self, action: &str) {
        let mut cache = self.cache.lock().unwrap();
        cache.pop(action);
    }

    /// 失效与指定模块相关的所有缓存条目
    pub fn invalidate_module(&self, module_id: &str) {
        let mut cache = self.cache.lock().unwrap();
        // 收集需要移除的 keys
        let keys_to_remove: Vec<String> = cache
            .iter()
            .filter(|(_, entry)| entry.module_id == module_id)
            .map(|(key, _)| key.clone())
            .collect();
        
        // 移除这些条目
        for key in keys_to_remove {
            cache.pop(&key);
        }
    }

    /// 失效以指定前缀开头的所有缓存条目
    pub fn invalidate_prefix(&self, prefix: &str) {
        let mut cache = self.cache.lock().unwrap();
        let keys_to_remove: Vec<String> = cache
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .map(|(key, _)| key.clone())
            .collect();
        
        for key in keys_to_remove {
            cache.pop(&key);
        }
    }

    /// 清空所有缓存
    pub fn clear(&self) {
        let mut cache = self.cache.lock().unwrap();
        cache.clear();
    }

    /// 获取缓存统计信息
    pub fn stats(&self) -> RouteCacheStats {
        let cache = self.cache.lock().unwrap();
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        
        RouteCacheStats {
            hits,
            misses,
            size: cache.len(),
            capacity: self.capacity,
            hit_rate: if total > 0 {
                (hits as f64 / total as f64) * 100.0
            } else {
                0.0
            },
        }
    }

    /// 重置统计计数器
    pub fn reset_stats(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
    }
}

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

/// 路由表配置
#[derive(Debug, Clone)]
pub struct RouteTableConfig {
    /// 是否启用缓存
    pub cache_enabled: bool,
    /// 缓存容量
    pub cache_capacity: usize,
    /// 是否启用快速路径
    pub fast_path_enabled: bool,
    /// 快速路径前缀列表
    pub fast_path_prefixes: Vec<String>,
}

impl Default for RouteTableConfig {
    fn default() -> Self {
        Self {
            cache_enabled: true,
            cache_capacity: DEFAULT_CACHE_CAPACITY,
            fast_path_enabled: true,
            fast_path_prefixes: vec!["foundation.".to_string()],
        }
    }
}

/// 路由表
///
/// 管理 action 到模块的映射关系，支持精确匹配和正则匹配。
/// 集成 LRU 缓存提升路由查找性能。
pub struct RouteTable {
    /// 精确匹配路由表 (action -> [RouteEntry])
    action_routes: Arc<RwLock<HashMap<String, Vec<RouteEntry>>>>,

    /// 自定义正则路由（优先级最高）
    custom_routes: Arc<RwLock<Vec<CompiledRegexRoute>>>,

    /// 前缀匹配路由
    prefix_routes: Arc<RwLock<Vec<RouteEntry>>>,

    /// 默认路由（当没有匹配时使用）
    default_route: Arc<RwLock<Option<RouteEntry>>>,

    /// 路由缓存
    cache: Option<RouteCache>,

    /// 配置
    config: RouteTableConfig,
}

impl RouteTable {
    /// 创建空路由表（使用默认配置）
    pub fn new() -> Self {
        Self::with_config(RouteTableConfig::default())
    }

    /// 使用指定配置创建路由表
    pub fn with_config(config: RouteTableConfig) -> Self {
        let cache = if config.cache_enabled {
            Some(RouteCache::new(config.cache_capacity))
        } else {
            None
        };

        Self {
            action_routes: Arc::new(RwLock::new(HashMap::new())),
            custom_routes: Arc::new(RwLock::new(Vec::new())),
            prefix_routes: Arc::new(RwLock::new(Vec::new())),
            default_route: Arc::new(RwLock::new(None)),
            cache,
            config,
        }
    }

    /// 创建无缓存的路由表
    pub fn without_cache() -> Self {
        Self::with_config(RouteTableConfig {
            cache_enabled: false,
            ..Default::default()
        })
    }

    /// 注册精确匹配路由
    pub async fn register_action_route(&self, action: &str, entry: RouteEntry) -> Result<()> {
        let mut routes = self.action_routes.write().await;
        routes.entry(action.to_string()).or_default().push(entry);

        // 按优先级排序（高到低）
        if let Some(entries) = routes.get_mut(action) {
            entries.sort_by(|a, b| b.priority.cmp(&a.priority));
        }

        // 失效该 action 的缓存
        if let Some(ref cache) = self.cache {
            cache.invalidate(action);
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

        // 正则路由变更影响范围广，清空整个缓存
        if let Some(ref cache) = self.cache {
            cache.clear();
        }

        Ok(())
    }

    /// 注册前缀匹配路由
    pub async fn register_prefix_route(&self, entry: RouteEntry) -> Result<()> {
        let prefix = entry.pattern.clone();
        
        let mut routes = self.prefix_routes.write().await;
        routes.push(entry);

        // 按优先级排序（高到低）
        routes.sort_by(|a, b| b.priority.cmp(&a.priority));

        // 失效匹配该前缀的缓存
        if let Some(ref cache) = self.cache {
            cache.invalidate_prefix(&prefix);
        }

        Ok(())
    }

    /// 设置默认路由
    pub async fn set_default_route(&self, entry: RouteEntry) {
        let mut default = self.default_route.write().await;
        *default = Some(entry);
        
        // 默认路由变更可能影响之前未命中的查询，但已缓存的结果仍然有效
        // 这里不需要清空缓存
    }

    /// 查找匹配的路由
    ///
    /// 查找顺序：
    /// 1. 缓存查找（如果启用）
    /// 2. 快速路径检查（如果启用且匹配）
    /// 3. 自定义正则路由（优先级最高）
    /// 4. 精确匹配路由
    /// 5. 前缀匹配路由
    /// 6. 默认路由
    pub async fn find_route(&self, request: &RouteRequest) -> Option<RouteEntry> {
        let action = &request.action;

        // 1. 尝试从缓存获取
        if let Some(ref cache) = self.cache {
            if let Some(entry) = cache.get(action) {
                return Some(entry);
            }
        }

        // 2. 快速路径优化：对于 Foundation 模块请求，尝试快速匹配
        if self.config.fast_path_enabled {
            if let Some(entry) = self.try_fast_path(action).await {
                // 将结果放入缓存
                if let Some(ref cache) = self.cache {
                    cache.put(action.to_string(), entry.clone());
                }
                return Some(entry);
            }
        }

        // 3. 正常路由查找
        let entry = self.find_route_internal(action).await;

        // 4. 将结果放入缓存（包括 None 的情况不缓存）
        if let Some(ref found_entry) = entry {
            if let Some(ref cache) = self.cache {
                cache.put(action.to_string(), found_entry.clone());
            }
        }

        entry
    }

    /// 快速路径查找
    /// 
    /// 对于快速路径前缀列表中的请求，直接进行精确匹配或前缀匹配，
    /// 跳过正则路由检查以提升性能。
    async fn try_fast_path(&self, action: &str) -> Option<RouteEntry> {
        // 检查是否匹配快速路径前缀
        let is_fast_path = self.config.fast_path_prefixes
            .iter()
            .any(|prefix| action.starts_with(prefix));

        if !is_fast_path {
            return None;
        }

        // 快速路径：优先精确匹配
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

        // 快速路径：前缀匹配
        {
            let prefix_routes = self.prefix_routes.read().await;
            for entry in prefix_routes.iter() {
                if entry.matches(action) {
                    return Some(entry.clone());
                }
            }
        }

        None
    }

    /// 内部路由查找（完整流程）
    async fn find_route_internal(&self, action: &str) -> Option<RouteEntry> {
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

        // 失效该模块相关的缓存
        if let Some(ref cache) = self.cache {
            cache.invalidate_module(module_id);
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
            cache_stats: self.cache.as_ref().map(|c| c.stats()),
        }
    }

    /// 获取缓存统计信息
    pub fn cache_stats(&self) -> Option<RouteCacheStats> {
        self.cache.as_ref().map(|c| c.stats())
    }

    /// 重置缓存统计
    pub fn reset_cache_stats(&self) {
        if let Some(ref cache) = self.cache {
            cache.reset_stats();
        }
    }

    /// 清空路由缓存
    pub fn clear_cache(&self) {
        if let Some(ref cache) = self.cache {
            cache.clear();
        }
    }

    /// 检查缓存是否启用
    pub fn is_cache_enabled(&self) -> bool {
        self.cache.is_some()
    }

    /// 获取配置
    pub fn config(&self) -> &RouteTableConfig {
        &self.config
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
    /// 缓存统计（如果启用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_stats: Option<RouteCacheStats>,
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

    // ========================================================================
    // 缓存相关测试
    // ========================================================================

    #[test]
    fn test_route_cache_basic() {
        let cache = RouteCache::new(10);
        
        // 初始状态应该是空的
        assert!(cache.get("test.action").is_none());
        
        // 添加条目
        let entry = RouteEntry::exact("handler", "test.action");
        cache.put("test.action".to_string(), entry.clone());
        
        // 现在应该能获取到
        let cached = cache.get("test.action");
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().module_id, "handler");
        
        // 检查统计
        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.size, 1);
    }

    #[test]
    fn test_route_cache_invalidate() {
        let cache = RouteCache::new(10);
        
        let entry1 = RouteEntry::exact("handler1", "test.action1");
        let entry2 = RouteEntry::exact("handler2", "test.action2");
        
        cache.put("test.action1".to_string(), entry1);
        cache.put("test.action2".to_string(), entry2);
        
        assert!(cache.get("test.action1").is_some());
        assert!(cache.get("test.action2").is_some());
        
        // 失效单个条目
        cache.invalidate("test.action1");
        assert!(cache.get("test.action1").is_none());
        assert!(cache.get("test.action2").is_some());
    }

    #[test]
    fn test_route_cache_invalidate_module() {
        let cache = RouteCache::new(10);
        
        cache.put("a.action1".to_string(), RouteEntry::exact("mod_a", "a.action1"));
        cache.put("a.action2".to_string(), RouteEntry::exact("mod_a", "a.action2"));
        cache.put("b.action1".to_string(), RouteEntry::exact("mod_b", "b.action1"));
        
        assert_eq!(cache.stats().size, 3);
        
        // 失效模块 mod_a 的所有缓存
        cache.invalidate_module("mod_a");
        
        assert!(cache.get("a.action1").is_none());
        assert!(cache.get("a.action2").is_none());
        assert!(cache.get("b.action1").is_some());
    }

    #[test]
    fn test_route_cache_invalidate_prefix() {
        let cache = RouteCache::new(10);
        
        cache.put("file.open".to_string(), RouteEntry::exact("fs", "file.open"));
        cache.put("file.save".to_string(), RouteEntry::exact("fs", "file.save"));
        cache.put("http.get".to_string(), RouteEntry::exact("net", "http.get"));
        
        // 失效所有 file. 前缀的缓存
        cache.invalidate_prefix("file.");
        
        assert!(cache.get("file.open").is_none());
        assert!(cache.get("file.save").is_none());
        assert!(cache.get("http.get").is_some());
    }

    #[test]
    fn test_route_cache_lru_eviction() {
        let cache = RouteCache::new(2); // 容量为 2
        
        cache.put("action1".to_string(), RouteEntry::exact("h1", "action1"));
        cache.put("action2".to_string(), RouteEntry::exact("h2", "action2"));
        cache.put("action3".to_string(), RouteEntry::exact("h3", "action3")); // 应该驱逐 action1
        
        // action1 应该被驱逐
        assert!(cache.get("action1").is_none());
        assert!(cache.get("action2").is_some());
        assert!(cache.get("action3").is_some());
    }

    #[test]
    fn test_route_cache_hit_rate() {
        let cache = RouteCache::new(10);
        
        cache.put("test".to_string(), RouteEntry::exact("h", "test"));
        
        // 3 次命中
        cache.get("test");
        cache.get("test");
        cache.get("test");
        
        // 1 次未命中
        cache.get("nonexistent");
        
        let stats = cache.stats();
        assert_eq!(stats.hits, 3);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate - 75.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_route_table_cache_integration() {
        let table = RouteTable::new();
        
        assert!(table.is_cache_enabled());
        
        // 注册路由
        table.register_action_route("test.action", RouteEntry::exact("handler", "test.action"))
            .await.unwrap();
        
        // 第一次查找（缓存未命中）
        let request = RouteRequest::new("app", "test.action", json!({}));
        let result = table.find_route(&request).await;
        assert!(result.is_some());
        
        // 第二次查找（缓存命中）
        let result2 = table.find_route(&request).await;
        assert!(result2.is_some());
        
        // 检查缓存统计
        let cache_stats = table.cache_stats().unwrap();
        assert_eq!(cache_stats.hits, 1);
        assert_eq!(cache_stats.misses, 1);
    }

    #[tokio::test]
    async fn test_route_table_cache_invalidation_on_update() {
        let table = RouteTable::new();
        
        // 注册初始路由
        table.register_action_route("test.action", RouteEntry::exact("old_handler", "test.action"))
            .await.unwrap();
        
        // 查找并缓存
        let request = RouteRequest::new("app", "test.action", json!({}));
        let result = table.find_route(&request).await;
        assert_eq!(result.unwrap().module_id, "old_handler");
        
        // 注册新路由（更高优先级）
        let new_entry = RouteEntry::exact("new_handler", "test.action").with_priority(10);
        table.register_action_route("test.action", new_entry).await.unwrap();
        
        // 缓存应该已失效，返回新路由
        let result2 = table.find_route(&request).await;
        assert_eq!(result2.unwrap().module_id, "new_handler");
    }

    #[tokio::test]
    async fn test_route_table_without_cache() {
        let table = RouteTable::without_cache();
        
        assert!(!table.is_cache_enabled());
        assert!(table.cache_stats().is_none());
        
        // 功能应该正常工作
        table.register_action_route("test.action", RouteEntry::exact("handler", "test.action"))
            .await.unwrap();
        
        let request = RouteRequest::new("app", "test.action", json!({}));
        let result = table.find_route(&request).await;
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_fast_path_foundation_routes() {
        let config = RouteTableConfig {
            cache_enabled: true,
            cache_capacity: 100,
            fast_path_enabled: true,
            fast_path_prefixes: vec!["foundation.".to_string()],
        };
        let table = RouteTable::with_config(config);
        
        // 注册 foundation 路由
        table.register_action_route(
            "foundation.storage.get", 
            RouteEntry::exact("foundation", "foundation.storage.get")
        ).await.unwrap();
        
        // 查找 foundation 路由（使用快速路径）
        let request = RouteRequest::new("app", "foundation.storage.get", json!({}));
        let result = table.find_route(&request).await;
        
        assert!(result.is_some());
        assert_eq!(result.unwrap().module_id, "foundation");
    }

    #[tokio::test]
    async fn test_cache_clear_on_regex_route_add() {
        let table = RouteTable::new();
        
        // 注册并缓存一些路由
        table.register_action_route("file.open", RouteEntry::exact("fs", "file.open"))
            .await.unwrap();
        
        let request = RouteRequest::new("app", "file.open", json!({}));
        table.find_route(&request).await;
        
        let stats = table.cache_stats().unwrap();
        assert_eq!(stats.size, 1);
        
        // 添加正则路由应该清空缓存
        table.register_custom_route(RouteEntry::regex("fs2", r"^file\..*"))
            .await.unwrap();
        
        let stats = table.cache_stats().unwrap();
        assert_eq!(stats.size, 0);
    }
}
