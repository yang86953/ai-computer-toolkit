//! 为语言中立契约提供稳定的 FNV-1a 64 位字节摘要。

// 定义标准 FNV-1a 64 位偏移基数。
const FNV_OFFSET_BASIS: u64 = 14_695_981_039_346_656_037;
// 定义标准 FNV-1a 64 位质数。
const FNV_PRIME: u64 = 1_099_511_628_211;

// 把任意字节切片投影为固定十六位小写摘要。
pub(crate) fn digest(bytes: &[u8]) -> String {
    // 从标准偏移基数开始累积。
    let mut value = FNV_OFFSET_BASIS;
    // 按字节顺序执行 FNV-1a。
    for byte in bytes {
        // 先异或当前字节。
        value ^= u64::from(*byte);
        // 再执行无符号回绕乘法。
        value = value.wrapping_mul(FNV_PRIME);
    }
    // 输出固定宽度的小写十六进制文本。
    format!("{value:016x}")
}

// 声明纯 Component 单元测试。
#[cfg(test)]
mod tests {
    // 导入待测摘要入口。
    use super::digest;

    // 验证标准空输入 golden。
    #[test]
    fn empty_digest_matches_fnv1a_golden() {
        // 核对标准偏移基数文本。
        assert_eq!(digest(&[]), "cbf29ce484222325");
    }

    // 验证固定字节序不会漂移。
    #[test]
    fn byte_order_is_stable() {
        // 核对独立计算的 hello golden。
        assert_eq!(digest(b"hello"), "a430d84680aabd0b");
    }
}
