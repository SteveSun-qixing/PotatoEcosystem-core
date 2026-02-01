//! 模块依赖管理
//!
//! 本模块提供模块依赖关系的图结构和解析器。
//!
//! # 主要组件
//!
//! - [`DependencyGraph`] - 依赖关系图，用于管理模块间的依赖关系
//! - [`DependencyResolver`] - 依赖解析器，用于解析模块的完整依赖链
//!
//! # 示例
//!
//! ```rust
//! use chips_core::module::dependency::DependencyGraph;
//!
//! let mut graph = DependencyGraph::new();
//! graph.add_module("module_a");
//! graph.add_module("module_b");
//! graph.add_dependency("module_a", "module_b");
//!
//! assert_eq!(graph.get_dependencies("module_a"), vec!["module_b".to_string()]);
//! assert!(!graph.has_cycle());
//! ```

use std::collections::{HashMap, HashSet, VecDeque};

use semver::{Version, VersionReq};

use crate::module::metadata::{Dependency, ModuleInfo};
use crate::utils::{CoreError, Result};

/// 模块依赖关系图
///
/// 用于表示和管理模块之间的依赖关系。
/// 支持循环依赖检测和拓扑排序。
#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    /// 正向边：模块 ID -> 该模块依赖的模块列表
    edges: HashMap<String, Vec<String>>,
    /// 反向边：模块 ID -> 依赖该模块的模块列表
    reverse_edges: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    /// 创建一个空的依赖图
    ///
    /// # 示例
    ///
    /// ```rust
    /// use chips_core::module::dependency::DependencyGraph;
    ///
    /// let graph = DependencyGraph::new();
    /// assert!(graph.is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            edges: HashMap::new(),
            reverse_edges: HashMap::new(),
        }
    }

    /// 添加模块节点到图中
    ///
    /// 如果模块已存在，则不会重复添加。
    ///
    /// # 参数
    ///
    /// * `module_id` - 模块唯一标识符
    ///
    /// # 示例
    ///
    /// ```rust
    /// use chips_core::module::dependency::DependencyGraph;
    ///
    /// let mut graph = DependencyGraph::new();
    /// graph.add_module("my_module");
    /// assert!(graph.contains_module("my_module"));
    /// ```
    pub fn add_module(&mut self, module_id: &str) {
        if !self.edges.contains_key(module_id) {
            self.edges.insert(module_id.to_string(), Vec::new());
        }
        if !self.reverse_edges.contains_key(module_id) {
            self.reverse_edges.insert(module_id.to_string(), Vec::new());
        }
    }

    /// 添加依赖关系
    ///
    /// 表示 `module_id` 依赖于 `dependency_id`。
    /// 如果模块不存在，会自动添加。
    ///
    /// # 参数
    ///
    /// * `module_id` - 依赖方模块 ID
    /// * `dependency_id` - 被依赖方模块 ID
    ///
    /// # 示例
    ///
    /// ```rust
    /// use chips_core::module::dependency::DependencyGraph;
    ///
    /// let mut graph = DependencyGraph::new();
    /// graph.add_dependency("module_a", "module_b");
    ///
    /// // module_a 依赖 module_b
    /// assert!(graph.get_dependencies("module_a").contains(&"module_b".to_string()));
    /// // module_b 被 module_a 依赖
    /// assert!(graph.get_dependents("module_b").contains(&"module_a".to_string()));
    /// ```
    pub fn add_dependency(&mut self, module_id: &str, dependency_id: &str) {
        // 确保两个模块都存在
        self.add_module(module_id);
        self.add_module(dependency_id);

        // 添加正向边（避免重复）
        let deps = self.edges.get_mut(module_id).unwrap();
        if !deps.contains(&dependency_id.to_string()) {
            deps.push(dependency_id.to_string());
        }

        // 添加反向边（避免重复）
        let rev_deps = self.reverse_edges.get_mut(dependency_id).unwrap();
        if !rev_deps.contains(&module_id.to_string()) {
            rev_deps.push(module_id.to_string());
        }
    }

    /// 移除依赖关系
    ///
    /// # 参数
    ///
    /// * `module_id` - 依赖方模块 ID
    /// * `dependency_id` - 被依赖方模块 ID
    pub fn remove_dependency(&mut self, module_id: &str, dependency_id: &str) {
        if let Some(deps) = self.edges.get_mut(module_id) {
            deps.retain(|d| d != dependency_id);
        }
        if let Some(rev_deps) = self.reverse_edges.get_mut(dependency_id) {
            rev_deps.retain(|d| d != module_id);
        }
    }

    /// 移除模块及其所有依赖关系
    ///
    /// # 参数
    ///
    /// * `module_id` - 要移除的模块 ID
    ///
    /// # 示例
    ///
    /// ```rust
    /// use chips_core::module::dependency::DependencyGraph;
    ///
    /// let mut graph = DependencyGraph::new();
    /// graph.add_dependency("a", "b");
    /// graph.add_dependency("c", "b");
    ///
    /// graph.remove_module("b");
    ///
    /// assert!(!graph.contains_module("b"));
    /// assert!(graph.get_dependencies("a").is_empty());
    /// assert!(graph.get_dependencies("c").is_empty());
    /// ```
    pub fn remove_module(&mut self, module_id: &str) {
        // 获取该模块的所有依赖，并移除反向边
        if let Some(deps) = self.edges.remove(module_id) {
            for dep in deps {
                if let Some(rev_deps) = self.reverse_edges.get_mut(&dep) {
                    rev_deps.retain(|d| d != module_id);
                }
            }
        }

        // 获取依赖该模块的所有模块，并移除正向边
        if let Some(dependents) = self.reverse_edges.remove(module_id) {
            for dependent in dependents {
                if let Some(deps) = self.edges.get_mut(&dependent) {
                    deps.retain(|d| d != module_id);
                }
            }
        }
    }

    /// 获取模块的直接依赖列表
    ///
    /// # 参数
    ///
    /// * `module_id` - 模块 ID
    ///
    /// # 返回
    ///
    /// 该模块直接依赖的模块 ID 列表
    pub fn get_dependencies(&self, module_id: &str) -> Vec<String> {
        self.edges.get(module_id).cloned().unwrap_or_default()
    }

    /// 获取依赖该模块的模块列表
    ///
    /// # 参数
    ///
    /// * `module_id` - 模块 ID
    ///
    /// # 返回
    ///
    /// 依赖该模块的模块 ID 列表
    pub fn get_dependents(&self, module_id: &str) -> Vec<String> {
        self.reverse_edges.get(module_id).cloned().unwrap_or_default()
    }

    /// 获取模块的所有传递依赖（递归）
    ///
    /// # 参数
    ///
    /// * `module_id` - 模块 ID
    ///
    /// # 返回
    ///
    /// 该模块的所有直接和间接依赖
    pub fn get_all_dependencies(&self, module_id: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        self.collect_dependencies(module_id, &mut result, &mut visited);
        result
    }

    /// 递归收集依赖
    fn collect_dependencies(
        &self,
        module_id: &str,
        result: &mut Vec<String>,
        visited: &mut HashSet<String>,
    ) {
        if visited.contains(module_id) {
            return;
        }
        visited.insert(module_id.to_string());

        if let Some(deps) = self.edges.get(module_id) {
            for dep in deps {
                self.collect_dependencies(dep, result, visited);
                if !result.contains(dep) {
                    result.push(dep.clone());
                }
            }
        }
    }

    /// 检测是否存在循环依赖
    ///
    /// 使用深度优先搜索 (DFS) 检测图中是否存在环。
    ///
    /// # 返回
    ///
    /// 如果存在循环依赖返回 `true`，否则返回 `false`
    ///
    /// # 示例
    ///
    /// ```rust
    /// use chips_core::module::dependency::DependencyGraph;
    ///
    /// let mut graph = DependencyGraph::new();
    /// graph.add_dependency("a", "b");
    /// graph.add_dependency("b", "c");
    /// assert!(!graph.has_cycle());
    ///
    /// // 添加循环依赖
    /// graph.add_dependency("c", "a");
    /// assert!(graph.has_cycle());
    /// ```
    pub fn has_cycle(&self) -> bool {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for node in self.edges.keys() {
            if self.has_cycle_util(node, &mut visited, &mut rec_stack) {
                return true;
            }
        }

        false
    }

    /// 检测循环依赖的辅助函数（DFS）
    fn has_cycle_util(
        &self,
        node: &str,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
    ) -> bool {
        // 如果当前节点在递归栈中，说明存在环
        if rec_stack.contains(node) {
            return true;
        }

        // 如果已经访问过且不在递归栈中，说明这条路径没有环
        if visited.contains(node) {
            return false;
        }

        // 标记为已访问并加入递归栈
        visited.insert(node.to_string());
        rec_stack.insert(node.to_string());

        // 检查所有邻居
        if let Some(neighbors) = self.edges.get(node) {
            for neighbor in neighbors {
                if self.has_cycle_util(neighbor, visited, rec_stack) {
                    return true;
                }
            }
        }

        // 回溯时从递归栈中移除
        rec_stack.remove(node);
        false
    }

    /// 查找循环依赖路径
    ///
    /// 如果存在循环依赖，返回参与循环的模块路径。
    ///
    /// # 返回
    ///
    /// 如果存在循环，返回循环路径；否则返回 `None`
    pub fn find_cycle(&self) -> Option<Vec<String>> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        for node in self.edges.keys() {
            if let Some(cycle) = self.find_cycle_util(node, &mut visited, &mut rec_stack, &mut path)
            {
                return Some(cycle);
            }
        }

        None
    }

    /// 查找循环的辅助函数
    fn find_cycle_util(
        &self,
        node: &str,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
        path: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        if rec_stack.contains(node) {
            // 找到循环，提取循环路径
            let cycle_start = path.iter().position(|n| n == node).unwrap();
            let mut cycle: Vec<String> = path[cycle_start..].to_vec();
            cycle.push(node.to_string()); // 闭合循环
            return Some(cycle);
        }

        if visited.contains(node) {
            return None;
        }

        visited.insert(node.to_string());
        rec_stack.insert(node.to_string());
        path.push(node.to_string());

        if let Some(neighbors) = self.edges.get(node) {
            for neighbor in neighbors {
                if let Some(cycle) = self.find_cycle_util(neighbor, visited, rec_stack, path) {
                    return Some(cycle);
                }
            }
        }

        path.pop();
        rec_stack.remove(node);
        None
    }

    /// 拓扑排序（Kahn 算法）
    ///
    /// 返回一个模块的加载顺序，保证依赖在依赖方之前加载。
    ///
    /// # 返回
    ///
    /// 成功时返回排序后的模块 ID 列表，失败时返回错误（通常是存在循环依赖）。
    ///
    /// # 错误
    ///
    /// 如果存在循环依赖，返回 `CoreError::CircularDependency`。
    ///
    /// # 示例
    ///
    /// ```rust
    /// use chips_core::module::dependency::DependencyGraph;
    ///
    /// let mut graph = DependencyGraph::new();
    /// graph.add_dependency("app", "service");
    /// graph.add_dependency("service", "database");
    ///
    /// let order = graph.topological_sort().unwrap();
    /// // database 在 service 之前，service 在 app 之前
    /// let db_pos = order.iter().position(|x| x == "database").unwrap();
    /// let svc_pos = order.iter().position(|x| x == "service").unwrap();
    /// let app_pos = order.iter().position(|x| x == "app").unwrap();
    /// assert!(db_pos < svc_pos);
    /// assert!(svc_pos < app_pos);
    /// ```
    pub fn topological_sort(&self) -> Result<Vec<String>> {
        // 首先检测循环
        if let Some(cycle) = self.find_cycle() {
            return Err(CoreError::CircularDependency(cycle.join(" -> ")));
        }

        // 计算每个节点的入度（被多少个节点依赖）
        // 注意：这里我们需要的是"谁依赖我"，所以入度是 reverse_edges 中的数量
        // 但实际上，对于加载顺序，我们需要的是"被依赖的模块先加载"
        // 所以入度应该是"依赖了多少个模块"
        let mut in_degree: HashMap<String, usize> = HashMap::new();

        // 初始化所有节点的入度为 0
        for node in self.edges.keys() {
            in_degree.insert(node.clone(), 0);
        }

        // 计算入度：A 依赖 B，则 A 的入度增加
        // 因为 B 必须先加载，所以 A 要等待
        for (_, deps) in &self.edges {
            for dep in deps {
                in_degree.entry(dep.clone()).or_insert(0);
            }
        }

        // 但我们需要的是：被依赖的先输出
        // 所以入度应该是"有多少模块依赖我"的相反
        // 实际上，我们需要统计"我依赖多少模块"
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        for node in self.edges.keys() {
            in_degree.insert(node.clone(), 0);
        }
        for (node, deps) in &self.edges {
            in_degree.insert(node.clone(), deps.len());
            for dep in deps {
                in_degree.entry(dep.clone()).or_insert(0);
            }
        }

        // 初始队列：入度为 0 的节点（不依赖任何模块的）
        let mut queue: VecDeque<String> = in_degree
            .iter()
            .filter(|(_, &degree)| degree == 0)
            .map(|(node, _)| node.clone())
            .collect();

        let mut result = Vec::new();

        while let Some(node) = queue.pop_front() {
            result.push(node.clone());

            // 对于每个依赖此节点的模块，减少其入度
            if let Some(dependents) = self.reverse_edges.get(&node) {
                for dependent in dependents {
                    if let Some(degree) = in_degree.get_mut(dependent) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(dependent.clone());
                        }
                    }
                }
            }
        }

        // 验证是否所有节点都被处理
        let total_nodes = self.edges.len();
        if result.len() != total_nodes {
            // 这不应该发生，因为我们已经检测过循环
            return Err(CoreError::CircularDependency(
                "无法完成拓扑排序".to_string(),
            ));
        }

        Ok(result)
    }

    /// 获取模块加载顺序
    ///
    /// 这是 `topological_sort` 的别名，返回模块应该被加载的顺序。
    /// 保证依赖模块在依赖方之前。
    ///
    /// # 返回
    ///
    /// 模块 ID 的加载顺序列表
    pub fn get_load_order(&self) -> Result<Vec<String>> {
        self.topological_sort()
    }

    /// 获取卸载顺序
    ///
    /// 返回模块应该被卸载的顺序（加载顺序的反序）。
    pub fn get_unload_order(&self) -> Result<Vec<String>> {
        let mut order = self.topological_sort()?;
        order.reverse();
        Ok(order)
    }

    /// 清空依赖图
    ///
    /// 移除所有模块和依赖关系。
    pub fn clear(&mut self) {
        self.edges.clear();
        self.reverse_edges.clear();
    }

    /// 检查图是否为空
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// 获取图中模块数量
    pub fn module_count(&self) -> usize {
        self.edges.len()
    }

    /// 检查是否包含指定模块
    pub fn contains_module(&self, module_id: &str) -> bool {
        self.edges.contains_key(module_id)
    }

    /// 获取所有模块 ID
    pub fn get_all_modules(&self) -> Vec<String> {
        self.edges.keys().cloned().collect()
    }
}

/// 依赖解析器
///
/// 用于解析模块的完整依赖链，包括版本兼容性检查和冲突检测。
pub struct DependencyResolver<'a> {
    /// 模块信息查找函数
    module_lookup: Box<dyn Fn(&str) -> Option<&'a ModuleInfo> + 'a>,
}

impl<'a> DependencyResolver<'a> {
    /// 创建新的依赖解析器
    ///
    /// # 参数
    ///
    /// * `module_lookup` - 模块信息查找函数，根据模块 ID 返回模块信息
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// use std::collections::HashMap;
    /// use chips_core::module::dependency::DependencyResolver;
    /// use chips_core::module::metadata::ModuleInfo;
    ///
    /// let modules: HashMap<String, ModuleInfo> = HashMap::new();
    /// let resolver = DependencyResolver::new(|id| modules.get(id));
    /// ```
    pub fn new<F>(lookup: F) -> Self
    where
        F: Fn(&str) -> Option<&'a ModuleInfo> + 'a,
    {
        Self {
            module_lookup: Box::new(lookup),
        }
    }

    /// 解析模块的所有依赖
    ///
    /// 返回按加载顺序排列的依赖列表（依赖在前，依赖方在后）。
    ///
    /// # 参数
    ///
    /// * `module_id` - 要解析依赖的模块 ID
    ///
    /// # 返回
    ///
    /// 成功时返回依赖模块 ID 列表（按加载顺序），失败时返回错误。
    ///
    /// # 错误
    ///
    /// - `CoreError::ModuleNotFound` - 模块未找到
    /// - `CoreError::DependencyNotFound` - 依赖模块未找到
    /// - `CoreError::CircularDependency` - 存在循环依赖
    /// - `CoreError::VersionMismatch` - 版本不兼容
    pub fn resolve(&self, module_id: &str) -> Result<Vec<String>> {
        let mut resolved = Vec::new();
        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();

        self.resolve_recursive(module_id, &mut resolved, &mut visiting, &mut visited)?;

        Ok(resolved)
    }

    /// 递归解析依赖
    fn resolve_recursive(
        &self,
        module_id: &str,
        resolved: &mut Vec<String>,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
    ) -> Result<()> {
        // 如果已经解析过，跳过
        if visited.contains(module_id) {
            return Ok(());
        }

        // 检测循环依赖
        if visiting.contains(module_id) {
            return Err(CoreError::CircularDependency(format!(
                "模块 '{}' 存在循环依赖",
                module_id
            )));
        }

        // 标记为正在访问
        visiting.insert(module_id.to_string());

        // 获取模块信息
        let module_info = (self.module_lookup)(module_id)
            .ok_or_else(|| CoreError::ModuleNotFound(module_id.to_string()))?;

        // 解析每个依赖
        for dep in &module_info.metadata.dependencies {
            // 跳过可选依赖（如果不存在）
            if !dep.required {
                if (self.module_lookup)(&dep.module_id).is_none() {
                    continue;
                }
            }

            // 获取依赖模块信息
            let dep_info = (self.module_lookup)(&dep.module_id).ok_or_else(|| {
                CoreError::DependencyNotFound(format!(
                    "模块 '{}' 的依赖 '{}' 未找到",
                    module_id, dep.module_id
                ))
            })?;

            // 检查版本兼容性
            self.check_version_compatibility(dep, dep_info)?;

            // 递归解析依赖的依赖
            self.resolve_recursive(&dep.module_id, resolved, visiting, visited)?;
        }

        // 标记为已访问，从正在访问中移除
        visiting.remove(module_id);
        visited.insert(module_id.to_string());

        // 将当前模块加入已解析列表（依赖在前，所以最后添加）
        resolved.push(module_id.to_string());

        Ok(())
    }

    /// 检查版本兼容性
    fn check_version_compatibility(&self, dep: &Dependency, dep_info: &ModuleInfo) -> Result<()> {
        // 解析版本要求
        let version_req = VersionReq::parse(&dep.version).map_err(|e| {
            CoreError::InvalidMetadata(format!(
                "依赖 '{}' 的版本要求 '{}' 格式无效: {}",
                dep.module_id, dep.version, e
            ))
        })?;

        // 解析实际版本
        let actual_version = Version::parse(&dep_info.metadata.version).map_err(|e| {
            CoreError::InvalidMetadata(format!(
                "模块 '{}' 的版本 '{}' 格式无效: {}",
                dep.module_id, dep_info.metadata.version, e
            ))
        })?;

        // 检查版本是否匹配
        if !version_req.matches(&actual_version) {
            return Err(CoreError::VersionMismatch {
                module: dep.module_id.clone(),
                required: dep.version.clone(),
                found: dep_info.metadata.version.clone(),
            });
        }

        Ok(())
    }

    /// 检查依赖冲突
    ///
    /// 检查给定的模块列表是否存在依赖冲突（同一模块的不兼容版本要求）。
    ///
    /// # 参数
    ///
    /// * `modules` - 要检查的模块 ID 列表
    ///
    /// # 返回
    ///
    /// 如果存在冲突返回错误，否则返回 `Ok(())`
    pub fn check_conflicts(&self, modules: &[String]) -> Result<()> {
        // 收集所有模块的依赖版本要求
        let mut version_requirements: HashMap<String, Vec<(String, String)>> = HashMap::new();

        for module_id in modules {
            let module_info = (self.module_lookup)(module_id)
                .ok_or_else(|| CoreError::ModuleNotFound(module_id.clone()))?;

            for dep in &module_info.metadata.dependencies {
                version_requirements
                    .entry(dep.module_id.clone())
                    .or_default()
                    .push((module_id.clone(), dep.version.clone()));
            }
        }

        // 检查每个被依赖模块的版本要求是否兼容
        for (dep_id, requirements) in version_requirements {
            if requirements.len() <= 1 {
                continue;
            }

            // 获取实际版本
            let dep_info = match (self.module_lookup)(&dep_id) {
                Some(info) => info,
                None => continue, // 可能是可选依赖
            };

            let actual_version = match Version::parse(&dep_info.metadata.version) {
                Ok(v) => v,
                Err(_) => continue,
            };

            // 检查所有版本要求是否都满足
            let mut unsatisfied = Vec::new();
            for (requirer, version_req_str) in &requirements {
                if let Ok(version_req) = VersionReq::parse(version_req_str) {
                    if !version_req.matches(&actual_version) {
                        unsatisfied.push(format!("{} 要求 {}", requirer, version_req_str));
                    }
                }
            }

            if !unsatisfied.is_empty() {
                return Err(CoreError::DependencyConflict {
                    module: dep_id,
                    requirements: unsatisfied,
                });
            }
        }

        Ok(())
    }

    /// 构建完整的依赖图
    ///
    /// 从给定的模块列表构建依赖图。
    ///
    /// # 参数
    ///
    /// * `modules` - 模块 ID 列表
    ///
    /// # 返回
    ///
    /// 构建好的依赖图
    pub fn build_dependency_graph(&self, modules: &[String]) -> Result<DependencyGraph> {
        let mut graph = DependencyGraph::new();

        for module_id in modules {
            let module_info = (self.module_lookup)(module_id)
                .ok_or_else(|| CoreError::ModuleNotFound(module_id.clone()))?;

            graph.add_module(module_id);

            for dep in &module_info.metadata.dependencies {
                // 只添加存在的依赖
                if (self.module_lookup)(&dep.module_id).is_some() {
                    graph.add_dependency(module_id, &dep.module_id);
                } else if dep.required {
                    return Err(CoreError::DependencyNotFound(format!(
                        "模块 '{}' 的必需依赖 '{}' 未找到",
                        module_id, dep.module_id
                    )));
                }
            }
        }

        Ok(graph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::metadata::{EntryPoint, ModuleMetadata};
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// 创建测试用的模块信息
    fn create_test_module(id: &str, version: &str, deps: Vec<(&str, &str)>) -> ModuleInfo {
        let mut metadata = ModuleMetadata::new(id, id, version);
        metadata.entry = EntryPoint::rust("lib.rs");
        metadata.dependencies = deps
            .into_iter()
            .map(|(dep_id, dep_ver)| Dependency::new(dep_id, dep_ver))
            .collect();

        ModuleInfo::new(metadata, PathBuf::from(format!("/modules/{}", id)))
    }

    // ==================== DependencyGraph 测试 ====================

    #[test]
    fn test_dependency_graph_new() {
        let graph = DependencyGraph::new();
        assert!(graph.is_empty());
        assert_eq!(graph.module_count(), 0);
    }

    #[test]
    fn test_add_module() {
        let mut graph = DependencyGraph::new();
        graph.add_module("module_a");

        assert!(graph.contains_module("module_a"));
        assert_eq!(graph.module_count(), 1);
    }

    #[test]
    fn test_add_module_duplicate() {
        let mut graph = DependencyGraph::new();
        graph.add_module("module_a");
        graph.add_module("module_a");

        assert_eq!(graph.module_count(), 1);
    }

    #[test]
    fn test_add_dependency() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("module_a", "module_b");

        assert!(graph.contains_module("module_a"));
        assert!(graph.contains_module("module_b"));
        assert_eq!(graph.get_dependencies("module_a"), vec!["module_b"]);
        assert_eq!(graph.get_dependents("module_b"), vec!["module_a"]);
    }

    #[test]
    fn test_add_dependency_duplicate() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("a", "b");

        assert_eq!(graph.get_dependencies("a").len(), 1);
    }

    #[test]
    fn test_remove_dependency() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("a", "c");

        graph.remove_dependency("a", "b");

        assert_eq!(graph.get_dependencies("a"), vec!["c"]);
        assert!(graph.get_dependents("b").is_empty());
    }

    #[test]
    fn test_remove_module() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("c", "b");
        graph.add_dependency("b", "d");

        graph.remove_module("b");

        assert!(!graph.contains_module("b"));
        assert!(graph.get_dependencies("a").is_empty());
        assert!(graph.get_dependencies("c").is_empty());
    }

    #[test]
    fn test_get_all_dependencies() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("b", "c");
        graph.add_dependency("c", "d");

        let all_deps = graph.get_all_dependencies("a");
        assert!(all_deps.contains(&"b".to_string()));
        assert!(all_deps.contains(&"c".to_string()));
        assert!(all_deps.contains(&"d".to_string()));
    }

    #[test]
    fn test_has_cycle_no_cycle() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("b", "c");
        graph.add_dependency("a", "c");

        assert!(!graph.has_cycle());
    }

    #[test]
    fn test_has_cycle_with_cycle() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("b", "c");
        graph.add_dependency("c", "a");

        assert!(graph.has_cycle());
    }

    #[test]
    fn test_has_cycle_self_loop() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "a");

        assert!(graph.has_cycle());
    }

    #[test]
    fn test_find_cycle() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("b", "c");
        graph.add_dependency("c", "a");

        let cycle = graph.find_cycle();
        assert!(cycle.is_some());
        let cycle = cycle.unwrap();
        assert!(cycle.len() >= 3);
    }

    #[test]
    fn test_find_cycle_no_cycle() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("b", "c");

        assert!(graph.find_cycle().is_none());
    }

    #[test]
    fn test_topological_sort_simple() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("app", "service");
        graph.add_dependency("service", "database");

        let order = graph.topological_sort().unwrap();

        let db_pos = order.iter().position(|x| x == "database").unwrap();
        let svc_pos = order.iter().position(|x| x == "service").unwrap();
        let app_pos = order.iter().position(|x| x == "app").unwrap();

        assert!(db_pos < svc_pos);
        assert!(svc_pos < app_pos);
    }

    #[test]
    fn test_topological_sort_complex() {
        let mut graph = DependencyGraph::new();
        // 构建更复杂的依赖图
        //     app
        //    /   \
        //   a     b
        //    \   /
        //      c
        //      |
        //      d
        graph.add_dependency("app", "a");
        graph.add_dependency("app", "b");
        graph.add_dependency("a", "c");
        graph.add_dependency("b", "c");
        graph.add_dependency("c", "d");

        let order = graph.topological_sort().unwrap();

        // d 必须在 c 之前
        // c 必须在 a 和 b 之前
        // a 和 b 必须在 app 之前
        let d_pos = order.iter().position(|x| x == "d").unwrap();
        let c_pos = order.iter().position(|x| x == "c").unwrap();
        let a_pos = order.iter().position(|x| x == "a").unwrap();
        let b_pos = order.iter().position(|x| x == "b").unwrap();
        let app_pos = order.iter().position(|x| x == "app").unwrap();

        assert!(d_pos < c_pos);
        assert!(c_pos < a_pos);
        assert!(c_pos < b_pos);
        assert!(a_pos < app_pos);
        assert!(b_pos < app_pos);
    }

    #[test]
    fn test_topological_sort_with_cycle() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("b", "c");
        graph.add_dependency("c", "a");

        let result = graph.topological_sort();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::CircularDependency(_)));
    }

    #[test]
    fn test_get_load_order() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("b", "c");

        let order = graph.get_load_order().unwrap();
        assert_eq!(order, vec!["c", "b", "a"]);
    }

    #[test]
    fn test_get_unload_order() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("b", "c");

        let order = graph.get_unload_order().unwrap();
        assert_eq!(order, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_clear() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("a", "b");
        graph.add_dependency("c", "d");

        graph.clear();

        assert!(graph.is_empty());
        assert_eq!(graph.module_count(), 0);
    }

    #[test]
    fn test_get_all_modules() {
        let mut graph = DependencyGraph::new();
        graph.add_module("a");
        graph.add_module("b");
        graph.add_module("c");

        let modules = graph.get_all_modules();
        assert_eq!(modules.len(), 3);
        assert!(modules.contains(&"a".to_string()));
        assert!(modules.contains(&"b".to_string()));
        assert!(modules.contains(&"c".to_string()));
    }

    // ==================== DependencyResolver 测试 ====================

    #[test]
    fn test_resolver_resolve_simple() {
        let mut modules = HashMap::new();
        modules.insert("a".to_string(), create_test_module("a", "1.0.0", vec![("b", "^1.0")]));
        modules.insert("b".to_string(), create_test_module("b", "1.0.0", vec![]));

        let resolver = DependencyResolver::new(|id| modules.get(id));

        let result = resolver.resolve("a").unwrap();
        assert_eq!(result, vec!["b", "a"]);
    }

    #[test]
    fn test_resolver_resolve_chain() {
        let mut modules = HashMap::new();
        modules.insert("a".to_string(), create_test_module("a", "1.0.0", vec![("b", "^1.0")]));
        modules.insert("b".to_string(), create_test_module("b", "1.0.0", vec![("c", "^1.0")]));
        modules.insert("c".to_string(), create_test_module("c", "1.0.0", vec![]));

        let resolver = DependencyResolver::new(|id| modules.get(id));

        let result = resolver.resolve("a").unwrap();
        assert_eq!(result, vec!["c", "b", "a"]);
    }

    #[test]
    fn test_resolver_resolve_diamond() {
        // 菱形依赖：a -> b, a -> c, b -> d, c -> d
        let mut modules = HashMap::new();
        modules.insert(
            "a".to_string(),
            create_test_module("a", "1.0.0", vec![("b", "^1.0"), ("c", "^1.0")]),
        );
        modules.insert("b".to_string(), create_test_module("b", "1.0.0", vec![("d", "^1.0")]));
        modules.insert("c".to_string(), create_test_module("c", "1.0.0", vec![("d", "^1.0")]));
        modules.insert("d".to_string(), create_test_module("d", "1.0.0", vec![]));

        let resolver = DependencyResolver::new(|id| modules.get(id));

        let result = resolver.resolve("a").unwrap();

        // d 应该在 b 和 c 之前
        let d_pos = result.iter().position(|x| x == "d").unwrap();
        let b_pos = result.iter().position(|x| x == "b").unwrap();
        let c_pos = result.iter().position(|x| x == "c").unwrap();
        let a_pos = result.iter().position(|x| x == "a").unwrap();

        assert!(d_pos < b_pos);
        assert!(d_pos < c_pos);
        assert!(b_pos < a_pos);
        assert!(c_pos < a_pos);
    }

    #[test]
    fn test_resolver_module_not_found() {
        let modules: HashMap<String, ModuleInfo> = HashMap::new();
        let resolver = DependencyResolver::new(|id| modules.get(id));

        let result = resolver.resolve("nonexistent");
        assert!(matches!(result, Err(CoreError::ModuleNotFound(_))));
    }

    #[test]
    fn test_resolver_dependency_not_found() {
        let mut modules = HashMap::new();
        modules.insert(
            "a".to_string(),
            create_test_module("a", "1.0.0", vec![("nonexistent", "^1.0")]),
        );

        let resolver = DependencyResolver::new(|id| modules.get(id));

        let result = resolver.resolve("a");
        assert!(matches!(result, Err(CoreError::DependencyNotFound(_))));
    }

    #[test]
    fn test_resolver_circular_dependency() {
        let mut modules = HashMap::new();
        modules.insert("a".to_string(), create_test_module("a", "1.0.0", vec![("b", "^1.0")]));
        modules.insert("b".to_string(), create_test_module("b", "1.0.0", vec![("c", "^1.0")]));
        modules.insert("c".to_string(), create_test_module("c", "1.0.0", vec![("a", "^1.0")]));

        let resolver = DependencyResolver::new(|id| modules.get(id));

        let result = resolver.resolve("a");
        assert!(matches!(result, Err(CoreError::CircularDependency(_))));
    }

    #[test]
    fn test_resolver_version_mismatch() {
        let mut modules = HashMap::new();
        modules.insert("a".to_string(), create_test_module("a", "1.0.0", vec![("b", "^2.0")]));
        modules.insert("b".to_string(), create_test_module("b", "1.0.0", vec![]));

        let resolver = DependencyResolver::new(|id| modules.get(id));

        let result = resolver.resolve("a");
        assert!(matches!(result, Err(CoreError::VersionMismatch { .. })));
    }

    #[test]
    fn test_resolver_version_compatible() {
        let mut modules = HashMap::new();
        modules.insert("a".to_string(), create_test_module("a", "1.0.0", vec![("b", "^1.0")]));
        modules.insert("b".to_string(), create_test_module("b", "1.5.0", vec![]));

        let resolver = DependencyResolver::new(|id| modules.get(id));

        let result = resolver.resolve("a");
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolver_check_conflicts_no_conflict() {
        let mut modules = HashMap::new();
        modules.insert("a".to_string(), create_test_module("a", "1.0.0", vec![("c", "^1.0")]));
        modules.insert("b".to_string(), create_test_module("b", "1.0.0", vec![("c", "^1.0")]));
        modules.insert("c".to_string(), create_test_module("c", "1.5.0", vec![]));

        let resolver = DependencyResolver::new(|id| modules.get(id));

        let result = resolver.check_conflicts(&["a".to_string(), "b".to_string()]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolver_check_conflicts_with_conflict() {
        let mut modules = HashMap::new();
        modules.insert("a".to_string(), create_test_module("a", "1.0.0", vec![("c", "^1.0")]));
        modules.insert("b".to_string(), create_test_module("b", "1.0.0", vec![("c", "^2.0")]));
        modules.insert("c".to_string(), create_test_module("c", "1.5.0", vec![]));

        let resolver = DependencyResolver::new(|id| modules.get(id));

        let result = resolver.check_conflicts(&["a".to_string(), "b".to_string()]);
        assert!(matches!(result, Err(CoreError::DependencyConflict { .. })));
    }

    #[test]
    fn test_resolver_build_dependency_graph() {
        let mut modules = HashMap::new();
        modules.insert("a".to_string(), create_test_module("a", "1.0.0", vec![("b", "^1.0")]));
        modules.insert("b".to_string(), create_test_module("b", "1.0.0", vec![("c", "^1.0")]));
        modules.insert("c".to_string(), create_test_module("c", "1.0.0", vec![]));

        let resolver = DependencyResolver::new(|id| modules.get(id));

        let graph = resolver
            .build_dependency_graph(&["a".to_string(), "b".to_string(), "c".to_string()])
            .unwrap();

        assert_eq!(graph.module_count(), 3);
        assert_eq!(graph.get_dependencies("a"), vec!["b"]);
        assert_eq!(graph.get_dependencies("b"), vec!["c"]);
        assert!(graph.get_dependencies("c").is_empty());
    }

    #[test]
    fn test_resolver_optional_dependency_missing() {
        let mut modules = HashMap::new();

        // 创建带有可选依赖的模块
        let mut metadata = ModuleMetadata::new("a", "a", "1.0.0");
        metadata.entry = EntryPoint::rust("lib.rs");
        metadata.dependencies = vec![Dependency::new("optional_dep", "^1.0").optional()];
        modules.insert(
            "a".to_string(),
            ModuleInfo::new(metadata, PathBuf::from("/modules/a")),
        );

        let resolver = DependencyResolver::new(|id| modules.get(id));

        // 可选依赖不存在时应该成功
        let result = resolver.resolve("a");
        assert!(result.is_ok());
    }
}
