//! 模块元数据解析器
//!
//! 负责从 module.yaml 文件解析模块元数据。

use std::path::Path;

use crate::module::metadata::ModuleMetadata;
use crate::utils::{CoreError, Result};

/// 模块元数据解析器
///
/// 提供从文件或字符串解析 module.yaml 的功能。
#[derive(Debug, Clone, Default)]
pub struct ModuleParser;

impl ModuleParser {
    /// 创建新的解析器实例
    pub fn new() -> Self {
        Self
    }

    /// 从文件解析模块元数据
    ///
    /// # Arguments
    ///
    /// * `path` - module.yaml 文件路径
    ///
    /// # Returns
    ///
    /// 解析后的 `ModuleMetadata`
    ///
    /// # Errors
    ///
    /// - 文件不存在或无法读取时返回 IO 错误
    /// - 文件内容不符合 YAML 格式时返回 YAML 错误
    /// - 元数据验证失败时返回 `InvalidMetadata` 错误
    pub async fn parse_file(path: &Path) -> Result<ModuleMetadata> {
        let content = tokio::fs::read_to_string(path).await?;
        Self::parse_string(&content)
    }

    /// 从文件同步解析模块元数据
    ///
    /// # Arguments
    ///
    /// * `path` - module.yaml 文件路径
    ///
    /// # Returns
    ///
    /// 解析后的 `ModuleMetadata`
    pub fn parse_file_sync(path: &Path) -> Result<ModuleMetadata> {
        let content = std::fs::read_to_string(path)?;
        Self::parse_string(&content)
    }

    /// 从字符串解析模块元数据
    ///
    /// # Arguments
    ///
    /// * `content` - YAML 格式的元数据字符串
    ///
    /// # Returns
    ///
    /// 解析后的 `ModuleMetadata`
    ///
    /// # Errors
    ///
    /// - YAML 解析失败时返回 `Yaml` 错误
    /// - 验证失败时返回 `InvalidMetadata` 错误
    pub fn parse_string(content: &str) -> Result<ModuleMetadata> {
        let metadata: ModuleMetadata = serde_yaml::from_str(content)?;
        Self::validate(&metadata)?;
        Ok(metadata)
    }

    /// 验证模块元数据
    ///
    /// 执行以下验证：
    /// - 必填字段检查（id, name, version, entry.path）
    /// - 版本号格式验证（semver）
    /// - 依赖声明版本格式验证
    /// - 权限声明格式验证
    ///
    /// # Arguments
    ///
    /// * `metadata` - 要验证的元数据
    ///
    /// # Returns
    ///
    /// 验证通过返回 `Ok(())`，失败返回详细的错误信息
    pub fn validate(metadata: &ModuleMetadata) -> Result<()> {
        let mut errors: Vec<String> = Vec::new();

        // 1. 验证必填字段
        if metadata.id.is_empty() {
            errors.push("模块 ID 不能为空".to_string());
        } else {
            // 验证 ID 格式（只允许字母、数字、下划线、连字符）
            if !Self::is_valid_module_id(&metadata.id) {
                errors.push(format!(
                    "模块 ID '{}' 格式无效，只允许字母、数字、下划线和连字符",
                    metadata.id
                ));
            }
        }

        if metadata.name.is_empty() {
            errors.push("模块名称不能为空".to_string());
        }

        // 2. 验证版本号格式
        if metadata.version.is_empty() {
            errors.push("模块版本号不能为空".to_string());
        } else if semver::Version::parse(&metadata.version).is_err() {
            errors.push(format!(
                "无效的版本号格式 '{}', 请使用 semver 格式 (如 1.0.0)",
                metadata.version
            ));
        }

        // 3. 验证入口点
        if metadata.entry.path.is_empty() {
            errors.push("入口文件路径不能为空".to_string());
        }

        // 4. 验证依赖声明
        for (index, dep) in metadata.dependencies.iter().enumerate() {
            if dep.module_id.is_empty() {
                errors.push(format!("第 {} 个依赖的模块 ID 不能为空", index + 1));
            }

            if dep.version.is_empty() {
                errors.push(format!(
                    "依赖 '{}' 的版本要求不能为空",
                    dep.module_id
                ));
            } else if semver::VersionReq::parse(&dep.version).is_err() {
                errors.push(format!(
                    "依赖 '{}' 的版本要求格式无效: '{}', 请使用 semver 范围格式 (如 ^1.0.0)",
                    dep.module_id, dep.version
                ));
            }
        }

        // 5. 验证权限声明
        for permission in &metadata.permissions.requested {
            if !Self::is_valid_permission(permission) {
                errors.push(format!(
                    "无效的权限声明格式: '{}'",
                    permission
                ));
            }
        }

        // 6. 验证兼容性声明
        if let Some(ref min_version) = metadata.compatibility.min_core_version {
            if semver::VersionReq::parse(min_version).is_err() {
                errors.push(format!(
                    "最低内核版本格式无效: '{}'",
                    min_version
                ));
            }
        }

        // 7. 验证接口声明 - action 名称格式
        for action in &metadata.interfaces.provides {
            if !Self::is_valid_action_name(action) {
                errors.push(format!(
                    "无效的 action 名称格式: '{}', 推荐使用 domain.action 格式",
                    action
                ));
            }
        }

        // 返回结果
        if errors.is_empty() {
            Ok(())
        } else {
            Err(CoreError::InvalidMetadata(errors.join("; ")))
        }
    }

    /// 检查模块 ID 格式是否有效
    ///
    /// 有效格式：字母开头，只包含字母、数字、下划线和连字符
    fn is_valid_module_id(id: &str) -> bool {
        if id.is_empty() {
            return false;
        }

        let first_char = id.chars().next().unwrap();
        if !first_char.is_ascii_alphabetic() {
            return false;
        }

        id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    }

    /// 检查权限声明格式是否有效
    ///
    /// 有效格式：domain.action 或 domain.*
    fn is_valid_permission(permission: &str) -> bool {
        if permission.is_empty() {
            return false;
        }

        // 允许的格式：
        // - domain.action (如 file.read)
        // - domain.* (如 file.*)
        // - 简单名称 (如 network)
        let parts: Vec<&str> = permission.split('.').collect();

        match parts.len() {
            1 => {
                // 简单名称
                Self::is_valid_identifier(parts[0])
            }
            2 => {
                // domain.action 格式
                Self::is_valid_identifier(parts[0])
                    && (parts[1] == "*" || Self::is_valid_identifier(parts[1]))
            }
            _ => false,
        }
    }

    /// 检查 action 名称格式是否有效
    ///
    /// 推荐格式：domain.action（如 card.create）
    fn is_valid_action_name(action: &str) -> bool {
        if action.is_empty() {
            return false;
        }

        // 允许 domain.action 格式或简单名称
        let parts: Vec<&str> = action.split('.').collect();

        parts.iter().all(|part| Self::is_valid_identifier(part))
    }

    /// 检查标识符是否有效（字母数字和下划线）
    fn is_valid_identifier(s: &str) -> bool {
        if s.is_empty() {
            return false;
        }

        let first_char = s.chars().next().unwrap();
        if !first_char.is_ascii_alphabetic() && first_char != '_' {
            return false;
        }

        s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::metadata::{EntryPoint, Runtime, Dependency, ModuleType};

    /// 创建一个有效的测试元数据
    fn create_valid_metadata() -> ModuleMetadata {
        let mut metadata = ModuleMetadata::new("test-module", "Test Module", "1.0.0");
        metadata.entry = EntryPoint {
            runtime: Runtime::Rust,
            path: "lib.rs".to_string(),
            function: None,
        };
        metadata
    }

    #[test]
    fn test_parse_valid_yaml() {
        let yaml = r#"
id: "video-card-plugin"
name: "视频卡片插件"
version: "1.2.3"
description: "支持视频卡片的编辑和渲染"
author: "Chips Team"
license: "MIT"
module_type: plugin
entry:
  runtime: javascript
  path: "dist/index.js"
dependencies:
  - module_id: "video-decoder"
    version: "^2.0.0"
    required: true
interfaces:
  provides:
    - "video.edit"
    - "video.render"
permissions:
  requested:
    - "file.read"
    - "file.write"
compatibility:
  protocol_version: "1.0"
  platforms:
    - "win32"
    - "darwin"
    - "linux"
"#;

        let result = ModuleParser::parse_string(yaml);
        assert!(result.is_ok(), "解析失败: {:?}", result.err());

        let metadata = result.unwrap();
        assert_eq!(metadata.id, "video-card-plugin");
        assert_eq!(metadata.name, "视频卡片插件");
        assert_eq!(metadata.version, "1.2.3");
        assert_eq!(metadata.module_type, ModuleType::Plugin);
        assert_eq!(metadata.entry.runtime, Runtime::JavaScript);
        assert_eq!(metadata.entry.path, "dist/index.js");
        assert_eq!(metadata.dependencies.len(), 1);
        assert_eq!(metadata.dependencies[0].module_id, "video-decoder");
        assert_eq!(metadata.interfaces.provides.len(), 2);
        assert_eq!(metadata.permissions.requested.len(), 2);
    }

    #[test]
    fn test_parse_minimal_yaml() {
        let yaml = r#"
id: "minimal-module"
name: "Minimal Module"
version: "0.1.0"
entry:
  runtime: rust
  path: "lib.rs"
"#;

        let result = ModuleParser::parse_string(yaml);
        assert!(result.is_ok(), "解析最小配置失败: {:?}", result.err());

        let metadata = result.unwrap();
        assert_eq!(metadata.id, "minimal-module");
        assert_eq!(metadata.module_type, ModuleType::Atomic); // 默认值
    }

    #[test]
    fn test_validate_empty_id() {
        let mut metadata = create_valid_metadata();
        metadata.id = String::new();

        let result = ModuleParser::validate(&metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("ID 不能为空"));
    }

    #[test]
    fn test_validate_invalid_id_format() {
        let mut metadata = create_valid_metadata();
        metadata.id = "123-invalid".to_string(); // 数字开头

        let result = ModuleParser::validate(&metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("格式无效"));
    }

    #[test]
    fn test_validate_empty_name() {
        let mut metadata = create_valid_metadata();
        metadata.name = String::new();

        let result = ModuleParser::validate(&metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("名称不能为空"));
    }

    #[test]
    fn test_validate_invalid_version() {
        let mut metadata = create_valid_metadata();
        metadata.version = "not-a-version".to_string();

        let result = ModuleParser::validate(&metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("版本号格式"));
    }

    #[test]
    fn test_validate_empty_version() {
        let mut metadata = create_valid_metadata();
        metadata.version = String::new();

        let result = ModuleParser::validate(&metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("版本号不能为空"));
    }

    #[test]
    fn test_validate_empty_entry_path() {
        let mut metadata = create_valid_metadata();
        metadata.entry.path = String::new();

        let result = ModuleParser::validate(&metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("入口文件路径不能为空"));
    }

    #[test]
    fn test_validate_dependency_invalid_version() {
        let mut metadata = create_valid_metadata();
        metadata.dependencies.push(Dependency {
            module_id: "other-module".to_string(),
            version: "invalid-version".to_string(),
            required: true,
        });

        let result = ModuleParser::validate(&metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("版本要求格式无效"));
    }

    #[test]
    fn test_validate_dependency_empty_module_id() {
        let mut metadata = create_valid_metadata();
        metadata.dependencies.push(Dependency {
            module_id: String::new(),
            version: "^1.0.0".to_string(),
            required: true,
        });

        let result = ModuleParser::validate(&metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("模块 ID 不能为空"));
    }

    #[test]
    fn test_validate_valid_permissions() {
        let mut metadata = create_valid_metadata();
        metadata.permissions.requested = vec![
            "file.read".to_string(),
            "file.write".to_string(),
            "network".to_string(),
            "file.*".to_string(),
        ];

        let result = ModuleParser::validate(&metadata);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_invalid_permission_format() {
        let mut metadata = create_valid_metadata();
        metadata.permissions.requested = vec!["invalid..permission".to_string()];

        let result = ModuleParser::validate(&metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("无效的权限声明格式"));
    }

    #[test]
    fn test_validate_valid_action_names() {
        let mut metadata = create_valid_metadata();
        metadata.interfaces.provides = vec![
            "card.create".to_string(),
            "card.update".to_string(),
            "simple_action".to_string(),
        ];

        let result = ModuleParser::validate(&metadata);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_invalid_action_name() {
        let mut metadata = create_valid_metadata();
        metadata.interfaces.provides = vec!["123invalid".to_string()];

        let result = ModuleParser::validate(&metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("无效的 action 名称格式"));
    }

    #[test]
    fn test_is_valid_module_id() {
        assert!(ModuleParser::is_valid_module_id("valid-module"));
        assert!(ModuleParser::is_valid_module_id("valid_module"));
        assert!(ModuleParser::is_valid_module_id("validModule123"));
        assert!(ModuleParser::is_valid_module_id("Module"));

        assert!(!ModuleParser::is_valid_module_id(""));
        assert!(!ModuleParser::is_valid_module_id("123module"));
        assert!(!ModuleParser::is_valid_module_id("-invalid"));
        assert!(!ModuleParser::is_valid_module_id("invalid.module"));
        assert!(!ModuleParser::is_valid_module_id("invalid module"));
    }

    #[test]
    fn test_is_valid_permission() {
        assert!(ModuleParser::is_valid_permission("file.read"));
        assert!(ModuleParser::is_valid_permission("file.*"));
        assert!(ModuleParser::is_valid_permission("network"));
        assert!(ModuleParser::is_valid_permission("media_encode"));

        assert!(!ModuleParser::is_valid_permission(""));
        assert!(!ModuleParser::is_valid_permission("file..read"));
        assert!(!ModuleParser::is_valid_permission(".read"));
        assert!(!ModuleParser::is_valid_permission("file.read.write"));
    }

    #[test]
    fn test_is_valid_action_name() {
        assert!(ModuleParser::is_valid_action_name("card.create"));
        assert!(ModuleParser::is_valid_action_name("simple_action"));
        assert!(ModuleParser::is_valid_action_name("domain.sub.action"));

        assert!(!ModuleParser::is_valid_action_name(""));
        assert!(!ModuleParser::is_valid_action_name("123action"));
        assert!(!ModuleParser::is_valid_action_name(".action"));
    }

    #[test]
    fn test_parse_yaml_with_all_runtime_types() {
        for (runtime_str, expected) in [
            ("rust", Runtime::Rust),
            ("javascript", Runtime::JavaScript),
            ("js", Runtime::JavaScript),
            ("python", Runtime::Python),
            ("webassembly", Runtime::WebAssembly),
            ("wasm", Runtime::WebAssembly),
        ] {
            let yaml = format!(
                r#"
id: "test-module"
name: "Test"
version: "1.0.0"
entry:
  runtime: {}
  path: "main"
"#,
                runtime_str
            );

            let result = ModuleParser::parse_string(&yaml);
            assert!(result.is_ok(), "Failed to parse runtime: {}", runtime_str);
            assert_eq!(result.unwrap().entry.runtime, expected);
        }
    }

    #[test]
    fn test_parse_yaml_with_all_module_types() {
        for (type_str, expected) in [
            ("atomic", ModuleType::Atomic),
            ("composite", ModuleType::Composite),
            ("plugin", ModuleType::Plugin),
            ("application", ModuleType::Application),
            ("service", ModuleType::Service),
        ] {
            let yaml = format!(
                r#"
id: "test-module"
name: "Test"
version: "1.0.0"
module_type: {}
entry:
  runtime: rust
  path: "main"
"#,
                type_str
            );

            let result = ModuleParser::parse_string(&yaml);
            assert!(result.is_ok(), "Failed to parse module_type: {}", type_str);
            assert_eq!(result.unwrap().module_type, expected);
        }
    }

    #[test]
    fn test_parse_invalid_yaml_syntax() {
        let invalid_yaml = r#"
id: "test
name: "broken yaml
"#;

        let result = ModuleParser::parse_string(invalid_yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_validation_errors() {
        let mut metadata = create_valid_metadata();
        metadata.id = String::new();
        metadata.name = String::new();
        metadata.version = "invalid".to_string();

        let result = ModuleParser::validate(&metadata);
        assert!(result.is_err());

        let error_msg = result.unwrap_err().to_string();
        // 应该包含多个错误
        assert!(error_msg.contains("ID 不能为空"));
        assert!(error_msg.contains("名称不能为空"));
        assert!(error_msg.contains("版本号格式"));
    }

    #[test]
    fn test_validate_compatibility_min_core_version() {
        let mut metadata = create_valid_metadata();
        metadata.compatibility.min_core_version = Some("^1.0.0".to_string());

        let result = ModuleParser::validate(&metadata);
        assert!(result.is_ok());

        metadata.compatibility.min_core_version = Some("invalid-version".to_string());
        let result = ModuleParser::validate(&metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("最低内核版本格式无效"));
    }
}
