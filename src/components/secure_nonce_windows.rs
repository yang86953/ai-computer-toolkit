//! 使用 Windows 系统随机源生成不携带身份事实的一次性 nonce。

// 导入 Windows CNG 系统首选随机源。
use windows::Win32::Security::Cryptography::{
    // 导入无需显式算法 provider 的系统随机标志。
    BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    // 导入系统随机函数。
    BCryptGenRandom,
};

// 导入工具统一错误类型。
use crate::domain::{AppControlError, AppResult};

// 固定一次性 nonce 为 128 位。
const NONCE_BYTES: usize = 16;

// 生成 canonical 32 位小写十六进制随机 nonce。
pub(crate) fn random_nonce() -> AppResult<String> {
    // 初始化不会携带进程、时间或 session 信息的随机缓冲区。
    let mut bytes = [0_u8; NONCE_BYTES];
    // 使用系统首选 CNG 随机源填充完整缓冲区。
    let status = unsafe {
        BCryptGenRandom(
            // 系统首选随机源不需要显式算法 handle。
            None,
            // 传入固定长度可变缓冲区。
            &mut bytes,
            // 要求操作系统选择已认证 provider。
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    // 任何非成功 NTSTATUS 都必须失败闭合。
    if status.is_err() {
        // 不公开原生状态值或本机随机 provider。
        return Err(AppControlError::new(
            // 使用稳定 endpoint 认证错误。
            "ENDPOINT_AUTHENTICATION_FAILED",
            // 提供不含平台细节的安全消息。
            "A cryptographically random endpoint nonce could not be generated.",
        ));
    }
    // 预分配固定 32 字节 ASCII 输出。
    let mut encoded = String::with_capacity(NONCE_BYTES.saturating_mul(2));
    // 逐字节输出两个小写十六进制字符。
    for byte in bytes {
        // 使用稳定小写格式且不引入分隔符。
        use std::fmt::Write as _;
        // 写入内存 String 理论上不会失败。
        if write!(&mut encoded, "{byte:02x}").is_err() {
            // 防御性返回相同封闭错误。
            return Err(AppControlError::new(
                // 保持唯一稳定错误码。
                "ENDPOINT_AUTHENTICATION_FAILED",
                // 不公开内部格式化状态。
                "A cryptographically random endpoint nonce could not be encoded.",
            ));
        }
    }
    // 返回不含身份事实的 canonical nonce。
    Ok(encoded)
}

// 声明系统随机 nonce 的形状回归。
#[cfg(test)]
mod tests {
    // 导入被测随机函数。
    use super::random_nonce;

    // 验证两次 nonce 都 canonical 且通常不相等。
    #[test]
    fn random_nonces_are_canonical_and_distinct() {
        // 生成第一条随机 nonce。
        let first = random_nonce()
            // 当前 Windows 测试环境必须提供系统随机源。
            .unwrap_or_else(|error| panic!("first random nonce failed: {error}"));
        // 生成第二条随机 nonce。
        let second = random_nonce()
            // 当前 Windows 测试环境必须提供系统随机源。
            .unwrap_or_else(|error| panic!("second random nonce failed: {error}"));
        // 两条值都固定为 32 位小写十六进制。
        for value in [&first, &second] {
            // 固定长度为 128 位十六进制文本。
            assert_eq!(value.len(), 32);
            // 只接受小写十六进制字符。
            assert!(
                value
                    // 遍历 ASCII 字节。
                    .bytes()
                    // 验证完整字符集。
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            );
        }
        // 在一次测试中重复值代表随机源不符合使用要求。
        assert_ne!(first, second);
    }
}
