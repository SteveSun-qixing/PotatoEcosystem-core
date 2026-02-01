//! 十位 62 进制 ID 生成器
//!
//! 本模块实现符合薯片生态规范的 ID 生成功能。
//! ID 格式：10 位 62 进制字符串（0-9, a-z, A-Z）

use rand::Rng;
use std::time::{SystemTime, UNIX_EPOCH};

/// 62 进制字符集
const BASE62_CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// ID 长度
const ID_LENGTH: usize = 10;

/// 生成 10 位 62 进制 ID
///
/// 使用时间戳 + 随机数组合，确保唯一性
///
/// # Returns
///
/// 返回 10 位 62 进制字符串
///
/// # Example
///
/// ```
/// use chips_core::utils::id::generate_id;
///
/// let id = generate_id();
/// assert_eq!(id.len(), 10);
/// ```
pub fn generate_id() -> String {
    let mut rng = rand::thread_rng();
    
    // 获取当前时间戳（毫秒）
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    // 生成随机数
    let random: u64 = rng.gen();
    
    // 组合时间戳和随机数
    let mut value = timestamp ^ random;
    
    // 转换为 62 进制
    let mut result = Vec::with_capacity(ID_LENGTH);
    for _ in 0..ID_LENGTH {
        let index = (value % 62) as usize;
        result.push(BASE62_CHARS[index]);
        value /= 62;
    }
    
    // 反转得到最终 ID
    result.reverse();
    String::from_utf8(result).unwrap()
}

/// 验证 ID 格式是否有效
///
/// # Arguments
///
/// * `id` - 要验证的 ID 字符串
///
/// # Returns
///
/// 如果 ID 格式有效返回 `true`
///
/// # Example
///
/// ```
/// use chips_core::utils::id::is_valid_id;
///
/// assert!(is_valid_id("a1B2c3D4e5"));
/// assert!(!is_valid_id("invalid"));
/// assert!(!is_valid_id("too-short"));
/// ```
pub fn is_valid_id(id: &str) -> bool {
    // 检查长度
    if id.len() != ID_LENGTH {
        return false;
    }
    
    // 检查每个字符是否在 62 进制字符集中
    id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// 从字符串解析 ID（带验证）
///
/// # Arguments
///
/// * `id` - 要解析的 ID 字符串
///
/// # Returns
///
/// 如果 ID 有效，返回 `Some(String)`，否则返回 `None`
pub fn parse_id(id: &str) -> Option<String> {
    if is_valid_id(id) {
        Some(id.to_string())
    } else {
        None
    }
}

/// 生成 UUID v4 格式的 ID
///
/// 用于请求 ID 等需要全局唯一性的场景
pub fn generate_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_generate_id_length() {
        let id = generate_id();
        assert_eq!(id.len(), ID_LENGTH);
    }

    #[test]
    fn test_generate_id_charset() {
        let id = generate_id();
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_generate_id_uniqueness() {
        let mut ids = HashSet::new();
        for _ in 0..1000 {
            let id = generate_id();
            assert!(ids.insert(id), "ID collision detected");
        }
    }

    #[test]
    fn test_is_valid_id() {
        // 有效 ID
        assert!(is_valid_id("a1B2c3D4e5"));
        assert!(is_valid_id("0000000000"));
        assert!(is_valid_id("zzzzzzzzzz"));
        assert!(is_valid_id("ZZZZZZZZZZ"));

        // 无效 ID - 长度错误
        assert!(!is_valid_id("short"));
        assert!(!is_valid_id("toolongstring"));
        assert!(!is_valid_id(""));

        // 无效 ID - 包含非法字符
        assert!(!is_valid_id("a1B2c3D4e!"));
        assert!(!is_valid_id("a1B2c3D4e "));
        assert!(!is_valid_id("a1B2c3-4e5"));
    }

    #[test]
    fn test_parse_id() {
        assert!(parse_id("a1B2c3D4e5").is_some());
        assert!(parse_id("invalid").is_none());
    }

    #[test]
    fn test_generate_uuid() {
        let uuid = generate_uuid();
        assert_eq!(uuid.len(), 36); // UUID v4 格式长度
        assert!(uuid.contains('-'));
    }
}
