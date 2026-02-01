//! 请求验证器
//!
//! 提供路由请求的格式验证和权限检查功能。

use chrono::{Duration, Utc};
use regex::Regex;
use std::sync::LazyLock;

use super::RouteRequest;
use crate::utils::{CoreError, Result};

/// Action 格式正则表达式
///
/// 格式: namespace.action
/// - namespace: 小写字母、数字和连字符，如 `card`, `file-system`
/// - action: 小写字母、数字和下划线，如 `read`, `list_all`
static ACTION_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z][a-z0-9-]*\.[a-z][a-z0-9_]*$").expect("Invalid action regex")
});

/// 验证错误详情
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// 错误字段
    pub field: String,
    /// 错误消息
    pub message: String,
    /// 错误码
    pub code: ValidationErrorCode,
}

impl ValidationError {
    /// 创建新的验证错误
    pub fn new(field: impl Into<String>, message: impl Into<String>, code: ValidationErrorCode) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            code,
        }
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}: {}", self.code, self.field, self.message)
    }
}

/// 验证错误码
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationErrorCode {
    /// 字段为空
    EmptyField,
    /// 格式无效
    InvalidFormat,
    /// 值超出范围
    OutOfRange,
    /// 权限不足
    PermissionDenied,
    /// 权限未声明
    PermissionNotDeclared,
}

impl std::fmt::Display for ValidationErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationErrorCode::EmptyField => write!(f, "EMPTY_FIELD"),
            ValidationErrorCode::InvalidFormat => write!(f, "INVALID_FORMAT"),
            ValidationErrorCode::OutOfRange => write!(f, "OUT_OF_RANGE"),
            ValidationErrorCode::PermissionDenied => write!(f, "PERMISSION_DENIED"),
            ValidationErrorCode::PermissionNotDeclared => write!(f, "PERMISSION_NOT_DECLARED"),
        }
    }
}

/// 验证结果
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// 是否通过验证
    pub is_valid: bool,
    /// 验证错误列表
    pub errors: Vec<ValidationError>,
}

impl ValidationResult {
    /// 创建成功的验证结果
    pub fn success() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
        }
    }

    /// 创建失败的验证结果
    pub fn failure(errors: Vec<ValidationError>) -> Self {
        Self {
            is_valid: false,
            errors,
        }
    }

    /// 添加错误
    pub fn add_error(&mut self, error: ValidationError) {
        self.is_valid = false;
        self.errors.push(error);
    }

    /// 是否有错误
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// 转换为 Result
    pub fn into_result(self) -> Result<()> {
        if self.is_valid {
            Ok(())
        } else {
            let messages: Vec<String> = self.errors.iter().map(|e| e.to_string()).collect();
            Err(CoreError::InvalidRequest(messages.join("; ")))
        }
    }
}

/// 请求验证器配置
#[derive(Debug, Clone)]
pub struct ValidatorConfig {
    /// 时间戳允许的最大过去偏差（秒）
    pub max_timestamp_drift_past_secs: i64,
    /// 时间戳允许的最大未来偏差（秒）
    pub max_timestamp_drift_future_secs: i64,
    /// 请求 ID 最大长度
    pub max_request_id_length: usize,
    /// 发送者 ID 最大长度
    pub max_sender_length: usize,
    /// Action 最大长度
    pub max_action_length: usize,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            // 允许 5 分钟的过去偏差
            max_timestamp_drift_past_secs: 300,
            // 允许 60 秒的未来偏差（考虑时钟同步误差）
            max_timestamp_drift_future_secs: 60,
            max_request_id_length: 128,
            max_sender_length: 64,
            max_action_length: 128,
        }
    }
}

/// 请求验证器
///
/// 用于验证路由请求的格式和权限。
#[derive(Debug, Clone)]
pub struct RequestValidator {
    /// 验证器配置
    config: ValidatorConfig,
}

impl RequestValidator {
    /// 创建新的请求验证器
    pub fn new() -> Self {
        Self {
            config: ValidatorConfig::default(),
        }
    }

    /// 使用自定义配置创建验证器
    pub fn with_config(config: ValidatorConfig) -> Self {
        Self { config }
    }

    /// 获取配置
    pub fn config(&self) -> &ValidatorConfig {
        &self.config
    }

    /// 验证请求格式
    ///
    /// 检查以下内容：
    /// - request_id 非空且长度合法
    /// - sender 非空且长度合法
    /// - action 非空、格式正确（namespace.action）且长度合法
    /// - timestamp 在允许的时间范围内
    ///
    /// # Arguments
    ///
    /// * `request` - 要验证的路由请求
    ///
    /// # Returns
    ///
    /// 返回 `ValidationResult`，包含验证是否通过以及所有验证错误
    pub fn validate_format(&self, request: &RouteRequest) -> ValidationResult {
        let mut result = ValidationResult::success();

        // 验证 request_id
        self.validate_request_id(&request.request_id, &mut result);

        // 验证 sender
        self.validate_sender(&request.sender, &mut result);

        // 验证 action
        self.validate_action(&request.action, &mut result);

        // 验证 timestamp
        self.validate_timestamp(&request.timestamp, &mut result);

        result
    }

    /// 验证权限
    ///
    /// 检查发送者是否具有执行指定 action 的权限。
    ///
    /// # Arguments
    ///
    /// * `request` - 要验证的路由请求
    /// * `required_permissions` - 执行该操作所需的权限列表
    /// * `sender_permissions` - 发送者拥有的权限列表
    ///
    /// # Returns
    ///
    /// 返回 `ValidationResult`
    ///
    /// # Note
    ///
    /// 当前为骨架实现，后续会集成完整的权限系统。
    pub fn validate_permission(
        &self,
        request: &RouteRequest,
        required_permissions: &[String],
        sender_permissions: &[String],
    ) -> ValidationResult {
        let mut result = ValidationResult::success();

        // 如果没有要求任何权限，直接通过
        if required_permissions.is_empty() {
            return result;
        }

        // 检查发送者是否拥有所有必需的权限
        for required in required_permissions {
            if !sender_permissions.contains(required) {
                result.add_error(ValidationError::new(
                    "permission",
                    format!(
                        "发送者 '{}' 缺少权限 '{}' 以执行 '{}'",
                        request.sender, required, request.action
                    ),
                    ValidationErrorCode::PermissionDenied,
                ));
            }
        }

        result
    }

    /// 完整验证（格式 + 权限）
    ///
    /// # Arguments
    ///
    /// * `request` - 要验证的路由请求
    /// * `required_permissions` - 执行该操作所需的权限列表
    /// * `sender_permissions` - 发送者拥有的权限列表
    ///
    /// # Returns
    ///
    /// 返回 `Result<()>`，验证失败时返回 `CoreError::InvalidRequest` 或 `CoreError::PermissionDenied`
    pub fn validate(
        &self,
        request: &RouteRequest,
        required_permissions: &[String],
        sender_permissions: &[String],
    ) -> Result<()> {
        // 先验证格式
        let format_result = self.validate_format(request);
        if format_result.has_errors() {
            return format_result.into_result();
        }

        // 再验证权限
        let permission_result =
            self.validate_permission(request, required_permissions, sender_permissions);
        if permission_result.has_errors() {
            // 权限错误使用专门的错误类型
            let messages: Vec<String> = permission_result
                .errors
                .iter()
                .map(|e| e.message.clone())
                .collect();
            return Err(CoreError::PermissionDenied(messages.join("; ")));
        }

        Ok(())
    }

    // ==================== 私有验证方法 ====================

    /// 验证 request_id
    fn validate_request_id(&self, request_id: &str, result: &mut ValidationResult) {
        // 检查非空
        if request_id.is_empty() {
            result.add_error(ValidationError::new(
                "request_id",
                "请求 ID 不能为空",
                ValidationErrorCode::EmptyField,
            ));
            return;
        }

        // 检查长度
        if request_id.len() > self.config.max_request_id_length {
            result.add_error(ValidationError::new(
                "request_id",
                format!(
                    "请求 ID 长度超过限制（最大 {} 字符）",
                    self.config.max_request_id_length
                ),
                ValidationErrorCode::OutOfRange,
            ));
        }

        // 检查是否只包含合法字符（字母、数字、连字符、下划线）
        if !request_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            result.add_error(ValidationError::new(
                "request_id",
                "请求 ID 只能包含字母、数字、连字符和下划线",
                ValidationErrorCode::InvalidFormat,
            ));
        }
    }

    /// 验证 sender
    fn validate_sender(&self, sender: &str, result: &mut ValidationResult) {
        // 检查非空
        if sender.is_empty() {
            result.add_error(ValidationError::new(
                "sender",
                "发送者 ID 不能为空",
                ValidationErrorCode::EmptyField,
            ));
            return;
        }

        // 检查长度
        if sender.len() > self.config.max_sender_length {
            result.add_error(ValidationError::new(
                "sender",
                format!(
                    "发送者 ID 长度超过限制（最大 {} 字符）",
                    self.config.max_sender_length
                ),
                ValidationErrorCode::OutOfRange,
            ));
        }

        // 检查是否只包含合法字符
        if !sender
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            result.add_error(ValidationError::new(
                "sender",
                "发送者 ID 只能包含字母、数字、连字符和下划线",
                ValidationErrorCode::InvalidFormat,
            ));
        }
    }

    /// 验证 action
    fn validate_action(&self, action: &str, result: &mut ValidationResult) {
        // 检查非空
        if action.is_empty() {
            result.add_error(ValidationError::new(
                "action",
                "操作名称不能为空",
                ValidationErrorCode::EmptyField,
            ));
            return;
        }

        // 检查长度
        if action.len() > self.config.max_action_length {
            result.add_error(ValidationError::new(
                "action",
                format!(
                    "操作名称长度超过限制（最大 {} 字符）",
                    self.config.max_action_length
                ),
                ValidationErrorCode::OutOfRange,
            ));
            return;
        }

        // 检查格式（namespace.action）
        if !ACTION_REGEX.is_match(action) {
            result.add_error(ValidationError::new(
                "action",
                format!(
                    "操作名称格式无效: '{}'. 期望格式: 'namespace.action', \
                    其中 namespace 使用小写字母、数字和连字符, \
                    action 使用小写字母、数字和下划线",
                    action
                ),
                ValidationErrorCode::InvalidFormat,
            ));
        }
    }

    /// 验证 timestamp
    fn validate_timestamp(
        &self,
        timestamp: &chrono::DateTime<chrono::Utc>,
        result: &mut ValidationResult,
    ) {
        let now = Utc::now();
        let max_past = now - Duration::seconds(self.config.max_timestamp_drift_past_secs);
        let max_future = now + Duration::seconds(self.config.max_timestamp_drift_future_secs);

        if *timestamp < max_past {
            result.add_error(ValidationError::new(
                "timestamp",
                format!(
                    "时间戳过早: {} (允许的最早时间: {})",
                    timestamp, max_past
                ),
                ValidationErrorCode::OutOfRange,
            ));
        } else if *timestamp > max_future {
            result.add_error(ValidationError::new(
                "timestamp",
                format!(
                    "时间戳过晚（超出未来时间限制）: {} (允许的最晚时间: {})",
                    timestamp, max_future
                ),
                ValidationErrorCode::OutOfRange,
            ));
        }
    }
}

impl Default for RequestValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// 验证 action 格式是否正确
///
/// 便捷函数，用于快速检查 action 格式是否符合 `namespace.action` 规范。
///
/// # Arguments
///
/// * `action` - 要验证的 action 字符串
///
/// # Returns
///
/// 返回 `true` 如果格式正确，否则返回 `false`
pub fn is_valid_action_format(action: &str) -> bool {
    ACTION_REGEX.is_match(action)
}

/// 解析 action 为 namespace 和 action 部分
///
/// # Arguments
///
/// * `action` - 要解析的 action 字符串
///
/// # Returns
///
/// 如果格式正确，返回 `Some((namespace, action))`，否则返回 `None`
pub fn parse_action(action: &str) -> Option<(&str, &str)> {
    if !is_valid_action_format(action) {
        return None;
    }
    action.split_once('.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use serde_json::json;

    /// 创建有效的测试请求
    fn create_valid_request() -> RouteRequest {
        RouteRequest::new("test-sender", "card.read", json!({}))
    }

    // ==================== RequestValidator 测试 ====================

    #[test]
    fn test_validator_new() {
        let validator = RequestValidator::new();
        assert_eq!(validator.config().max_request_id_length, 128);
        assert_eq!(validator.config().max_sender_length, 64);
    }

    #[test]
    fn test_validator_with_config() {
        let config = ValidatorConfig {
            max_request_id_length: 64,
            ..Default::default()
        };
        let validator = RequestValidator::with_config(config);
        assert_eq!(validator.config().max_request_id_length, 64);
    }

    // ==================== validate_format 测试 ====================

    #[test]
    fn test_validate_format_valid_request() {
        let validator = RequestValidator::new();
        let request = create_valid_request();
        let result = validator.validate_format(&request);

        assert!(result.is_valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_validate_format_empty_request_id() {
        let validator = RequestValidator::new();
        let mut request = create_valid_request();
        request.request_id = String::new();

        let result = validator.validate_format(&request);

        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].field, "request_id");
        assert_eq!(result.errors[0].code, ValidationErrorCode::EmptyField);
    }

    #[test]
    fn test_validate_format_empty_sender() {
        let validator = RequestValidator::new();
        let mut request = create_valid_request();
        request.sender = String::new();

        let result = validator.validate_format(&request);

        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].field, "sender");
    }

    #[test]
    fn test_validate_format_empty_action() {
        let validator = RequestValidator::new();
        let mut request = create_valid_request();
        request.action = String::new();

        let result = validator.validate_format(&request);

        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].field, "action");
    }

    #[test]
    fn test_validate_format_invalid_action_format() {
        let validator = RequestValidator::new();

        // 测试各种无效的 action 格式
        let invalid_actions = vec![
            "invalid",           // 缺少点号
            "CARD.read",         // 大写 namespace
            "card.READ",         // 大写 action
            ".read",             // 缺少 namespace
            "card.",             // 缺少 action
            "card..read",        // 多个点号
            "card.read.more",    // 太多点号
            "1card.read",        // namespace 以数字开头
            "card.1read",        // action 以数字开头
            "card_mgr.read",     // namespace 包含下划线（应使用连字符）
            "card.read-all",     // action 包含连字符（应使用下划线）
            " card.read",        // 前导空格
            "card.read ",        // 尾随空格
        ];

        for action in invalid_actions {
            let mut request = create_valid_request();
            request.action = action.to_string();

            let result = validator.validate_format(&request);

            assert!(
                !result.is_valid,
                "Action '{}' should be invalid",
                action
            );
            assert!(
                result.errors.iter().any(|e| e.field == "action"),
                "Should have action error for '{}'",
                action
            );
        }
    }

    #[test]
    fn test_validate_format_valid_action_formats() {
        let validator = RequestValidator::new();

        // 测试有效的 action 格式
        let valid_actions = vec![
            "card.read",
            "card.write",
            "file-system.list",
            "file-system.read_all",
            "auth.check_permission",
            "plugin.install",
            "a.b",
            "a1.b2",
            "module-name.action_name",
        ];

        for action in valid_actions {
            let mut request = create_valid_request();
            request.action = action.to_string();

            let result = validator.validate_format(&request);

            assert!(
                result.is_valid,
                "Action '{}' should be valid, but got errors: {:?}",
                action,
                result.errors
            );
        }
    }

    #[test]
    fn test_validate_format_timestamp_too_old() {
        let validator = RequestValidator::new();
        let mut request = create_valid_request();
        // 设置时间戳为 10 分钟前（超过 5 分钟的限制）
        request.timestamp = Utc::now() - Duration::minutes(10);

        let result = validator.validate_format(&request);

        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.field == "timestamp"));
    }

    #[test]
    fn test_validate_format_timestamp_in_future() {
        let validator = RequestValidator::new();
        let mut request = create_valid_request();
        // 设置时间戳为 5 分钟后（超过 60 秒的限制）
        request.timestamp = Utc::now() + Duration::minutes(5);

        let result = validator.validate_format(&request);

        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.field == "timestamp"));
    }

    #[test]
    fn test_validate_format_timestamp_within_drift() {
        let validator = RequestValidator::new();
        let mut request = create_valid_request();
        // 设置时间戳为 30 秒前（在允许范围内）
        request.timestamp = Utc::now() - Duration::seconds(30);

        let result = validator.validate_format(&request);

        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_format_multiple_errors() {
        let validator = RequestValidator::new();
        let mut request = create_valid_request();
        request.request_id = String::new();
        request.sender = String::new();
        request.action = String::new();

        let result = validator.validate_format(&request);

        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 3);
    }

    #[test]
    fn test_validate_format_request_id_too_long() {
        let validator = RequestValidator::new();
        let mut request = create_valid_request();
        request.request_id = "a".repeat(200);

        let result = validator.validate_format(&request);

        assert!(!result.is_valid);
        assert!(result
            .errors
            .iter()
            .any(|e| e.field == "request_id" && e.code == ValidationErrorCode::OutOfRange));
    }

    #[test]
    fn test_validate_format_invalid_request_id_chars() {
        let validator = RequestValidator::new();
        let mut request = create_valid_request();
        request.request_id = "invalid@id!".to_string();

        let result = validator.validate_format(&request);

        assert!(!result.is_valid);
        assert!(result
            .errors
            .iter()
            .any(|e| e.field == "request_id" && e.code == ValidationErrorCode::InvalidFormat));
    }

    // ==================== validate_permission 测试 ====================

    #[test]
    fn test_validate_permission_no_required() {
        let validator = RequestValidator::new();
        let request = create_valid_request();

        let result = validator.validate_permission(&request, &[], &[]);

        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_permission_has_all_required() {
        let validator = RequestValidator::new();
        let request = create_valid_request();

        let required = vec!["card.read".to_string()];
        let sender_perms = vec!["card.read".to_string(), "card.write".to_string()];

        let result = validator.validate_permission(&request, &required, &sender_perms);

        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_permission_missing_permission() {
        let validator = RequestValidator::new();
        let request = create_valid_request();

        let required = vec!["card.write".to_string()];
        let sender_perms = vec!["card.read".to_string()];

        let result = validator.validate_permission(&request, &required, &sender_perms);

        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].code, ValidationErrorCode::PermissionDenied);
    }

    #[test]
    fn test_validate_permission_missing_multiple() {
        let validator = RequestValidator::new();
        let request = create_valid_request();

        let required = vec![
            "card.write".to_string(),
            "card.delete".to_string(),
        ];
        let sender_perms = vec!["card.read".to_string()];

        let result = validator.validate_permission(&request, &required, &sender_perms);

        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 2);
    }

    // ==================== validate (完整验证) 测试 ====================

    #[test]
    fn test_validate_full_success() {
        let validator = RequestValidator::new();
        let request = create_valid_request();

        let required = vec!["card.read".to_string()];
        let sender_perms = vec!["card.read".to_string()];

        let result = validator.validate(&request, &required, &sender_perms);

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_full_format_error() {
        let validator = RequestValidator::new();
        let mut request = create_valid_request();
        request.action = "invalid".to_string();

        let result = validator.validate(&request, &[], &[]);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::InvalidRequest(_)));
    }

    #[test]
    fn test_validate_full_permission_error() {
        let validator = RequestValidator::new();
        let request = create_valid_request();

        let required = vec!["card.delete".to_string()];
        let sender_perms = vec!["card.read".to_string()];

        let result = validator.validate(&request, &required, &sender_perms);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::PermissionDenied(_)));
    }

    // ==================== ValidationResult 测试 ====================

    #[test]
    fn test_validation_result_success() {
        let result = ValidationResult::success();
        assert!(result.is_valid);
        assert!(!result.has_errors());
    }

    #[test]
    fn test_validation_result_failure() {
        let errors = vec![ValidationError::new(
            "test",
            "test error",
            ValidationErrorCode::EmptyField,
        )];
        let result = ValidationResult::failure(errors);

        assert!(!result.is_valid);
        assert!(result.has_errors());
    }

    #[test]
    fn test_validation_result_add_error() {
        let mut result = ValidationResult::success();
        result.add_error(ValidationError::new(
            "test",
            "test error",
            ValidationErrorCode::EmptyField,
        ));

        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_validation_result_into_result_success() {
        let result = ValidationResult::success();
        assert!(result.into_result().is_ok());
    }

    #[test]
    fn test_validation_result_into_result_failure() {
        let errors = vec![ValidationError::new(
            "test",
            "test error",
            ValidationErrorCode::EmptyField,
        )];
        let result = ValidationResult::failure(errors);
        let res = result.into_result();

        assert!(res.is_err());
    }

    // ==================== 便捷函数测试 ====================

    #[test]
    fn test_is_valid_action_format() {
        assert!(is_valid_action_format("card.read"));
        assert!(is_valid_action_format("file-system.list"));
        assert!(is_valid_action_format("auth.check_permission"));

        assert!(!is_valid_action_format("invalid"));
        assert!(!is_valid_action_format("Card.read"));
        assert!(!is_valid_action_format("card.Read"));
    }

    #[test]
    fn test_parse_action() {
        assert_eq!(parse_action("card.read"), Some(("card", "read")));
        assert_eq!(parse_action("file-system.list"), Some(("file-system", "list")));
        assert_eq!(parse_action("auth.check_permission"), Some(("auth", "check_permission")));

        assert_eq!(parse_action("invalid"), None);
        assert_eq!(parse_action("Card.read"), None);
    }

    // ==================== ValidationError 测试 ====================

    #[test]
    fn test_validation_error_display() {
        let error = ValidationError::new("field", "message", ValidationErrorCode::EmptyField);
        let display = format!("{}", error);

        assert!(display.contains("EMPTY_FIELD"));
        assert!(display.contains("field"));
        assert!(display.contains("message"));
    }

    #[test]
    fn test_validation_error_code_display() {
        assert_eq!(format!("{}", ValidationErrorCode::EmptyField), "EMPTY_FIELD");
        assert_eq!(format!("{}", ValidationErrorCode::InvalidFormat), "INVALID_FORMAT");
        assert_eq!(format!("{}", ValidationErrorCode::OutOfRange), "OUT_OF_RANGE");
        assert_eq!(format!("{}", ValidationErrorCode::PermissionDenied), "PERMISSION_DENIED");
    }
}
