//! 定义持久窗口代际 broker 的严格私有 JSON 协议。

// 导入有序集合以拒绝一次快照内重复 token。
use std::collections::BTreeSet;

// 导入严格 JSON 反序列化派生。
use serde::Deserialize;
// 导入 JSON 值以区分字段缺失与显式 null。
use serde_json::Value;

// 固定窗口代际 broker 协议版本。
pub(crate) const CONTRACT_VERSION: &str = "act/window-generation-broker/v1";
// 固定同安装构建身份且不包含路径或用户信息。
pub(crate) const BROKER_BUILD_ID: &str = concat!(
    // 绑定主产品包名。
    env!("CARGO_PKG_NAME"),
    // 使用稳定分隔符。
    "/",
    // 绑定包版本。
    env!("CARGO_PKG_VERSION"),
    // 绑定当前 broker 角色。
    "/window-generation-broker/v1"
);
// 固定请求 nonce 与随机 broker epoch 的小写十六进制长度。
const NONCE_HEX_LENGTH: usize = 32;
// 固定窗口 token、进程代际与 owner generation 的十六进制长度。
const FACT_HEX_LENGTH: usize = 16;
// 限制一次 resolve snapshot 的窗口数量。
pub(crate) const MAXIMUM_SNAPSHOT_WINDOWS: usize = 16_384;
// 限制 broker 同时持有的 live 窗口记录。
pub(crate) const MAXIMUM_TRACKED_WINDOWS: usize = 65_536;
// 限制 Adapter 到 Module 的待处理事件数量。
pub(crate) const MAXIMUM_EVENT_QUEUE: usize = 65_536;

// 表示 broker 接受的封闭只读操作。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
// 使用 kebab-case 对齐私有 JSON 协议。
#[serde(rename_all = "kebab-case")]
pub(crate) enum WindowGenerationBrokerOperation {
    // 查询 owner 健康与序列屏障。
    Health,
    // 解析一批当前窗口事实到 owner generation。
    ResolveSnapshot,
}

// 表示严格解析后的随机 broker epoch。
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct BrokerEpoch(String);

// 为 broker epoch 提供唯一规范化构造。
impl BrokerEpoch {
    // 只接受 128 位小写十六进制文本。
    pub(crate) fn parse(value: &str) -> Option<Self> {
        // 拒绝长度、大小写或非十六进制字符漂移。
        is_fixed_lower_hex(value, NONCE_HEX_LENGTH).then(|| Self(value.to_owned()))
    }

    // 借用 canonical 私有 epoch。
    pub(crate) fn as_str(&self) -> &str {
        // 不向公共 JSON 投影该值。
        &self.0
    }
}

// 表示 resolve 请求中的一条当前窗口私有事实。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct WindowGenerationCandidate {
    // 保存当前私有进程 ID。
    process_id: u32,
    // 保存完整当前窗口 token。
    current_window_token: u64,
    // 保存进程创建 FILETIME。
    process_generation: u64,
}

// 为候选事实提供封闭构造与只读投影。
impl WindowGenerationCandidate {
    // 构造全部非零的候选事实。
    pub(crate) const fn new(
        // 接收私有进程 ID。
        process_id: u32,
        // 接收完整当前窗口 token。
        current_window_token: u64,
        // 接收进程创建代际。
        process_generation: u64,
    ) -> Option<Self> {
        // 任一事实缺失都不能签发绝对身份。
        if process_id == 0 || current_window_token == 0 || process_generation == 0 {
            // 返回失败闭合结果。
            return None;
        }
        // 保存完整候选。
        Some(Self {
            // 保存 PID。
            process_id,
            // 保存 token。
            current_window_token,
            // 保存进程代际。
            process_generation,
        })
    }

    // 返回私有进程 ID。
    pub(crate) const fn process_id(self) -> u32 {
        // 复制整数事实。
        self.process_id
    }

    // 返回完整当前窗口 token。
    pub(crate) const fn current_window_token(self) -> u64 {
        // 复制整数事实。
        self.current_window_token
    }

    // 返回进程创建代际。
    pub(crate) const fn process_generation(self) -> u64 {
        // 复制整数事实。
        self.process_generation
    }
}

// 表示协议解析允许返回的封闭失败分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowGenerationProtocolErrorCode {
    // JSON、版本、nonce、epoch 或窗口事实不合法。
    InvalidArgument,
    // 请求期待的 broker epoch 已经过期。
    StaleBrokerEpoch,
    // 事件 owner 尚未建立或已经 poison。
    OwnerUnavailable,
    // registry 或事件队列达到固定容量。
    OwnerCapacityExhausted,
}

// 为失败分类提供稳定私有 wire 文本。
impl WindowGenerationProtocolErrorCode {
    // 返回稳定大写错误码。
    pub(crate) const fn as_str(self) -> &'static str {
        // 穷举封闭失败集合。
        match self {
            // 映射参数错误。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射 broker 换代。
            Self::StaleBrokerEpoch => "STALE_BROKER_EPOCH",
            // 映射 owner 不可用。
            Self::OwnerUnavailable => "WINDOW_GENERATION_OWNER_UNAVAILABLE",
            // 映射固定容量耗尽。
            Self::OwnerCapacityExhausted => "WINDOW_GENERATION_OWNER_CAPACITY_EXHAUSTED",
        }
    }

    // 返回不泄漏原生目标或本机 endpoint 的固定消息。
    pub(crate) const fn message(self) -> &'static str {
        // 按分类选择安全消息。
        match self {
            // 描述严格协议拒绝。
            Self::InvalidArgument => "The window generation broker request violates protocol v1.",
            // 描述 epoch 变化。
            Self::StaleBrokerEpoch => "The window generation broker epoch is stale.",
            // 描述 owner 无法证明连续性。
            Self::OwnerUnavailable => "The window generation owner is unavailable.",
            // 描述固定资源上限。
            Self::OwnerCapacityExhausted => {
                "The window generation owner reached its fixed capacity."
            }
        }
    }
}

// 表示协议解析失败且不回显不可信输入。
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct WindowGenerationProtocolFailure {
    // 保存封闭错误分类。
    code: WindowGenerationProtocolErrorCode,
}

// 为解析失败提供只读投影。
impl WindowGenerationProtocolFailure {
    // 构造不携带输入内容的失败。
    fn invalid() -> Self {
        // 固定参数错误。
        Self {
            // 保存封闭分类。
            code: WindowGenerationProtocolErrorCode::InvalidArgument,
        }
    }

    // 返回封闭错误分类。
    pub(crate) const fn code(&self) -> WindowGenerationProtocolErrorCode {
        // 复制无状态枚举。
        self.code
    }
}

// 表示 wire 上的一条未验证窗口事实。
#[derive(Debug, Deserialize)]
// 固定 camelCase 并拒绝未知字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireWindowCandidate {
    // 接收私有 PID。
    process_id: u32,
    // 接收固定宽度窗口 token。
    current_window_token: String,
    // 接收固定宽度进程代际。
    process_generation: String,
}

// 表示 wire 上的未验证 broker 请求。
#[derive(Debug, Deserialize)]
// 固定 camelCase 并拒绝未知字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireBrokerRequest {
    // 接收固定协议版本。
    contract_version: String,
    // 接收一次性请求关联值。
    request_nonce: String,
    // 接收 ready frame 发布的期望 epoch。
    expected_broker_epoch: String,
    // 接收封闭操作。
    operation: WindowGenerationBrokerOperation,
    // 仅 resolve-snapshot 接受窗口数组。
    windows: Option<Vec<WireWindowCandidate>>,
}

// 表示完全验证且尚未触碰 owner 的 broker 请求。
#[derive(Debug)]
pub(crate) struct WindowGenerationBrokerRequest {
    // 保存 canonical 请求 nonce。
    request_nonce: String,
    // 保存 canonical 期望 epoch。
    expected_broker_epoch: BrokerEpoch,
    // 保存封闭操作。
    operation: WindowGenerationBrokerOperation,
    // 保存严格去重的窗口事实。
    windows: Vec<WindowGenerationCandidate>,
}

// 为 broker 请求提供严格 parser 与只读投影。
impl WindowGenerationBrokerRequest {
    // 解析一条完整 UTF-8 JSON frame。
    pub(crate) fn parse(text: &str) -> Result<Self, WindowGenerationProtocolFailure> {
        // 只兼容单个 UTF-8 BOM 与外围空白。
        let normalized = text.trim_start_matches('\u{feff}').trim();
        // 先解析 Value 以区分字段缺失和显式 null。
        let value = serde_json::from_str::<Value>(normalized)
            // 任意 JSON 失败都不回显输入。
            .map_err(|_| WindowGenerationProtocolFailure::invalid())?;
        // 根必须是对象。
        let object = value
            // 取得对象引用。
            .as_object()
            // 非对象失败闭合。
            .ok_or_else(WindowGenerationProtocolFailure::invalid)?;
        // 记录 windows 字段是否真实存在。
        let has_windows = object.contains_key("windows");
        // 显式 null 不等于字段缺失。
        if object.get("windows").is_some_and(Value::is_null) {
            // 拒绝模糊形状。
            return Err(WindowGenerationProtocolFailure::invalid());
        }
        // 再用 deny_unknown_fields 解析强类型。
        let wire = serde_json::from_value::<WireBrokerRequest>(value)
            // 不保留底层 serde 诊断。
            .map_err(|_| WindowGenerationProtocolFailure::invalid())?;
        // 固定版本不得协商。
        if wire.contract_version != CONTRACT_VERSION
            // nonce 必须是 canonical 128 位小写十六进制。
            || !is_fixed_lower_hex(&wire.request_nonce, NONCE_HEX_LENGTH)
        {
            // 拒绝不可信 envelope。
            return Err(WindowGenerationProtocolFailure::invalid());
        }
        // 解析 canonical epoch。
        let expected_broker_epoch = BrokerEpoch::parse(&wire.expected_broker_epoch)
            // 非 canonical epoch 失败闭合。
            .ok_or_else(WindowGenerationProtocolFailure::invalid)?;
        // 按操作验证 windows 字段的存在性。
        match wire.operation {
            // health 不允许携带候选数组。
            WindowGenerationBrokerOperation::Health if has_windows => {
                // 拒绝扩展语义。
                return Err(WindowGenerationProtocolFailure::invalid());
            }
            // resolve-snapshot 必须显式携带数组，空数组合法。
            WindowGenerationBrokerOperation::ResolveSnapshot if !has_windows => {
                // 拒绝缺失快照。
                return Err(WindowGenerationProtocolFailure::invalid());
            }
            // 其余字段组合合法。
            _ => {}
        }
        // 取得候选数组或 health 的空集合。
        let wire_windows = wire.windows.unwrap_or_default();
        // 拒绝无界快照。
        if wire_windows.len() > MAXIMUM_SNAPSHOT_WINDOWS {
            // 固定容量不接受截断。
            return Err(WindowGenerationProtocolFailure::invalid());
        }
        // 建立 token 去重集合。
        let mut tokens = BTreeSet::new();
        // 预分配已验证候选。
        let mut windows = Vec::with_capacity(wire_windows.len());
        // 验证每条私有事实。
        for candidate in wire_windows {
            // 解析完整当前窗口 token。
            let current_window_token = parse_fixed_hex_u64(&candidate.current_window_token)
                // 非 canonical token 失败闭合。
                .ok_or_else(WindowGenerationProtocolFailure::invalid)?;
            // 解析进程创建 FILETIME。
            let process_generation = parse_fixed_hex_u64(&candidate.process_generation)
                // 非 canonical 代际失败闭合。
                .ok_or_else(WindowGenerationProtocolFailure::invalid)?;
            // 构造全部非零候选。
            let candidate = WindowGenerationCandidate::new(
                // 传入 PID。
                candidate.process_id,
                // 传入 token。
                current_window_token,
                // 传入进程代际。
                process_generation,
            )
            // 缺失事实失败闭合。
            .ok_or_else(WindowGenerationProtocolFailure::invalid)?;
            // 同一快照 token 必须唯一。
            if !tokens.insert(current_window_token) {
                // 不允许任取重复候选。
                return Err(WindowGenerationProtocolFailure::invalid());
            }
            // 保存已验证候选。
            windows.push(candidate);
        }
        // 返回严格请求。
        Ok(Self {
            // 保存请求 nonce。
            request_nonce: wire.request_nonce,
            // 保存 epoch。
            expected_broker_epoch,
            // 保存操作。
            operation: wire.operation,
            // 保存候选。
            windows,
        })
    }

    // 返回 canonical 请求 nonce。
    pub(crate) fn request_nonce(&self) -> &str {
        // 借用私有文本。
        &self.request_nonce
    }

    // 返回期望 broker epoch。
    pub(crate) const fn expected_broker_epoch(&self) -> &BrokerEpoch {
        // 借用强类型 epoch。
        &self.expected_broker_epoch
    }

    // 返回封闭操作。
    pub(crate) const fn operation(&self) -> WindowGenerationBrokerOperation {
        // 复制无状态枚举。
        self.operation
    }

    // 返回严格候选数组。
    pub(crate) fn windows(&self) -> &[WindowGenerationCandidate] {
        // 借用固定顺序快照。
        &self.windows
    }
}

// 判断文本是否为固定宽度小写十六进制。
fn is_fixed_lower_hex(value: &str, length: usize) -> bool {
    // 长度必须逐字相等且字符只允许数字或小写 a-f。
    value.len() == length
        // 验证全部 ASCII 字节。
        && value
            // 遍历字节避免 Unicode 大小写歧义。
            .bytes()
            // 逐字判断合法字符。
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// 解析固定 64 位小写十六进制事实。
fn parse_fixed_hex_u64(value: &str) -> Option<u64> {
    // 先冻结宽度和字符集。
    if !is_fixed_lower_hex(value, FACT_HEX_LENGTH) {
        // 非 canonical 文本失败闭合。
        return None;
    }
    // 解析完整 64 位事实。
    u64::from_str_radix(value, 16).ok()
}

// 协议纯函数回归保留在独立文件以控制代码行数。
#[cfg(test)]
#[path = "window_generation_protocol_tests.rs"]
mod tests;
