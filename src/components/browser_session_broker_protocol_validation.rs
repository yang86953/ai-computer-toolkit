//! 定义 browser session broker parser 使用的窄、无副作用输入验证。

// 导入标准 IPv6 解析器。
use std::net::Ipv6Addr;
// 导入字符串解析 trait。
use std::str::FromStr;

// 验证仅允许公开 HTTP(S) URL 且必须存在真实 host。
pub(super) fn http_url(value: &str) -> bool {
    // 先实施总长度与禁止字符门禁。
    if value.is_empty()
        // 限制 schema 冻结的最大长度。
        || value.chars().count() > 8_192
        // 禁止 userinfo 以及路径中的身份歧义。
        || value.contains('@')
        // 禁止 Windows 路径与 URL 分隔歧义。
        || value.contains('\\')
        // 禁止所有控制字符。
        || value.chars().any(char::is_control)
        // 禁止所有空白字符。
        || value.chars().any(char::is_whitespace)
    {
        // 任一边界失败都拒绝。
        return false;
    }
    // 只允许固定 HTTP 或 HTTPS scheme。
    let Some(after_scheme) = value
        // 尝试剥离 HTTP scheme。
        .strip_prefix("http://")
        // 或剥离 HTTPS scheme。
        .or_else(|| value.strip_prefix("https://"))
    else {
        // 拒绝其他 scheme 或大小写猜测。
        return false;
    };
    // authority 在首个 path、query 或 fragment 分隔符前结束。
    let authority = after_scheme
        // 按三个公开分隔符切分。
        .split(['/', '?', '#'])
        // 取得唯一首段。
        .next()
        // 理论空值保守映射为空。
        .unwrap_or_default();
    // authority 必须解析出真实 host 和可选端口。
    valid_authority(authority)
}

// 验证 host[:port] 或 [ipv6][:port] authority。
fn valid_authority(authority: &str) -> bool {
    // 空 authority 永远不是合法网络目标。
    if authority.is_empty() {
        // 拒绝 http:// 与 http:///path。
        return false;
    }
    // 方括号形式只允许真实 IPv6 literal。
    if let Some(bracketed) = authority.strip_prefix('[') {
        // 必须存在唯一右括号。
        let Some(closing) = bracketed.find(']') else {
            // 拒绝未闭合 IPv6。
            return false;
        };
        // 取得括号内 host。
        let host = &bracketed[..closing];
        // 取得右括号后的可选端口。
        let suffix = &bracketed[closing + 1..];
        // 必须由标准库完整解析 IPv6。
        return Ipv6Addr::from_str(host).is_ok()
            // suffix 只能为空或合法 :port。
            && (suffix.is_empty()
                // 验证端口前缀与数值。
                || suffix.strip_prefix(':').is_some_and(valid_port));
    }
    // 非括号 authority 不允许裸 IPv6 的多个冒号。
    if authority.bytes().filter(|byte| *byte == b':').count() > 1 {
        // 要求 IPv6 使用方括号消除端口歧义。
        return false;
    }
    // 拆出可选端口。
    let (host, port) = authority
        // 从右侧查找唯一冒号。
        .rsplit_once(':')
        // 有冒号时保存端口。
        .map_or((authority, None), |(host, port)| (host, Some(port)));
    // host 必须是受限 DNS/IPv4 文本，端口必须可解析。
    valid_dns_or_ipv4_host(host)
        // 无端口或端口合法。
        && port.is_none_or(valid_port)
}

// 验证受限 ASCII DNS 名或 IPv4 literal。
fn valid_dns_or_ipv4_host(host: &str) -> bool {
    // DNS 总长度必须有界且非空。
    if host.is_empty() || host.len() > 253 {
        // 拒绝空 host 与超长 host。
        return false;
    }
    // 每个 label 都必须非空且由安全 ASCII 组成。
    host.split('.').all(|label| {
        // label 长度遵守 DNS 边界。
        !label.is_empty()
            // 单个 label 不超过 63 字节。
            && label.len() <= 63
            // 首字符必须是 ASCII 字母或数字。
            && label
                // 读取首字节。
                .as_bytes()
                // 取得首字符。
                .first()
                // 验证字母数字。
                .is_some_and(u8::is_ascii_alphanumeric)
            // 末字符也必须是 ASCII 字母或数字。
            && label
                // 读取末字节。
                .as_bytes()
                // 取得末字符。
                .last()
                // 验证字母数字。
                .is_some_and(u8::is_ascii_alphanumeric)
            // 中间只允许字母、数字或连字符。
            && label
                // 逐字节扫描。
                .bytes()
                // 拒绝下划线、百分号和其他歧义字符。
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

// 验证非空十进制 u16 端口。
fn valid_port(port: &str) -> bool {
    // 端口必须非空且只含 ASCII 数字。
    !port.is_empty()
        // 逐字节检查数字。
        && port.bytes().all(|byte| byte.is_ascii_digit())
        // 数值必须落入 u16。
        && port.parse::<u16>().is_ok()
}
