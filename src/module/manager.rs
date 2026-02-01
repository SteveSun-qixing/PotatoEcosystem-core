//! 模块管理器
//!
//! 整合模块管理系统的所有组件，提供统一的模块管理接口。

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, debug, instrument};

use super::dependency::DependencyGraph;
use super::lifecycle::LifecycleManager;
use super::loader::ModuleLoader;
use super::metadata::{HealthStatus, ModuleInfo, ModuleState};
use super::parser::ModuleParser;
use super::registry::ModuleRegistry;
use super::runtime::{RuntimeManager, RustRuntimeManager};
use crate::router::{Event, RouteEntry, RouteTable, system_events};
use crate::utils::{CoreError, Result};

/// 模块管理器配置
#[derive(Debug, Clone)]
pub struct ModuleManagerConfig {
    /// 模块目录列表
    pub module_dirs: Vec<PathBuf>,
    /// 是否启用热插拔
    pub hot_reload: bool,
    /// 健康检查间隔（秒）
    pub health_check_interval_secs: u64,
    /// 自动加载的模块列表
    pub auto_load: Vec<String>,
}

impl Default for ModuleManagerConfig {
    fn default() -> Self {
        Self {
            module_dirs: vec![],
            hot_reload: true,
            health_check_interval_secs: 30,
            auto_load: vec![],
        }
    }
}

/// 模块管理器
///
/// 负责模块的整个生命周期管理，包括：
/// - 模块扫描和注册
/// - 依赖解析
/// - 模块加载和卸载
/// - 生命周期管理
/// - 热插拔支持
pub struct ModuleManager {
    /// 配置
    config: ModuleManagerConfig,
    /// 模块注册表
    registry: ModuleRegistry,
    /// 模块加载器
    loader: ModuleLoader,
    /// 生命周期管理器
    lifecycle: LifecycleManager,
    /// 运行时管理器
    runtime_manager: Arc<dyn RuntimeManager>,
    /// 依赖图
    dependency_graph: Arc<RwLock<DependencyGraph>>,
    /// 路由表引用（用于注册模块路由）
    route_table: Option<Arc<RouteTable>>,
    /// 事件发布函数
    event_publisher: Option<Arc<dyn Fn(Event) -> futures::future::BoxFuture<'static, ()> + Send + Sync>>,
}

impl ModuleManager {
    /// 创建新的模块管理器
    pub fn new(config: ModuleManagerConfig) -> Self {
        let registry = ModuleRegistry::new(config.module_dirs.clone());
        let runtime_manager: Arc<dyn RuntimeManager> = Arc::new(RustRuntimeManager::new());
        let loader = ModuleLoader::new();
        let lifecycle = LifecycleManager::new();
        
        Self {
            config,
            registry,
            loader,
            lifecycle,
            runtime_manager,
            dependency_graph: Arc::new(RwLock::new(DependencyGraph::new())),
            route_table: None,
            event_publisher: None,
        }
    }

    /// 使用默认配置创建
    pub fn with_defaults() -> Self {
        Self::new(ModuleManagerConfig::default())
    }

    /// 设置路由表（用于注册模块路由）
    pub fn set_route_table(&mut self, route_table: Arc<RouteTable>) {
        self.route_table = Some(route_table);
    }

    /// 设置事件发布器
    pub fn set_event_publisher<F>(&mut self, publisher: F)
    where
        F: Fn(Event) -> futures::future::BoxFuture<'static, ()> + Send + Sync + 'static,
    {
        self.event_publisher = Some(Arc::new(publisher));
    }

    /// 初始化模块管理器
    ///
    /// 扫描模块目录并构建依赖图
    #[instrument(skip(self))]
    pub async fn initialize(&self) -> Result<()> {
        info!("初始化模块管理器");
        
        // 扫描模块目录
        let registered = self.registry.scan().await?;
        info!(count = registered.len(), "扫描到模块");
        
        // 构建依赖图
        self.build_dependency_graph().await?;
        
        // 自动加载配置的模块
        for module_id in &self.config.auto_load {
            if let Err(e) = self.load_module(module_id).await {
                warn!(module_id, error = %e, "自动加载模块失败");
            }
        }
        
        Ok(())
    }

    /// 构建依赖图
    async fn build_dependency_graph(&self) -> Result<()> {
        let modules = self.registry.list_modules().await;
        let mut graph = self.dependency_graph.write().await;
        graph.clear();
        
        for module in &modules {
            graph.add_module(&module.metadata.id);
            for dep in &module.metadata.dependencies {
                graph.add_dependency(&module.metadata.id, &dep.module_id);
            }
        }
        
        // 检查循环依赖
        if graph.has_cycle() {
            if let Some(cycle) = graph.find_cycle() {
                return Err(CoreError::CircularDependency(cycle.join(" -> ")));
            }
        }
        
        Ok(())
    }

    /// 加载模块
    #[instrument(skip(self), fields(module_id = %module_id))]
    pub async fn load_module(&self, module_id: &str) -> Result<()> {
        info!("加载模块");
        
        // 获取模块信息
        let module_info = self.registry.get_module(module_id).await
            .ok_or_else(|| CoreError::ModuleNotFound(module_id.to_string()))?;
        
        // 检查状态
        if !module_info.state.can_load() {
            return Err(CoreError::ModuleLoadFailed {
                module_id: module_id.to_string(),
                reason: format!("模块当前状态 {:?} 不允许加载", module_info.state),
            });
        }
        
        // 解析并加载依赖
        let load_order = self.resolve_dependencies(module_id).await?;
        
        for dep_id in &load_order {
            if dep_id != module_id {
                // 递归加载依赖
                if let Some(dep_info) = self.registry.get_module(dep_id).await {
                    if dep_info.state.can_load() {
                        self.load_single_module(dep_id).await?;
                    }
                }
            }
        }
        
        // 加载目标模块
        self.load_single_module(module_id).await?;
        
        // 发布事件
        self.publish_event(Event::new(
            system_events::MODULE_LOADED,
            "module_manager",
            serde_json::json!({ "module_id": module_id }),
        )).await;
        
        Ok(())
    }

    /// 加载单个模块（不处理依赖）
    async fn load_single_module(&self, module_id: &str) -> Result<()> {
        // 更新状态
        self.registry.set_state(module_id, ModuleState::Loading).await?;
        
        // 获取模块信息
        let module_info = self.registry.get_module(module_id).await
            .ok_or_else(|| CoreError::ModuleNotFound(module_id.to_string()))?;
        
        // 调用加载器
        if let Err(e) = self.loader.load(&module_info, self.runtime_manager.as_ref()).await {
            self.registry.set_error(module_id, &e.to_string()).await?;
            return Err(e);
        }
        
        // 更新状态为已加载
        self.registry.set_state(module_id, ModuleState::Loaded).await?;
        
        // 调用初始化钩子
        self.lifecycle.call_init(module_id, self.runtime_manager.as_ref()).await?;
        
        // 注册路由
        if let Some(ref route_table) = self.route_table {
            self.register_module_routes(module_id, &module_info, route_table).await?;
        }
        
        debug!(module_id, "模块加载完成");
        Ok(())
    }

    /// 注册模块路由
    async fn register_module_routes(
        &self,
        module_id: &str,
        module_info: &ModuleInfo,
        route_table: &RouteTable,
    ) -> Result<()> {
        for action in &module_info.metadata.interfaces.provides {
            let entry = RouteEntry::exact(module_id, action);
            route_table.register_action_route(action, entry).await?;
            debug!(module_id, action, "注册路由");
        }
        Ok(())
    }

    /// 卸载模块
    #[instrument(skip(self), fields(module_id = %module_id))]
    pub async fn unload_module(&self, module_id: &str) -> Result<()> {
        info!("卸载模块");
        
        // 获取模块信息
        let module_info = self.registry.get_module(module_id).await
            .ok_or_else(|| CoreError::ModuleNotFound(module_id.to_string()))?;
        
        // 检查状态
        if !module_info.state.can_unload() {
            return Err(CoreError::ModuleUnloadFailed {
                module_id: module_id.to_string(),
                reason: format!("模块当前状态 {:?} 不允许卸载", module_info.state),
            });
        }
        
        // 检查是否有依赖此模块的其他模块
        let dependents = self.get_dependents(module_id).await?;
        let running_dependents: Vec<_> = futures::future::join_all(
            dependents.iter().map(|id| async {
                self.registry.get_module(id).await
                    .map(|info| (id.clone(), info.state))
            })
        ).await
        .into_iter()
        .flatten()
        .filter(|(_, state)| matches!(state, ModuleState::Running | ModuleState::Loaded))
        .map(|(id, _)| id)
        .collect();
        
        if !running_dependents.is_empty() {
            return Err(CoreError::ModuleHasDependents {
                module: module_id.to_string(),
                dependents: running_dependents,
            });
        }
        
        // 更新状态
        self.registry.set_state(module_id, ModuleState::Unloading).await?;
        
        // 调用清理钩子
        self.lifecycle.call_cleanup(module_id, self.runtime_manager.as_ref()).await?;
        
        // 调用加载器卸载
        if let Err(e) = self.loader.unload(module_id, self.runtime_manager.as_ref()).await {
            self.registry.set_error(module_id, &e.to_string()).await?;
            return Err(e);
        }
        
        // 移除路由
        if let Some(ref route_table) = self.route_table {
            route_table.remove_module_routes(module_id).await;
        }
        
        // 更新状态
        self.registry.set_state(module_id, ModuleState::Unloaded).await?;
        
        // 发布事件
        self.publish_event(Event::new(
            system_events::MODULE_UNLOADED,
            "module_manager",
            serde_json::json!({ "module_id": module_id }),
        )).await;
        
        debug!(module_id, "模块卸载完成");
        Ok(())
    }

    /// 启动模块
    #[instrument(skip(self), fields(module_id = %module_id))]
    pub async fn start_module(&self, module_id: &str) -> Result<()> {
        info!("启动模块");
        
        // 更新状态
        self.registry.set_state(module_id, ModuleState::Starting).await?;
        
        // 调用启动钩子
        if let Err(e) = self.lifecycle.call_start(module_id, self.runtime_manager.as_ref()).await {
            self.registry.set_error(module_id, &e.to_string()).await?;
            return Err(e);
        }
        
        // 更新状态为运行中
        self.registry.set_state(module_id, ModuleState::Running).await?;
        
        // 发布事件
        self.publish_event(Event::new(
            system_events::MODULE_STARTED,
            "module_manager",
            serde_json::json!({ "module_id": module_id }),
        )).await;
        
        Ok(())
    }

    /// 停止模块
    #[instrument(skip(self), fields(module_id = %module_id))]
    pub async fn stop_module(&self, module_id: &str) -> Result<()> {
        info!("停止模块");
        
        // 更新状态
        self.registry.set_state(module_id, ModuleState::Stopping).await?;
        
        // 调用停止钩子
        if let Err(e) = self.lifecycle.call_stop(module_id, self.runtime_manager.as_ref()).await {
            self.registry.set_error(module_id, &e.to_string()).await?;
            return Err(e);
        }
        
        // 更新状态为已停止
        self.registry.set_state(module_id, ModuleState::Stopped).await?;
        
        // 发布事件
        self.publish_event(Event::new(
            system_events::MODULE_STOPPED,
            "module_manager",
            serde_json::json!({ "module_id": module_id }),
        )).await;
        
        Ok(())
    }

    /// 重启模块
    pub async fn restart_module(&self, module_id: &str) -> Result<()> {
        self.stop_module(module_id).await?;
        self.start_module(module_id).await?;
        Ok(())
    }

    /// 热安装模块
    #[instrument(skip(self), fields(path = %path.display()))]
    pub async fn hot_install(&self, path: &PathBuf) -> Result<String> {
        if !self.config.hot_reload {
            return Err(CoreError::InitFailed("热插拔未启用".to_string()));
        }
        
        info!("热安装模块");
        
        // 解析模块元数据
        let module_yaml = path.join("module.yaml");
        let metadata = ModuleParser::parse_file(&module_yaml).await?;
        let module_id = metadata.id.clone();
        
        // 注册模块
        self.registry.register(path).await?;
        
        // 更新依赖图
        self.build_dependency_graph().await?;
        
        // 加载模块
        self.load_module(&module_id).await?;
        
        // 启动模块
        self.start_module(&module_id).await?;
        
        info!(module_id, "模块热安装完成");
        Ok(module_id)
    }

    /// 热卸载模块
    pub async fn hot_uninstall(&self, module_id: &str) -> Result<()> {
        if !self.config.hot_reload {
            return Err(CoreError::InitFailed("热插拔未启用".to_string()));
        }
        
        info!(module_id, "热卸载模块");
        
        // 停止模块
        if let Some(info) = self.registry.get_module(module_id).await {
            if info.state == ModuleState::Running {
                self.stop_module(module_id).await?;
            }
        }
        
        // 卸载模块
        self.unload_module(module_id).await?;
        
        // 从注册表移除
        self.registry.unregister(module_id).await?;
        
        // 更新依赖图
        {
            let mut graph = self.dependency_graph.write().await;
            graph.remove_module(module_id);
        }
        
        info!(module_id, "模块热卸载完成");
        Ok(())
    }

    /// 解析模块依赖
    async fn resolve_dependencies(&self, module_id: &str) -> Result<Vec<String>> {
        let graph = self.dependency_graph.read().await;
        
        // 获取模块的所有依赖
        let deps = graph.get_all_dependencies(module_id);
        
        // 将目标模块添加到依赖列表末尾
        let mut load_order = deps;
        if !load_order.contains(&module_id.to_string()) {
            load_order.push(module_id.to_string());
        }
        
        Ok(load_order)
    }

    /// 获取依赖此模块的模块列表
    async fn get_dependents(&self, module_id: &str) -> Result<Vec<String>> {
        let graph = self.dependency_graph.read().await;
        Ok(graph.get_dependents(module_id))
    }

    /// 发布事件
    async fn publish_event(&self, event: Event) {
        if let Some(ref publisher) = self.event_publisher {
            publisher(event).await;
        }
    }

    /// 获取模块信息
    pub async fn get_module(&self, module_id: &str) -> Option<ModuleInfo> {
        self.registry.get_module(module_id).await
    }

    /// 获取所有模块
    pub async fn list_modules(&self) -> Vec<ModuleInfo> {
        self.registry.list_modules().await
    }

    /// 获取运行中的模块
    pub async fn get_running_modules(&self) -> Vec<ModuleInfo> {
        self.registry.get_running_modules().await
    }

    /// 健康检查
    pub async fn health_check(&self, module_id: &str) -> Result<HealthStatus> {
        self.lifecycle.health_check(module_id, self.runtime_manager.as_ref()).await
    }

    /// 检查所有模块健康状态
    pub async fn health_check_all(&self) -> std::collections::HashMap<String, HealthStatus> {
        self.lifecycle.health_check_all(self.runtime_manager.as_ref()).await
    }

    /// 获取模块数量
    pub async fn module_count(&self) -> usize {
        self.registry.count().await
    }

    /// 获取配置
    pub fn config(&self) -> &ModuleManagerConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_manager_creation() {
        let manager = ModuleManager::with_defaults();
        assert_eq!(manager.module_count().await, 0);
    }

    #[tokio::test]
    async fn test_manager_with_config() {
        let config = ModuleManagerConfig {
            module_dirs: vec![PathBuf::from("./modules")],
            hot_reload: true,
            health_check_interval_secs: 60,
            auto_load: vec!["test_module".to_string()],
        };
        let manager = ModuleManager::new(config);
        assert!(manager.config.hot_reload);
    }

    #[tokio::test]
    async fn test_load_nonexistent_module() {
        let manager = ModuleManager::with_defaults();
        let result = manager.load_module("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_module_operations() {
        let dir = tempdir().unwrap();
        let module_dir = dir.path().join("test_module");
        std::fs::create_dir_all(&module_dir).unwrap();
        
        // 创建 module.yaml
        let module_yaml = module_dir.join("module.yaml");
        std::fs::write(&module_yaml, r#"
id: test-module
name: Test Module
version: 1.0.0
entry:
  runtime: rust
  path: lib.rs
interfaces:
  provides:
    - test.action
"#).unwrap();
        
        let config = ModuleManagerConfig {
            module_dirs: vec![dir.path().to_path_buf()],
            ..Default::default()
        };
        let manager = ModuleManager::new(config);
        
        // 初始化
        manager.initialize().await.unwrap();
        
        // 应该扫描到一个模块
        assert_eq!(manager.module_count().await, 1);
        
        // 获取模块信息
        let module = manager.get_module("test-module").await;
        assert!(module.is_some());
        assert_eq!(module.unwrap().metadata.name, "Test Module");
    }
}
