//! 模块注册表
//!
//! 管理所有已注册的模块，提供模块的注册、查询、状态管理等功能。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::RwLock;

use crate::module::metadata::{ModuleInfo, ModuleMetadata, ModuleState};
use crate::module::parser::ModuleParser;
use crate::utils::{CoreError, Result};

/// 默认的模块配置文件名
const MODULE_CONFIG_FILENAME: &str = "module.yaml";

/// 模块注册表
///
/// 管理所有已注册的模块，包括：
/// - 模块信息存储
/// - 模块状态管理
/// - 模块目录扫描
/// - 模块注册/取消注册
#[derive(Debug)]
pub struct ModuleRegistry {
    /// 已注册的模块：module_id -> ModuleInfo
    modules: Arc<RwLock<HashMap<String, ModuleInfo>>>,

    /// 模块状态：module_id -> ModuleState（用于快速访问状态）
    states: Arc<RwLock<HashMap<String, ModuleState>>>,

    /// 模块目录路径列表
    module_dirs: Vec<PathBuf>,
}

impl ModuleRegistry {
    /// 创建新的模块注册表
    ///
    /// # Arguments
    ///
    /// * `module_dirs` - 模块目录路径列表
    ///
    /// # Returns
    ///
    /// 新的 `ModuleRegistry` 实例
    pub fn new(module_dirs: Vec<PathBuf>) -> Self {
        Self {
            modules: Arc::new(RwLock::new(HashMap::new())),
            states: Arc::new(RwLock::new(HashMap::new())),
            module_dirs,
        }
    }

    /// 创建带有默认配置的注册表
    pub fn with_defaults() -> Self {
        Self::new(vec![PathBuf::from("./modules")])
    }

    /// 扫描所有模块目录，发现并注册模块
    ///
    /// # Returns
    ///
    /// 成功注册的模块 ID 列表
    ///
    /// # Notes
    ///
    /// - 扫描每个模块目录中的子目录
    /// - 查找包含 module.yaml 的目录
    /// - 自动解析并注册找到的模块
    /// - 跳过解析失败的模块（记录警告日志）
    pub async fn scan(&self) -> Result<Vec<String>> {
        let mut registered_ids = Vec::new();

        for dir in &self.module_dirs {
            if !dir.exists() {
                tracing::debug!("模块目录不存在，跳过: {:?}", dir);
                continue;
            }

            let entries = match tokio::fs::read_dir(dir).await {
                Ok(entries) => entries,
                Err(e) => {
                    tracing::warn!("无法读取模块目录 {:?}: {}", dir, e);
                    continue;
                }
            };

            let mut entries = entries;
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();

                // 只处理目录
                if !path.is_dir() {
                    continue;
                }

                // 检查是否存在 module.yaml
                let config_path = path.join(MODULE_CONFIG_FILENAME);
                if !config_path.exists() {
                    tracing::trace!("目录 {:?} 中未找到 module.yaml，跳过", path);
                    continue;
                }

                // 尝试注册模块
                match self.register(&path).await {
                    Ok(module_id) => {
                        tracing::info!("成功注册模块: {} (路径: {:?})", module_id, path);
                        registered_ids.push(module_id);
                    }
                    Err(e) => {
                        tracing::warn!("注册模块失败 {:?}: {}", path, e);
                    }
                }
            }
        }

        Ok(registered_ids)
    }

    /// 注册单个模块
    ///
    /// # Arguments
    ///
    /// * `module_path` - 模块根目录路径（包含 module.yaml）
    ///
    /// # Returns
    ///
    /// 注册成功的模块 ID
    ///
    /// # Errors
    ///
    /// - 模块配置文件不存在
    /// - 配置文件解析失败
    /// - 模块已存在（ID 冲突）
    pub async fn register(&self, module_path: &Path) -> Result<String> {
        let config_path = module_path.join(MODULE_CONFIG_FILENAME);

        // 解析模块元数据
        let metadata = ModuleParser::parse_file(&config_path).await?;
        let module_id = metadata.id.clone();

        // 检查模块是否已存在
        {
            let modules = self.modules.read().await;
            if modules.contains_key(&module_id) {
                return Err(CoreError::ModuleAlreadyLoaded(module_id));
            }
        }

        // 创建模块信息
        let module_info = ModuleInfo::new(metadata, module_path.to_path_buf());

        // 注册模块
        {
            let mut modules = self.modules.write().await;
            let mut states = self.states.write().await;

            modules.insert(module_id.clone(), module_info);
            states.insert(module_id.clone(), ModuleState::Registered);
        }

        tracing::debug!("模块已注册: {}", module_id);
        Ok(module_id)
    }

    /// 使用元数据直接注册模块
    ///
    /// 用于在没有物理文件的情况下注册模块（如内置模块或测试）
    ///
    /// # Arguments
    ///
    /// * `metadata` - 模块元数据
    /// * `path` - 模块路径
    ///
    /// # Returns
    ///
    /// 注册成功的模块 ID
    pub async fn register_with_metadata(
        &self,
        metadata: ModuleMetadata,
        path: PathBuf,
    ) -> Result<String> {
        let module_id = metadata.id.clone();

        // 验证元数据
        ModuleParser::validate(&metadata)?;

        // 检查模块是否已存在
        {
            let modules = self.modules.read().await;
            if modules.contains_key(&module_id) {
                return Err(CoreError::ModuleAlreadyLoaded(module_id));
            }
        }

        // 创建模块信息
        let module_info = ModuleInfo::new(metadata, path);

        // 注册模块
        {
            let mut modules = self.modules.write().await;
            let mut states = self.states.write().await;

            modules.insert(module_id.clone(), module_info);
            states.insert(module_id.clone(), ModuleState::Registered);
        }

        Ok(module_id)
    }

    /// 取消注册模块
    ///
    /// # Arguments
    ///
    /// * `module_id` - 要取消注册的模块 ID
    ///
    /// # Errors
    ///
    /// - 模块不存在
    /// - 模块正在运行中（需要先停止）
    pub async fn unregister(&self, module_id: &str) -> Result<()> {
        // 检查模块状态
        {
            let states = self.states.read().await;
            if let Some(state) = states.get(module_id) {
                if *state == ModuleState::Running {
                    return Err(CoreError::ModuleUnloadFailed {
                        module_id: module_id.to_string(),
                        reason: "模块正在运行中，请先停止模块".to_string(),
                    });
                }
            } else {
                return Err(CoreError::ModuleNotFound(module_id.to_string()));
            }
        }

        // 移除模块
        {
            let mut modules = self.modules.write().await;
            let mut states = self.states.write().await;

            modules.remove(module_id);
            states.remove(module_id);
        }

        tracing::debug!("模块已取消注册: {}", module_id);
        Ok(())
    }

    /// 获取模块信息
    ///
    /// # Arguments
    ///
    /// * `module_id` - 模块 ID
    ///
    /// # Returns
    ///
    /// 模块信息（如果存在）
    pub async fn get_module(&self, module_id: &str) -> Option<ModuleInfo> {
        let modules = self.modules.read().await;
        modules.get(module_id).cloned()
    }

    /// 获取所有已注册的模块列表
    ///
    /// # Returns
    ///
    /// 所有模块信息的列表
    pub async fn list_modules(&self) -> Vec<ModuleInfo> {
        let modules = self.modules.read().await;
        modules.values().cloned().collect()
    }

    /// 获取模块状态
    ///
    /// # Arguments
    ///
    /// * `module_id` - 模块 ID
    ///
    /// # Returns
    ///
    /// 模块状态（如果存在）
    pub async fn get_state(&self, module_id: &str) -> Option<ModuleState> {
        let states = self.states.read().await;
        states.get(module_id).copied()
    }

    /// 设置模块状态
    ///
    /// # Arguments
    ///
    /// * `module_id` - 模块 ID
    /// * `state` - 新状态
    ///
    /// # Errors
    ///
    /// - 模块不存在
    pub async fn set_state(&self, module_id: &str, state: ModuleState) -> Result<()> {
        let mut states = self.states.write().await;
        let mut modules = self.modules.write().await;

        if !states.contains_key(module_id) {
            return Err(CoreError::ModuleNotFound(module_id.to_string()));
        }

        // 更新状态
        states.insert(module_id.to_string(), state);

        // 同步更新 ModuleInfo 中的状态
        if let Some(info) = modules.get_mut(module_id) {
            info.state = state;

            // 更新时间戳
            match state {
                ModuleState::Loaded => {
                    info.loaded_at = Some(Utc::now());
                }
                ModuleState::Running => {
                    info.started_at = Some(Utc::now());
                }
                _ => {}
            }
        }

        tracing::trace!("模块 {} 状态更新为: {:?}", module_id, state);
        Ok(())
    }

    /// 设置模块错误状态
    ///
    /// # Arguments
    ///
    /// * `module_id` - 模块 ID
    /// * `error` - 错误信息
    pub async fn set_error(&self, module_id: &str, error: &str) -> Result<()> {
        let mut states = self.states.write().await;
        let mut modules = self.modules.write().await;

        if !states.contains_key(module_id) {
            return Err(CoreError::ModuleNotFound(module_id.to_string()));
        }

        states.insert(module_id.to_string(), ModuleState::Error);

        if let Some(info) = modules.get_mut(module_id) {
            info.state = ModuleState::Error;
            info.last_error = Some(error.to_string());
        }

        Ok(())
    }

    /// 检查模块是否存在
    ///
    /// # Arguments
    ///
    /// * `module_id` - 模块 ID
    ///
    /// # Returns
    ///
    /// 模块是否存在
    pub async fn exists(&self, module_id: &str) -> bool {
        let modules = self.modules.read().await;
        modules.contains_key(module_id)
    }

    /// 按条件查找模块
    ///
    /// # Arguments
    ///
    /// * `predicate` - 过滤条件函数
    ///
    /// # Returns
    ///
    /// 满足条件的模块列表
    pub async fn find_modules<F>(&self, predicate: F) -> Vec<ModuleInfo>
    where
        F: Fn(&ModuleInfo) -> bool,
    {
        let modules = self.modules.read().await;
        modules.values().filter(|m| predicate(m)).cloned().collect()
    }

    /// 按状态查找模块
    ///
    /// # Arguments
    ///
    /// * `state` - 要查找的状态
    ///
    /// # Returns
    ///
    /// 处于指定状态的模块列表
    pub async fn find_by_state(&self, state: ModuleState) -> Vec<ModuleInfo> {
        self.find_modules(|m| m.state == state).await
    }

    /// 获取正在运行的模块列表
    pub async fn get_running_modules(&self) -> Vec<ModuleInfo> {
        self.find_by_state(ModuleState::Running).await
    }

    /// 获取已注册模块数量
    pub async fn count(&self) -> usize {
        let modules = self.modules.read().await;
        modules.len()
    }

    /// 清空所有注册的模块
    ///
    /// # Warning
    ///
    /// 此操作会移除所有模块，谨慎使用
    pub async fn clear(&self) {
        let mut modules = self.modules.write().await;
        let mut states = self.states.write().await;

        modules.clear();
        states.clear();

        tracing::warn!("已清空所有注册的模块");
    }

    /// 获取模块目录列表
    pub fn module_dirs(&self) -> &[PathBuf] {
        &self.module_dirs
    }

    /// 添加模块目录
    pub fn add_module_dir(&mut self, dir: PathBuf) {
        if !self.module_dirs.contains(&dir) {
            self.module_dirs.push(dir);
        }
    }

    /// 获取模块元数据
    pub async fn get_metadata(&self, module_id: &str) -> Option<ModuleMetadata> {
        let modules = self.modules.read().await;
        modules.get(module_id).map(|m| m.metadata.clone())
    }

    /// 获取所有模块 ID
    pub async fn get_all_module_ids(&self) -> Vec<String> {
        let modules = self.modules.read().await;
        modules.keys().cloned().collect()
    }

    /// 批量获取模块状态
    pub async fn get_states(&self, module_ids: &[String]) -> HashMap<String, ModuleState> {
        let states = self.states.read().await;
        module_ids
            .iter()
            .filter_map(|id| states.get(id).map(|s| (id.clone(), *s)))
            .collect()
    }
}

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl Clone for ModuleRegistry {
    fn clone(&self) -> Self {
        Self {
            modules: Arc::clone(&self.modules),
            states: Arc::clone(&self.states),
            module_dirs: self.module_dirs.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::metadata::{EntryPoint, ModuleType, Runtime};
    use tempfile::TempDir;

    /// 创建测试用的元数据
    fn create_test_metadata(id: &str) -> ModuleMetadata {
        let mut metadata = ModuleMetadata::new(id, format!("Test Module {}", id), "1.0.0");
        metadata.entry = EntryPoint {
            runtime: Runtime::Rust,
            path: "lib.rs".to_string(),
            function: None,
        };
        metadata
    }

    /// 创建测试模块目录
    async fn create_test_module_dir(
        temp_dir: &TempDir,
        module_id: &str,
    ) -> PathBuf {
        let module_dir = temp_dir.path().join(module_id);
        tokio::fs::create_dir_all(&module_dir).await.unwrap();

        let yaml_content = format!(
            r#"
id: "{}"
name: "Test Module {}"
version: "1.0.0"
entry:
  runtime: rust
  path: "lib.rs"
"#,
            module_id, module_id
        );

        let config_path = module_dir.join("module.yaml");
        tokio::fs::write(&config_path, yaml_content).await.unwrap();

        module_dir
    }

    #[tokio::test]
    async fn test_registry_creation() {
        let registry = ModuleRegistry::new(vec![PathBuf::from("./modules")]);
        assert_eq!(registry.module_dirs().len(), 1);
        assert_eq!(registry.count().await, 0);
    }

    #[tokio::test]
    async fn test_registry_with_defaults() {
        let registry = ModuleRegistry::with_defaults();
        assert!(!registry.module_dirs().is_empty());
    }

    #[tokio::test]
    async fn test_register_with_metadata() {
        let registry = ModuleRegistry::new(vec![]);
        let metadata = create_test_metadata("test-module");
        let path = PathBuf::from("/test/path");

        let result = registry.register_with_metadata(metadata, path).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-module");

        // 验证模块已注册
        assert!(registry.exists("test-module").await);
        assert_eq!(registry.count().await, 1);
    }

    #[tokio::test]
    async fn test_register_duplicate_module() {
        let registry = ModuleRegistry::new(vec![]);
        let metadata = create_test_metadata("test-module");
        let path = PathBuf::from("/test/path");

        // 第一次注册成功
        registry
            .register_with_metadata(metadata.clone(), path.clone())
            .await
            .unwrap();

        // 第二次注册应该失败
        let result = registry.register_with_metadata(metadata, path).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CoreError::ModuleAlreadyLoaded(_)
        ));
    }

    #[tokio::test]
    async fn test_unregister_module() {
        let registry = ModuleRegistry::new(vec![]);
        let metadata = create_test_metadata("test-module");
        let path = PathBuf::from("/test/path");

        registry
            .register_with_metadata(metadata, path)
            .await
            .unwrap();

        // 取消注册
        let result = registry.unregister("test-module").await;
        assert!(result.is_ok());
        assert!(!registry.exists("test-module").await);
    }

    #[tokio::test]
    async fn test_unregister_nonexistent_module() {
        let registry = ModuleRegistry::new(vec![]);

        let result = registry.unregister("nonexistent").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CoreError::ModuleNotFound(_)
        ));
    }

    #[tokio::test]
    async fn test_unregister_running_module() {
        let registry = ModuleRegistry::new(vec![]);
        let metadata = create_test_metadata("test-module");
        let path = PathBuf::from("/test/path");

        registry
            .register_with_metadata(metadata, path)
            .await
            .unwrap();

        // 设置为运行状态
        registry
            .set_state("test-module", ModuleState::Running)
            .await
            .unwrap();

        // 取消注册应该失败
        let result = registry.unregister("test-module").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_module() {
        let registry = ModuleRegistry::new(vec![]);
        let metadata = create_test_metadata("test-module");
        let path = PathBuf::from("/test/path");

        registry
            .register_with_metadata(metadata, path)
            .await
            .unwrap();

        let module = registry.get_module("test-module").await;
        assert!(module.is_some());
        assert_eq!(module.unwrap().id(), "test-module");

        let nonexistent = registry.get_module("nonexistent").await;
        assert!(nonexistent.is_none());
    }

    #[tokio::test]
    async fn test_list_modules() {
        let registry = ModuleRegistry::new(vec![]);

        for i in 1..=3 {
            let metadata = create_test_metadata(&format!("module-{}", i));
            let path = PathBuf::from(format!("/test/path/{}", i));
            registry
                .register_with_metadata(metadata, path)
                .await
                .unwrap();
        }

        let modules = registry.list_modules().await;
        assert_eq!(modules.len(), 3);
    }

    #[tokio::test]
    async fn test_get_and_set_state() {
        let registry = ModuleRegistry::new(vec![]);
        let metadata = create_test_metadata("test-module");
        let path = PathBuf::from("/test/path");

        registry
            .register_with_metadata(metadata, path)
            .await
            .unwrap();

        // 初始状态应该是 Registered
        let state = registry.get_state("test-module").await;
        assert_eq!(state, Some(ModuleState::Registered));

        // 设置新状态
        registry
            .set_state("test-module", ModuleState::Loading)
            .await
            .unwrap();
        let state = registry.get_state("test-module").await;
        assert_eq!(state, Some(ModuleState::Loading));

        // 设置不存在的模块状态应该失败
        let result = registry.set_state("nonexistent", ModuleState::Running).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_set_error() {
        let registry = ModuleRegistry::new(vec![]);
        let metadata = create_test_metadata("test-module");
        let path = PathBuf::from("/test/path");

        registry
            .register_with_metadata(metadata, path)
            .await
            .unwrap();

        registry
            .set_error("test-module", "Test error message")
            .await
            .unwrap();

        let state = registry.get_state("test-module").await;
        assert_eq!(state, Some(ModuleState::Error));

        let module = registry.get_module("test-module").await.unwrap();
        assert_eq!(module.last_error, Some("Test error message".to_string()));
    }

    #[tokio::test]
    async fn test_find_modules() {
        let registry = ModuleRegistry::new(vec![]);

        // 注册几个不同类型的模块
        for (id, module_type) in [
            ("plugin-1", ModuleType::Plugin),
            ("plugin-2", ModuleType::Plugin),
            ("service-1", ModuleType::Service),
        ] {
            let mut metadata = create_test_metadata(id);
            metadata.module_type = module_type;
            let path = PathBuf::from(format!("/test/path/{}", id));
            registry
                .register_with_metadata(metadata, path)
                .await
                .unwrap();
        }

        // 查找所有插件
        let plugins = registry
            .find_modules(|m| m.metadata.module_type == ModuleType::Plugin)
            .await;
        assert_eq!(plugins.len(), 2);

        // 查找所有服务
        let services = registry
            .find_modules(|m| m.metadata.module_type == ModuleType::Service)
            .await;
        assert_eq!(services.len(), 1);
    }

    #[tokio::test]
    async fn test_find_by_state() {
        let registry = ModuleRegistry::new(vec![]);

        for i in 1..=3 {
            let metadata = create_test_metadata(&format!("module-{}", i));
            let path = PathBuf::from(format!("/test/path/{}", i));
            registry
                .register_with_metadata(metadata, path)
                .await
                .unwrap();
        }

        // 设置不同状态
        registry
            .set_state("module-1", ModuleState::Running)
            .await
            .unwrap();
        registry
            .set_state("module-2", ModuleState::Running)
            .await
            .unwrap();

        let running = registry.find_by_state(ModuleState::Running).await;
        assert_eq!(running.len(), 2);

        let registered = registry.find_by_state(ModuleState::Registered).await;
        assert_eq!(registered.len(), 1);
    }

    #[tokio::test]
    async fn test_get_running_modules() {
        let registry = ModuleRegistry::new(vec![]);

        let metadata = create_test_metadata("test-module");
        let path = PathBuf::from("/test/path");
        registry
            .register_with_metadata(metadata, path)
            .await
            .unwrap();

        registry
            .set_state("test-module", ModuleState::Running)
            .await
            .unwrap();

        let running = registry.get_running_modules().await;
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id(), "test-module");
    }

    #[tokio::test]
    async fn test_clear() {
        let registry = ModuleRegistry::new(vec![]);

        for i in 1..=3 {
            let metadata = create_test_metadata(&format!("module-{}", i));
            let path = PathBuf::from(format!("/test/path/{}", i));
            registry
                .register_with_metadata(metadata, path)
                .await
                .unwrap();
        }

        assert_eq!(registry.count().await, 3);

        registry.clear().await;

        assert_eq!(registry.count().await, 0);
    }

    #[tokio::test]
    async fn test_scan_module_directory() {
        let temp_dir = TempDir::new().unwrap();

        // 创建测试模块目录
        create_test_module_dir(&temp_dir, "module-a").await;
        create_test_module_dir(&temp_dir, "module-b").await;

        // 创建一个无效的目录（没有 module.yaml）
        let invalid_dir = temp_dir.path().join("invalid-dir");
        tokio::fs::create_dir_all(&invalid_dir).await.unwrap();

        let registry = ModuleRegistry::new(vec![temp_dir.path().to_path_buf()]);
        let registered = registry.scan().await.unwrap();

        assert_eq!(registered.len(), 2);
        assert!(registered.contains(&"module-a".to_string()));
        assert!(registered.contains(&"module-b".to_string()));
    }

    #[tokio::test]
    async fn test_scan_nonexistent_directory() {
        let registry = ModuleRegistry::new(vec![PathBuf::from("/nonexistent/path")]);
        let registered = registry.scan().await.unwrap();

        // 不存在的目录应该被跳过，不报错
        assert!(registered.is_empty());
    }

    #[tokio::test]
    async fn test_register_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let module_dir = create_test_module_dir(&temp_dir, "file-module").await;

        let registry = ModuleRegistry::new(vec![]);
        let result = registry.register(&module_dir).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "file-module");
        assert!(registry.exists("file-module").await);
    }

    #[tokio::test]
    async fn test_get_metadata() {
        let registry = ModuleRegistry::new(vec![]);
        let metadata = create_test_metadata("test-module");
        let path = PathBuf::from("/test/path");

        registry
            .register_with_metadata(metadata, path)
            .await
            .unwrap();

        let retrieved = registry.get_metadata("test-module").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "test-module");

        let nonexistent = registry.get_metadata("nonexistent").await;
        assert!(nonexistent.is_none());
    }

    #[tokio::test]
    async fn test_get_all_module_ids() {
        let registry = ModuleRegistry::new(vec![]);

        for i in 1..=3 {
            let metadata = create_test_metadata(&format!("module-{}", i));
            let path = PathBuf::from(format!("/test/path/{}", i));
            registry
                .register_with_metadata(metadata, path)
                .await
                .unwrap();
        }

        let ids = registry.get_all_module_ids().await;
        assert_eq!(ids.len(), 3);
    }

    #[tokio::test]
    async fn test_get_states_batch() {
        let registry = ModuleRegistry::new(vec![]);

        for i in 1..=3 {
            let metadata = create_test_metadata(&format!("module-{}", i));
            let path = PathBuf::from(format!("/test/path/{}", i));
            registry
                .register_with_metadata(metadata, path)
                .await
                .unwrap();
        }

        registry
            .set_state("module-1", ModuleState::Running)
            .await
            .unwrap();

        let ids = vec![
            "module-1".to_string(),
            "module-2".to_string(),
            "nonexistent".to_string(),
        ];
        let states = registry.get_states(&ids).await;

        assert_eq!(states.len(), 2); // nonexistent 不应该在结果中
        assert_eq!(states.get("module-1"), Some(&ModuleState::Running));
        assert_eq!(states.get("module-2"), Some(&ModuleState::Registered));
    }

    #[tokio::test]
    async fn test_add_module_dir() {
        let mut registry = ModuleRegistry::new(vec![PathBuf::from("./modules")]);
        assert_eq!(registry.module_dirs().len(), 1);

        registry.add_module_dir(PathBuf::from("./plugins"));
        assert_eq!(registry.module_dirs().len(), 2);

        // 添加重复的目录不应该增加
        registry.add_module_dir(PathBuf::from("./modules"));
        assert_eq!(registry.module_dirs().len(), 2);
    }

    #[tokio::test]
    async fn test_state_updates_timestamps() {
        let registry = ModuleRegistry::new(vec![]);
        let metadata = create_test_metadata("test-module");
        let path = PathBuf::from("/test/path");

        registry
            .register_with_metadata(metadata, path)
            .await
            .unwrap();

        // 初始时没有时间戳
        let module = registry.get_module("test-module").await.unwrap();
        assert!(module.loaded_at.is_none());
        assert!(module.started_at.is_none());

        // 设置 Loaded 状态应该更新 loaded_at
        registry
            .set_state("test-module", ModuleState::Loaded)
            .await
            .unwrap();
        let module = registry.get_module("test-module").await.unwrap();
        assert!(module.loaded_at.is_some());

        // 设置 Running 状态应该更新 started_at
        registry
            .set_state("test-module", ModuleState::Running)
            .await
            .unwrap();
        let module = registry.get_module("test-module").await.unwrap();
        assert!(module.started_at.is_some());
    }

    #[tokio::test]
    async fn test_registry_clone() {
        let registry = ModuleRegistry::new(vec![]);
        let metadata = create_test_metadata("test-module");
        let path = PathBuf::from("/test/path");

        registry
            .register_with_metadata(metadata, path)
            .await
            .unwrap();

        // 克隆注册表
        let cloned = registry.clone();

        // 两个注册表应该共享同一份数据
        assert!(cloned.exists("test-module").await);

        // 在原始注册表中修改状态
        registry
            .set_state("test-module", ModuleState::Running)
            .await
            .unwrap();

        // 克隆的注册表也应该看到变化
        let state = cloned.get_state("test-module").await;
        assert_eq!(state, Some(ModuleState::Running));
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        use tokio::task;

        let registry = Arc::new(ModuleRegistry::new(vec![]));

        // 并发注册多个模块
        let mut handles = vec![];
        for i in 0..10 {
            let reg = Arc::clone(&registry);
            let handle = task::spawn(async move {
                let metadata = create_test_metadata(&format!("concurrent-module-{}", i));
                let path = PathBuf::from(format!("/test/path/{}", i));
                reg.register_with_metadata(metadata, path).await
            });
            handles.push(handle);
        }

        // 等待所有任务完成
        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        // 验证所有模块都已注册
        assert_eq!(registry.count().await, 10);
    }
}
