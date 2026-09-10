//! 验证 sequence execution 持久身份、nonce 与 SHA-256 文本外壳。

// 固定 execution identity 前缀。
const EXECUTION_PREFIX: &str = "s2:q:";
// 固定 step identity 前缀。
const STEP_PREFIX: &str = "s2:qs:";

// 从 canonical execution identity 借用 32 位指纹。
pub(crate) fn execution_fingerprint(value: &str) -> Option<&str> {
    // 剥离固定前缀并验证完整随机主体。
    value
        .strip_prefix(EXECUTION_PREFIX)
        .filter(|body| is_nonce(body))
}

// 从 canonical step identity 借用 32 位指纹。
pub(crate) fn step_fingerprint(value: &str) -> Option<&str> {
    // 剥离固定前缀并验证完整随机主体。
    value
        .strip_prefix(STEP_PREFIX)
        .filter(|body| is_nonce(body))
}

// 从 canonical 32 位指纹建立 execution identity。
pub(crate) fn execution_id_from_fingerprint(fingerprint: &str) -> Option<String> {
    // 只组合已经通过 canonical 校验的主体。
    is_nonce(fingerprint).then(|| format!("{EXECUTION_PREFIX}{fingerprint}"))
}

// 验证 32 位小写十六进制 nonce。
pub(crate) fn is_nonce(value: &str) -> bool {
    // 固定长度且只接受小写十六进制。
    value.len() == 32
        && value
            // 遍历字节。
            .bytes()
            // 验证字符集。
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// 验证 64 位小写十六进制 SHA-256 文本。
pub(crate) fn is_digest(value: &str) -> bool {
    // 固定长度且只接受小写十六进制。
    value.len() == 64
        && value
            // 遍历字节。
            .bytes()
            // 验证字符集。
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// 声明 canonical 文本边界测试。
#[cfg(test)]
mod tests {
    // 导入被测私有实现。
    use super::*;

    // 验证每类文本只接受无别名 canonical 外壳。
    #[test]
    fn identities_and_digests_reject_aliases() {
        // canonical execution identity 返回稳定指纹。
        assert_eq!(
            execution_fingerprint("s2:q:0123456789abcdef0123456789abcdef"),
            Some("0123456789abcdef0123456789abcdef")
        );
        // step identity 不得混入 execution 类别。
        assert_eq!(
            step_fingerprint("s2:q:0123456789abcdef0123456789abcdef"),
            None
        );
        // 大写十六进制不是 canonical nonce。
        assert!(!is_nonce("0123456789ABCDEF0123456789ABCDEF"));
        // SHA-256 文本必须精确为 64 位。
        assert!(!is_digest("abcdef"));
    }
}
