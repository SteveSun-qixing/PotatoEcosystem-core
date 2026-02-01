//! 生命周期管理器
//!
//! 管理模块的生命周期，包括启动、停止、重启和健康检查。

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use super::metadata::{HealthStatus, ModuleState};
use super::runtime::RuntimeManager;
use crate::utils::{CoreError, Result};

/// 生命周期管理器
///
/// 负责管理模块的生命周期状态转换和健康检查。
pub struct LifecycleManager {
    /// 模块状态：module_id -> ModuleState
    states: Arc<RwLock<HashMap<String, ModuleState>>>,
    
    /// 健康状态缓存：module_id -> HealthStatus
    health_cache: Arc<RwLock<HashMap<String, HealthStatus>>>,
}

impl LifecycleManager {
    /// 创建新的生命周期管理器
    pub fn new() -> Self {
        info!("创建生命周期管理器");
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
            health_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 使用共享状态创建生命周期管理器
    ///
    /// 允许与 ModuleLoader 共享状态
    pub fn with_shared_states(states: Arc<RwLock<HashMap<String, ModuleState>>>) -> Self {
        info!("创建生命周期管理器（共享状态）");
        Self {
            states,
            health_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // ==================== 生命周期钩子调用 ====================

    /// 调用模块的 onInit 钩子
    ///
    /// 在模块加载后、启动前调用
    pub async fn call_init(
        &self,
        module_id: &str,
        runtime_manager: &dyn RuntimeManager,
    ) -> Result<()> {
        info!(module_id = %module_id, "调用 onInit 钩子");
        
        match runtime_manager
            .call_method(module_id, "onInit", serde_json::json!({}))
            .await
        {
            Ok(_) => {
                debug!(module_id = %module_id, "onInit 调用成功");
                Ok(())
            }
            Err(e) => {
                // 骨架实现：记录但不视为致命错误
                debug!(module_id = %module_id, error = ?e, "onInit 调用（骨架实现）");
                Ok(())
            }
        }
    }

    /// 调用模块的 onStart 钩子
    ///
    /// 在模块启动时调用
    pub async fn call_start(
        &self,
        module_id: &str,
        runtime_manager: &dyn RuntimeManager,
    ) -> Result<()> {
        info!(module_id = %module_id, "调用 onStart 钩子");
        
        match runtime_manager
            .call_method(module_id, "onStart", serde_json::json!({}))
            .await
        {
            Ok(_) => {
                debug!(module_id = %module_id, "onStart 调用成功");
                Ok(())
            }
            Err(e) => {
                debug!(module_id = %module_id, error = ?e, "onStart 调用（骨架实现）");
                Ok(())
            }
        }
    }

    /// 调用模块的 onStop 钩子
    ///
    /// 在模块停止时调用
    pub async fn call_stop(
        &self,
        module_id: &str,
        runtime_manager: &dyn RuntimeManager,
    ) -> Result<()> {
        info!(module_id = %module_id, "调用 onStop 钩子");
        
        match runtime_manager
            .call_method(module_id, "onStop", serde_json::json!({}))
            .await
        {
            Ok(_) => {
                debug!(module_id = %module_id, "onStop 调用成功");
                Ok(())
            }
            Err(e) => {
                debug!(module_id = %module_id, error = ?e, "onStop 调用（骨架实现）");
                Ok(())
            }
        }
    }

    /// 调用模块的 onCleanup 钩子
    ///
    /// 在模块卸载前调用
    pub async fn call_cleanup(
        &self,
        module_id: &str,
        runtime_manager: &dyn RuntimeManager,
    ) -> Result<()> {
        info!(module_id = %module_id, "调用 onCleanup 钩子");
        
        match runtime_manager
            .call_method(module_id, "onCleanup", serde_json::json!({}))
            .await
        {
            Ok(_) => {
                debug!(module_id = %module_id, "onCleanup 调用成功");
                Ok(())
            }
            Err(e) => {
                debug!(module_id = %module_id, error = ?e, "onCleanup 调用（骨架实现）");
                Ok(())
            }
        }
    }

    // ==================== 模块生命周期管理 ====================

    /// 启动模块
    ///
    /// 将模块从 Loaded/Stopped 状态转换为 Running 状态
    pub async fn start_module(
        &self,
        module_id: &str,
        runtime_manager: &dyn RuntimeManager,
    ) -> Result<()> {
        info!(module_id = %module_id, "启动模块");

        // 检查当前状态
        let current_state = self.get_state(module_id).await;
        match current_state {
            Some(state) if state.can_start() => {
                // 状态正常，可以启动
            }
            Some(ModuleState::Running) => {
                warn!(module_id = %module_id, "模块已在运行中");
                return Ok(());
            }
            Some(state) => {
                error!(module_id = %module_id, state = ?state, "模块状态不允许启动");
                return Err(CoreError::ModuleLoadFailed {
                    module_id: module_id.to_string(),
                    reason: format!("模块状态 {:?} 不允许启动", state),
                });
            }
            None => {
                return Err(CoreError::ModuleNotLoaded(module_id.to_string()));
            }
        }

        // 设置状态为 Starting
        self.set_state(module_id, ModuleState::Starting).await;

        // 调用 onStart 钩子
        if let Err(e) = self.call_start(module_id, runtime_manager).await {
            error!(module_id = %module_id, error = ?e, "启动钩子调用失败");
            self.set_state(module_id, ModuleState::Error).await;
            return Err(e);
        }

        // 设置状态为 Running
        self.set_state(module_id, ModuleState::Running).await;
        info!(module_id = %module_id, "模块启动成功");

        Ok(())
    }

    /// 停止模块
    ///
    /// 将模块从 Running 状态转换为 Stopped 状态
    pub async fn stop_module(
        &self,
        module_id: &str,
        runtime_manager: &dyn RuntimeManager,
    ) -> Result<()> {
        info!(module_id = %module_id, "停止模块");

        // 检查当前状态
        let current_state = self.get_state(module_id).await;
        match current_state {
            Some(state) if state.can_stop() => {
                // 状态正常，可以停止
            }
            Some(ModuleState::Stopped) => {
                warn!(module_id = %module_id, "模块已停止");
                return Ok(());
            }
            Some(state) => {
                error!(module_id = %module_id, state = ?state, "模块状态不允许停止");
                return Err(CoreError::ModuleUnloadFailed {
                    module_id: module_id.to_string(),
                    reason: format!("模块状态 {:?} 不允许停止", state),
                });
            }
            None => {
                return Err(CoreError::ModuleNotLoaded(module_id.to_string()));
            }
        }

        // 设置状态为 Stopping
        self.set_state(module_id, ModuleState::Stopping).await;

        // 调用 onStop 钩子
        if let Err(e) = self.call_stop(module_id, runtime_manager).await {
            error!(module_id = %module_id, error = ?e, "停止钩子调用失败");
            // 即使钩子调用失败，也继续停止流程
            warn!(module_id = %module_id, "忽略停止钩子错误，继续停止流程");
        }

        // 设置状态为 Stopped
        self.set_state(module_id, ModuleState::Stopped).await;
        info!(module_id = %module_id, "模块停止成功");

        Ok(())
    }

    /// 重启模块
    ///
    /// 先停止再启动模块
    pub async fn restart_module(
        &self,
        module_id: &str,
        runtime_manager: &dyn RuntimeManager,
    ) -> Result<()> {
        info!(module_id = %module_id, "重启模块");

        // 检查模块是否存在
        let current_state = self.get_state(module_id).await;
        if current_state.is_none() {
            return Err(CoreError::ModuleNotLoaded(module_id.to_string()));
        }

        // 如果模块正在运行，先停止
        if let Some(ModuleState::Running) = current_state {
            self.stop_module(module_id, runtime_manager).await?;
        }

        // 启动模块
        self.start_module(module_id, runtime_manager).await?;

        info!(module_id = %module_id, "模块重启成功");
        Ok(())
    }

    // ==================== 健康检查 ====================

    /// 对单个模块进行健康检查
    pub async fn health_check(
        &self,
        module_id: &str,
        runtime_manager: &dyn RuntimeManager,
    ) -> Result<HealthStatus> {
        debug!(module_id = %module_id, "执行健康检查");

        // 检查模块状态
        let state = self.get_state(module_id).await;
        match state {
            None => {
                return Err(CoreError::ModuleNotLoaded(module_id.to_string()));
            }
            Some(ModuleState::Error) => {
                let status = HealthStatus::unhealthy("模块处于错误状态");
                self.cache_health_status(module_id, status.clone()).await;
                return Ok(status);
            }
            Some(state) if state != ModuleState::Running => {
                let status = HealthStatus::degraded(&format!("模块未运行，当前状态: {:?}", state));
                self.cache_health_status(module_id, status.clone()).await;
                return Ok(status);
            }
            _ => {}
        }

        // 调用模块的 onHealthCheck 钩子
        match runtime_manager
            .call_method(module_id, "onHealthCheck", serde_json::json!({}))
            .await
        {
            Ok(result) => {
                // 尝试解析健康状态
                let status = serde_json::from_value::<HealthStatus>(result)
                    .unwrap_or_else(|_| HealthStatus::healthy());
                self.cache_health_status(module_id, status.clone()).await;
                Ok(status)
            }
            Err(e) => {
                // 骨架实现：如果调用失败，返回健康状态（因为这只是骨架）
                debug!(module_id = %module_id, error = ?e, "健康检查调用（骨架实现）");
                let status = HealthStatus::healthy();
                self.cache_health_status(module_id, status.clone()).await;
                Ok(status)
            }
        }
    }

    /// 检查所有模块的健康状态
    pub async fn health_check_all(
        &self,
        runtime_manager: &dyn RuntimeManager,
    ) -> HashMap<String, HealthStatus> {
        info!("执行全局健康检查");
        
        let states = self.states.read().await;
        let mut results = HashMap::new();

        for module_id in states.keys() {
            match self.health_check(module_id, runtime_manager).await {
                Ok(status) => {
                    results.insert(module_id.clone(), status);
                }
                Err(e) => {
                    error!(module_id = %module_id, error = ?e, "健康检查失败");
                    results.insert(
                        module_id.clone(),
                        HealthStatus::unhealthy(&format!("健康检查失败: {}", e)),
                    );
                }
            }
        }

        results
    }

    /// 获取缓存的健康状态
    pub async fn get_cached_health(&self, module_id: &str) -> Option<HealthStatus> {
        let cache = self.health_cache.read().await;
        cache.get(module_id).cloned()
    }

    // ==================== 状态管理 ====================

    /// 获取模块状态
    pub async fn get_state(&self, module_id: &str) -> Option<ModuleState> {
        let states = self.states.read().await;
        states.get(module_id).copied()
    }

    /// 设置模块状态
    pub async fn set_state(&self, module_id: &str, state: ModuleState) {
        let mut states = self.states.write().await;
        states.insert(module_id.to_string(), state);
        debug!(module_id = %module_id, state = ?state, "模块状态已更新");
    }

    /// 注册模块（设置初始状态）
    pub async fn register_module(&self, module_id: &str) {
        self.set_state(module_id, ModuleState::Registered).await;
        info!(module_id = %module_id, "模块已注册");
    }

    /// 获取所有模块的状态
    pub async fn get_all_states(&self) -> HashMap<String, ModuleState> {
        let states = self.states.read().await;
        states.clone()
    }

    /// 获取处于指定状态的模块列表
    pub async fn get_modules_by_state(&self, target_state: ModuleState) -> Vec<String> {
        let states = self.states.read().await;
        states
            .iter()
            .filter(|(_, &state)| state == target_state)
            .map(|(id, _)| id.clone())
            .collect()
    }

    // ==================== 内部方法 ====================

    /// 缓存健康状态
    async fn cache_health_status(&self, module_id: &str, status: HealthStatus) {
        let mut cache = self.health_cache.write().await;
        cache.insert(module_id.to_string(), status);
    }
}

impl Default for LifecycleManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::runtime::RustRuntimeManager;

    #[tokio::test]
    async fn test_lifecycle_manager_creation() {
        let manager = LifecycleManager::new();
        assert!(manager.get_all_states().await.is_empty());
    }

    #[tokio::test]
    async fn test_register_module() {
        let manager = LifecycleManager::new();
        manager.register_module("test_module").await;
        
        assert_eq!(
            manager.get_state("test_module").await,
            Some(ModuleState::Registered)
        );
    }

    #[tokio::test]
    async fn test_start_module() {
        let manager = LifecycleManager::new();
        let runtime = RustRuntimeManager::new();
        
        // 先设置为 Loaded 状态
        manager.set_state("test_module", ModuleState::Loaded).await;
        
        let result = manager.start_module("test_module", &runtime).await;
        assert!(result.is_ok());
        assert_eq!(
            manager.get_state("test_module").await,
            Some(ModuleState::Running)
        );
    }

    #[tokio::test]
    async fn test_start_module_not_loaded() {
        let manager = LifecycleManager::new();
        let runtime = RustRuntimeManager::new();
        
        let result = manager.start_module("nonexistent", &runtime).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::ModuleNotLoaded(_)));
    }

    #[tokio::test]
    async fn test_start_module_invalid_state() {
        let manager = LifecycleManager::new();
        let runtime = RustRuntimeManager::new();
        
        // 设置为 Loading 状态（不允许启动）
        manager.set_state("test_module", ModuleState::Loading).await;
        
        let result = manager.start_module("test_module", &runtime).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stop_module() {
        let manager = LifecycleManager::new();
        let runtime = RustRuntimeManager::new();
        
        // 设置为 Running 状态
        manager.set_state("test_module", ModuleState::Running).await;
        
        let result = manager.stop_module("test_module", &runtime).await;
        assert!(result.is_ok());
        assert_eq!(
            manager.get_state("test_module").await,
            Some(ModuleState::Stopped)
        );
    }

    #[tokio::test]
    async fn test_stop_module_not_running() {
        let manager = LifecycleManager::new();
        let runtime = RustRuntimeManager::new();
        
        // 设置为 Loaded 状态（不是 Running）
        manager.set_state("test_module", ModuleState::Loaded).await;
        
        let result = manager.stop_module("test_module", &runtime).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_restart_module() {
        let manager = LifecycleManager::new();
        let runtime = RustRuntimeManager::new();
        
        // 设置为 Running 状态
        manager.set_state("test_module", ModuleState::Running).await;
        
        let result = manager.restart_module("test_module", &runtime).await;
        assert!(result.is_ok());
        assert_eq!(
            manager.get_state("test_module").await,
            Some(ModuleState::Running)
        );
    }

    #[tokio::test]
    async fn test_restart_stopped_module() {
        let manager = LifecycleManager::new();
        let runtime = RustRuntimeManager::new();
        
        // 设置为 Stopped 状态
        manager.set_state("test_module", ModuleState::Stopped).await;
        
        let result = manager.restart_module("test_module", &runtime).await;
        assert!(result.is_ok());
        assert_eq!(
            manager.get_state("test_module").await,
            Some(ModuleState::Running)
        );
    }

    #[tokio::test]
    async fn test_health_check() {
        let manager = LifecycleManager::new();
        let runtime = RustRuntimeManager::new();
        
        // 设置为 Running 状态
        manager.set_state("test_module", ModuleState::Running).await;
        
        let result = manager.health_check("test_module", &runtime).await;
        assert!(result.is_ok());
        
        let status = result.unwrap();
        assert!(status.is_healthy());
    }

    #[tokio::test]
    async fn test_health_check_not_running() {
        let manager = LifecycleManager::new();
        let runtime = RustRuntimeManager::new();
        
        // 设置为 Loaded 状态
        manager.set_state("test_module", ModuleState::Loaded).await;
        
        let result = manager.health_check("test_module", &runtime).await;
        assert!(result.is_ok());
        
        let status = result.unwrap();
        // 模块未运行应该返回 degraded 状态
        assert!(!status.is_healthy());
        assert_eq!(status.status, "degraded");
    }

    #[tokio::test]
    async fn test_health_check_error_state() {
        let manager = LifecycleManager::new();
        let runtime = RustRuntimeManager::new();
        
        // 设置为 Error 状态
        manager.set_state("test_module", ModuleState::Error).await;
        
        let result = manager.health_check("test_module", &runtime).await;
        assert!(result.is_ok());
        
        let status = result.unwrap();
        assert!(!status.is_healthy());
        assert_eq!(status.status, "unhealthy");
    }

    #[tokio::test]
    async fn test_health_check_all() {
        let manager = LifecycleManager::new();
        let runtime = RustRuntimeManager::new();
        
        manager.set_state("module1", ModuleState::Running).await;
        manager.set_state("module2", ModuleState::Running).await;
        manager.set_state("module3", ModuleState::Error).await;
        
        let results = manager.health_check_all(&runtime).await;
        
        assert_eq!(results.len(), 3);
        assert!(results.get("module1").unwrap().is_healthy());
        assert!(results.get("module2").unwrap().is_healthy());
        assert!(!results.get("module3").unwrap().is_healthy());
    }

    #[tokio::test]
    async fn test_get_modules_by_state() {
        let manager = LifecycleManager::new();
        
        manager.set_state("module1", ModuleState::Running).await;
        manager.set_state("module2", ModuleState::Running).await;
        manager.set_state("module3", ModuleState::Stopped).await;
        
        let running = manager.get_modules_by_state(ModuleState::Running).await;
        assert_eq!(running.len(), 2);
        assert!(running.contains(&"module1".to_string()));
        assert!(running.contains(&"module2".to_string()));
        
        let stopped = manager.get_modules_by_state(ModuleState::Stopped).await;
        assert_eq!(stopped.len(), 1);
        assert!(stopped.contains(&"module3".to_string()));
    }

    #[tokio::test]
    async fn test_lifecycle_hooks() {
        let manager = LifecycleManager::new();
        let runtime = RustRuntimeManager::new();
        
        // 测试所有钩子调用（骨架实现应该都成功）
        assert!(manager.call_init("test", &runtime).await.is_ok());
        assert!(manager.call_start("test", &runtime).await.is_ok());
        assert!(manager.call_stop("test", &runtime).await.is_ok());
        assert!(manager.call_cleanup("test", &runtime).await.is_ok());
    }

    #[tokio::test]
    async fn test_cached_health_status() {
        let manager = LifecycleManager::new();
        let runtime = RustRuntimeManager::new();
        
        manager.set_state("test_module", ModuleState::Running).await;
        
        // 首先应该没有缓存
        assert!(manager.get_cached_health("test_module").await.is_none());
        
        // 执行健康检查后应该有缓存
        manager.health_check("test_module", &runtime).await.unwrap();
        
        let cached = manager.get_cached_health("test_module").await;
        assert!(cached.is_some());
        assert!(cached.unwrap().is_healthy());
    }

    #[tokio::test]
    async fn test_shared_states() {
        let states = Arc::new(RwLock::new(HashMap::new()));
        let manager = LifecycleManager::with_shared_states(states.clone());
        
        manager.set_state("test", ModuleState::Running).await;
        
        // 验证共享状态被更新
        let states_guard = states.read().await;
        assert_eq!(states_guard.get("test"), Some(&ModuleState::Running));
    }
}
