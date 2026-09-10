//! 提供浏览器会话公开身份的严格无状态分类。

// 声明浏览器会话身份的封闭公开形状。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionIdentityShape {
    // 表示 canonical s2:bs 浏览器会话身份。
    Canonical,
    // 表示其他已知或未知的非浏览器会话身份。
    NotBrowserSession,
    // 表示使用 s2:bs 前缀但不满足严格 canonical 形状的身份。
    Malformed,
}

// 分类调用方提供的公开会话身份而不解析任何原生对象。
pub(crate) fn classify_browser_session_id(value: &str) -> BrowserSessionIdentityShape {
    // 只有浏览器会话前缀需要进入严格形状检查。
    let Some(suffix) = value.strip_prefix("s2:bs:") else {
        // 其他目标绝不作为浏览器会话交给 broker。
        return BrowserSessionIdentityShape::NotBrowserSession;
    };
    // browser session 固定为三十二位小写十六进制随机身份。
    if suffix.len() == 32
        // 禁止大小写别名、空白和其他宽松字符。
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        // 返回唯一允许进入固定 broker 的公开身份。
        BrowserSessionIdentityShape::Canonical
    } else {
        // 同前缀的错误形状不得降级成其他 target。
        BrowserSessionIdentityShape::Malformed
    }
}

// 验证浏览器会话身份 Component 的公开边界。
#[cfg(test)]
mod tests {
    // 导入分类器与封闭形状。
    use super::{BrowserSessionIdentityShape, classify_browser_session_id};

    // 验证 browser_session_public_route 身份分类拒绝通用 opaque 回退。
    #[test]
    fn browser_session_public_route_identity_is_strict_and_non_generic() {
        // 唯一合法浏览器会话身份必须通过。
        assert_eq!(
            // 分类固定合法身份。
            classify_browser_session_id("s2:bs:0123456789abcdef0123456789abcdef"),
            // 只接受 canonical 类别。
            BrowserSessionIdentityShape::Canonical,
        );
        // 普通 opaque 窗口不得被误作浏览器会话。
        assert_eq!(
            // 分类其他 target。
            classify_browser_session_id("s2:w:0123456789abcdef"),
            // 保持非浏览器会话类别。
            BrowserSessionIdentityShape::NotBrowserSession,
        );
        // 同前缀但大写的别名必须严格拒绝。
        assert_eq!(
            // 分类非 canonical browser 前缀。
            classify_browser_session_id("s2:bs:0123456789ABCDEF0123456789abcdef"),
            // 保持 malformed 类别。
            BrowserSessionIdentityShape::Malformed,
        );
    }
}
