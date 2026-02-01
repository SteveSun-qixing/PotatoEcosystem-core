//! 模块加载器
//!
//! 负责模块的加载和卸载操作，管理模块的加载状态。

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use super::metadata::{ModuleInfo, ModuleState, Runtime};
use super::runtime::{ModuleInstance, RuntimeManager};
use crate::utils::{CoreError, Result};

/// 模块加载器
///
/// 负责实际加载和卸载模块代码，管理已加载模块的实例。
pub struct ModuleLoader {
    /// 已加载的模块实例
    instances: Arc<RwLock<HashMap<String, ModuleInstance>>>,
    
    /// 模块状态
    states: Arc<RwLock<HashMap<String, ModuleState>>>,
    
    /// 依赖图：module_id -> 被依赖的模块列表
    dependents: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl ModuleLoader {
    /// 创建新的模块加载器
    pub fn new() -> Self {
        info!("创建模块加载器");
        Self {
            instances: Arc::new(RwLock::new(HashMap::new())),
            states: Arc::new(RwLock::new(HashMap::new())),
            dependents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 加载模块
    ///
    /// # 加载流程
    /// 1. 检查模块是否已加载
    /// 2. 设置状态为 Loading
    /// 3. 加载模块代码（骨架实现）
    /// 4. 初始化模块
    /// 5. 设置状态为 Loaded
    ///
    /// # Arguments
    /// * `module_info` - 模块信息
    /// * `runtime_manager` - 运行时管理器
    ///
    /// # Returns
    /// 成功返回模块实例引用，失败返回错误
    pub async fn load(
        &self,
        module_info: &ModuleInfo,
        runtime_manager: &dyn RuntimeManager,
    ) -> Result<()> {
        let module_id = module_info.id();
        info!(module_id = %module_id, "开始加载模块");

        // 1. 检查模块是否已加载
        {
            let states = self.states.read().await;
            if let Some(state) = states.get(module_id) {
                match state {
                    ModuleState::Loaded | ModuleState::Running => {
                        warn!(module_id = %module_id, "模块已加载");
                        return Err(CoreError::ModuleAlreadyLoaded(module_id.to_string()));
                    }
                    ModuleState::Loading => {
                        warn!(module_id = %module_id, "模块正在加载中");
                        return Err(CoreError::ModuleLoadFailed {
                            module_id: module_id.to_string(),
                            reason: "模块正在加载中".to_string(),
                        });
                    }
                    _ => {}
                }
            }
        }

        // 2. 设置状态为 Loading
        self.set_state(module_id, ModuleState::Loading).await;
        debug!(module_id = %module_id, "模块状态设置为 Loading");

        // 3. 加载模块代码
        let instance = match runtime_manager.load_module(module_info).await {
            Ok(inst) => inst,
            Err(e) => {
                error!(module_id = %module_id, error = ?e, "模块代码加载失败");
                self.set_state(module_id, ModuleState::Error).await;
                return Err(CoreError::ModuleLoadFailed {
                    module_id: module_id.to_string(),
                    reason: e.to_string(),
                });
            }
        };

        // 4. 初始化模块（调用 onInit 钩子 - 在骨架实现中通过 runtime_manager）
        if let Err(e) = runtime_manager
            .call_method(module_id, "onInit", serde_json::json!({}))
            .await
        {
            // 对于骨架实现，忽略初始化错误
            debug!(module_id = %module_id, error = ?e, "模块初始化（骨架实现）");
        }

        // 5. 存储模块实例
        {
            let mut instances = self.instances.write().await;
            instances.insert(module_id.to_string(), instance);
        }

        // 6. 记录依赖关系
        self.record_dependencies(module_info).await;

        // 7. 设置状态为 Loaded
        self.set_state(module_id, ModuleState::Loaded).await;
        info!(module_id = %module_id, "模块加载完成");

        Ok(())
    }

    /// 卸载模块
    ///
    /// # 卸载流程
    /// 1. 检查是否有其他模块依赖
    /// 2. 调用模块的 cleanup
    /// 3. 设置状态为 Unloaded
    ///
    /// # Arguments
    /// * `module_id` - 模块 ID
    /// * `runtime_manager` - 运行时管理器
    ///
    /// # Returns
    /// 成功返回 ()，失败返回错误
    pub async fn unload(
        &self,
        module_id: &str,
        runtime_manager: &dyn RuntimeManager,
    ) -> Result<()> {
        info!(module_id = %module_id, "开始卸载模块");

        // 1. 检查模块状态
        {
            let states = self.states.read().await;
            match states.get(module_id) {
                None => {
                    return Err(CoreError::ModuleNotLoaded(module_id.to_string()));
                }
                Some(state) if !state.can_unload() => {
                    return Err(CoreError::ModuleUnloadFailed {
                        module_id: module_id.to_string(),
                        reason: format!("模块状态 {:?} 不允许卸载", state),
                    });
                }
                _ => {}
            }
        }

        // 2. 检查是否有其他模块依赖此模块
        let dependents = self.get_dependents(module_id).await;
        if !dependents.is_empty() {
            error!(module_id = %module_id, dependents = ?dependents, "模块有依赖者，无法卸载");
            return Err(CoreError::ModuleHasDependents {
                module: module_id.to_string(),
                dependents,
            });
        }

        // 3. 设置状态为 Unloading
        self.set_state(module_id, ModuleState::Unloading).await;

        // 4. 调用模块的 cleanup 钩子
        if let Err(e) = runtime_manager
            .call_method(module_id, "onCleanup", serde_json::json!({}))
            .await
        {
            // 对于骨架实现，记录但不阻止卸载
            debug!(module_id = %module_id, error = ?e, "模块清理（骨架实现）");
        }

        // 5. 卸载模块代码
        if let Err(e) = runtime_manager.unload_module(module_id).await {
            error!(module_id = %module_id, error = ?e, "运行时卸载模块失败");
            self.set_state(module_id, ModuleState::Error).await;
            return Err(CoreError::ModuleUnloadFailed {
                module_id: module_id.to_string(),
                reason: e.to_string(),
            });
        }

        // 6. 移除模块实例
        {
            let mut instances = self.instances.write().await;
            instances.remove(module_id);
        }

        // 7. 清理依赖关系
        self.remove_dependencies(module_id).await;

        // 8. 设置状态为 Unloaded
        self.set_state(module_id, ModuleState::Unloaded).await;
        info!(module_id = %module_id, "模块卸载完成");

        Ok(())
    }

    /// 检查模块是否已加载
    pub async fn is_loaded(&self, module_id: &str) -> bool {
        let states = self.states.read().await;
        matches!(
            states.get(module_id),
            Some(ModuleState::Loaded) | Some(ModuleState::Running)
        )
    }

    /// 获取模块状态
    pub async fn get_state(&self, module_id: &str) -> Option<ModuleState> {
        let states = self.states.read().await;
        states.get(module_id).copied()
    }

    /// 设置模块状态
    pub async fn set_state(&self, module_id: &str, state: ModuleState) {
        let mut states = self.states.write().await;
        states.insert(module_id.to_string(), state);
    }

    /// 获取模块实例
    pub async fn get_instance(&self, module_id: &str) -> Option<Runtime> {
        let instances = self.instances.read().await;
        instances.get(module_id).map(|i| i.runtime.clone())
    }

    /// 获取所有已加载模块的 ID
    pub async fn loaded_modules(&self) -> Vec<String> {
        let instances = self.instances.read().await;
        instances.keys().cloned().collect()
    }

    /// 获取依赖此模块的所有模块
    pub async fn get_dependents(&self, module_id: &str) -> Vec<String> {
        let dependents = self.dependents.read().await;
        dependents.get(module_id).cloned().unwrap_or_default()
    }

    /// 记录模块依赖关系
    async fn record_dependencies(&self, module_info: &ModuleInfo) {
        let module_id = module_info.id();
        let deps = &module_info.metadata.dependencies;

        let mut dependents = self.dependents.write().await;
        for dep in deps {
            dependents
                .entry(dep.module_id.clone())
                .or_insert_with(Vec::new)
                .push(module_id.to_string());
        }
    }

    /// 移除模块的依赖关系记录
    async fn remove_dependencies(&self, module_id: &str) {
        let mut dependents = self.dependents.write().await;
        
        // 从所有依赖列表中移除此模块
        for deps in dependents.values_mut() {
            deps.retain(|id| id != module_id);
        }
        
        // 移除此模块的依赖者列表
        dependents.remove(module_id);
    }
}

impl Default for ModuleLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::metadata::{EntryPoint, ModuleMetadata};
    use crate::module::runtime::RustRuntimeManager;
    use std::path::PathBuf;

    fn create_test_module(id: &str) -> ModuleInfo {
        let mut metadata = ModuleMetadata::new(id, format!("Test {}", id), "1.0.0");
        metadata.entry = EntryPoint::rust("lib.rs");
        ModuleInfo::new(metadata, PathBuf::from("/test"))
    }

    fn create_test_module_with_dep(id: &str, dep_id: &str) -> ModuleInfo {
        let mut metadata = ModuleMetadata::new(id, format!("Test {}", id), "1.0.0");
        metadata.entry = EntryPoint::rust("lib.rs");
        metadata.dependencies.push(crate::module::Dependency::new(dep_id, "^1.0.0"));
        ModuleInfo::new(metadata, PathBuf::from("/test"))
    }

    #[tokio::test]
    async fn test_loader_creation() {
        let loader = ModuleLoader::new();
        assert!(loader.loaded_modules().await.is_empty());
    }

    #[tokio::test]
    async fn test_load_module() {
        let loader = ModuleLoader::new();
        let runtime = RustRuntimeManager::new();
        let module = create_test_module("test_module");

        let result = loader.load(&module, &runtime).await;
        assert!(result.is_ok());
        assert!(loader.is_loaded("test_module").await);
        assert_eq!(loader.get_state("test_module").await, Some(ModuleState::Loaded));
    }

    #[tokio::test]
    async fn test_load_module_already_loaded() {
        let loader = ModuleLoader::new();
        let runtime = RustRuntimeManager::new();
        let module = create_test_module("test_module");

        loader.load(&module, &runtime).await.unwrap();
        let result = loader.load(&module, &runtime).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::ModuleAlreadyLoaded(_)));
    }

    #[tokio::test]
    async fn test_unload_module() {
        let loader = ModuleLoader::new();
        let runtime = RustRuntimeManager::new();
        let module = create_test_module("test_module");

        loader.load(&module, &runtime).await.unwrap();
        let result = loader.unload("test_module", &runtime).await;

        assert!(result.is_ok());
        assert!(!loader.is_loaded("test_module").await);
        assert_eq!(loader.get_state("test_module").await, Some(ModuleState::Unloaded));
    }

    #[tokio::test]
    async fn test_unload_not_loaded_module() {
        let loader = ModuleLoader::new();
        let runtime = RustRuntimeManager::new();

        let result = loader.unload("nonexistent", &runtime).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::ModuleNotLoaded(_)));
    }

    #[tokio::test]
    async fn test_unload_with_dependents() {
        let loader = ModuleLoader::new();
        let runtime = RustRuntimeManager::new();

        // 加载基础模块
        let base_module = create_test_module("base_module");
        loader.load(&base_module, &runtime).await.unwrap();

        // 加载依赖基础模块的模块
        let dependent_module = create_test_module_with_dep("dependent_module", "base_module");
        loader.load(&dependent_module, &runtime).await.unwrap();

        // 尝试卸载被依赖的模块
        let result = loader.unload("base_module", &runtime).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::ModuleHasDependents { .. }));
    }

    #[tokio::test]
    async fn test_unload_dependent_first() {
        let loader = ModuleLoader::new();
        let runtime = RustRuntimeManager::new();

        // 加载基础模块
        let base_module = create_test_module("base_module");
        loader.load(&base_module, &runtime).await.unwrap();

        // 加载依赖基础模块的模块
        let dependent_module = create_test_module_with_dep("dependent_module", "base_module");
        loader.load(&dependent_module, &runtime).await.unwrap();

        // 先卸载依赖者
        loader.unload("dependent_module", &runtime).await.unwrap();

        // 然后卸载基础模块
        let result = loader.unload("base_module", &runtime).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_loaded_modules() {
        let loader = ModuleLoader::new();
        let runtime = RustRuntimeManager::new();

        let module1 = create_test_module("module1");
        let module2 = create_test_module("module2");

        loader.load(&module1, &runtime).await.unwrap();
        loader.load(&module2, &runtime).await.unwrap();

        let loaded = loader.loaded_modules().await;
        assert_eq!(loaded.len(), 2);
        assert!(loaded.contains(&"module1".to_string()));
        assert!(loaded.contains(&"module2".to_string()));
    }

    #[tokio::test]
    async fn test_get_dependents() {
        let loader = ModuleLoader::new();
        let runtime = RustRuntimeManager::new();

        let base_module = create_test_module("base");
        loader.load(&base_module, &runtime).await.unwrap();

        let dep1 = create_test_module_with_dep("dep1", "base");
        let dep2 = create_test_module_with_dep("dep2", "base");

        loader.load(&dep1, &runtime).await.unwrap();
        loader.load(&dep2, &runtime).await.unwrap();

        let dependents = loader.get_dependents("base").await;
        assert_eq!(dependents.len(), 2);
        assert!(dependents.contains(&"dep1".to_string()));
        assert!(dependents.contains(&"dep2".to_string()));
    }
}
