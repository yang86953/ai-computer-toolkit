#[cfg(not(target_os = "windows"))]
fn main() {
    // Windows 专用 fixture 在 Linux 上只返回结构化能力缺口。
    println!(
        "{}",
        r#"{"ok":false,"error":{"code":"CAPABILITY_UNAVAILABLE","message":"This fixture requires a certified Windows provider.","details":{"platform":"linux","executionRealm":"none","fallback":"none"}}}"#
    );
    std::process::exit(2);
}

#[cfg(target_os = "windows")]
mod windows_fixture {
    //! 无参数生产 worker 固定 sibling 替身，仅供 broker transport 集成测试复制使用。

    // 导入有界 JSON Lines 读写。
    use std::io::{BufRead, Write};

    // 导入 JSON 值与构造宏。
    use serde_json::{Value, json};
    // 导入 Windows 系统首选 CNG 随机源。
    use windows::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };

    // 固定真实 worker 协议版本。
    const CONTRACT_VERSION: &str = "act/browser-session-worker/v1";
    // 固定真实页面命令协议版本。
    const PAGE_CONTRACT_VERSION: &str = "act/browser-page-command-worker/v1";
    // 固定一次性 request nonce 长度。
    const NONCE_LENGTH: usize = 32;
    // 固定真实 worker 单行输入资源上限。
    const MAXIMUM_INPUT_BYTES: usize = 8 * 1024;
    // 固定 accepted 到 ready 的测试观察门控时长。
    const ACCEPTED_READY_GATE: std::time::Duration = std::time::Duration::from_millis(250);
    // 固定 fixture 私有 page ref。
    const PAGE_REF: &str = "w1:bp:11111111111111111111111111111111";
    // 固定 fixture 私有 element ref。
    const ELEMENT_REF: &str = "w1:be:22222222222222222222222222222222";

    // 验证 JSON 对象只含冻结字段集合。
    fn exact_keys(value: &Value, expected: &[&str]) -> bool {
        // 只接受 JSON 对象。
        let Some(object) = value.as_object() else {
            // 非对象不能成为协议帧。
            return false;
        };
        // 字段数量和名称必须同时精确匹配。
        object.len() == expected.len()
        // 禁止任何未知或遗漏字段。
        && object
            .keys()
            .all(|key| expected.contains(&key.as_str()))
    }

    // 向 stdout 写入并 flush 一条严格 worker frame。
    fn write_frame(frame: &Value) -> bool {
        // 序列化固定 JSON frame。
        let Ok(text) = serde_json::to_string(frame) else {
            // 序列化失败不能继续协议。
            return false;
        };
        // 取得 stdout 唯一写锁。
        let stdout = std::io::stdout();
        // 锁定输出以避免 frame 交错。
        let mut output = stdout.lock();
        // 写入一行并立刻 flush 给真实 parent reader。
        writeln!(output, "{text}").is_ok() && output.flush().is_ok()
    }

    // 验证 worker open request nonce 的 canonical 小写十六进制形状。
    fn canonical_nonce(value: &str) -> bool {
        // 只接受 128 位小写十六进制值。
        value.len() == NONCE_LENGTH
        // 拒绝大小写、分隔符、空白和非 ASCII 字符。
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }

    // 从真实 worker open frame 读取并验证 request nonce。
    fn read_open_nonce(
        // 可变借用 worker 全生命周期唯一输入 reader。
        input: &mut impl BufRead,
    ) -> Option<String> {
        // 保存首条 JSON Lines frame。
        let mut line = String::new();
        // EOF 不能建立 worker session。
        if input.read_line(&mut line).ok()? == 0 {
            // 关闭失败。
            return None;
        }
        // 拒绝超过生产协议资源边界的输入。
        if line.len() > MAXIMUM_INPUT_BYTES {
            // 超限输入不能进入字段解析。
            return None;
        }
        // 解析唯一 JSON 值。
        let value = serde_json::from_str::<Value>(line.trim_end()).ok()?;
        // open 根对象必须只含生产协议五个字段。
        if !exact_keys(
            // 核对完整 open frame。
            &value,
            // 冻结字段顺序只用于审阅，JSON 本身不依赖顺序。
            &[
                "kind",
                "contractVersion",
                "requestNonce",
                "timeoutMs",
                "source",
            ],
        ) {
            // 拒绝 parent wire 扩展或缺失。
            return None;
        }
        // 首条输入必须为固定 open 与真实协议版本。
        if value.get("kind").and_then(Value::as_str) != Some("open")
            || value.get("contractVersion").and_then(Value::as_str) != Some(CONTRACT_VERSION)
        {
            // 拒绝协议漂移。
            return None;
        }
        // deadline 必须保持生产协议冻结闭区间。
        if !matches!(
            value.get("timeoutMs").and_then(Value::as_u64),
            Some(1..=30_000)
        ) {
            // 拒绝缺失、浮点、负数和越界预算。
            return None;
        }
        // source 必须存在且保持隔离 profile 的封闭空对象。
        let source = value.get("source")?;
        // source 只允许唯一 kind 字段。
        if !exact_keys(source, &["kind"])
        // 只接受生产 broker 当前授权的隔离 profile。
        || source.get("kind").and_then(Value::as_str) != Some("isolated-profile")
        {
            // 任意 endpoint、profile 参数或未知来源失败闭合。
            return None;
        }
        // 读取并复制关联 nonce。
        let nonce = value
            .get("requestNonce")
            .and_then(Value::as_str)?
            .to_owned();
        // 只接受真实 parent 生成的 canonical nonce。
        canonical_nonce(&nonce).then_some(nonce)
    }

    // 使用 OS CNG 生成与 request nonce 无关的 opaque session ID。
    fn random_session_id() -> Option<String> {
        // 初始化 128 位随机缓冲区。
        let mut bytes = [0_u8; 16];
        // 用系统首选 CNG provider 填满缓冲区。
        if unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.is_err() {
            // 随机失败不能降级确定性 session ID。
            return None;
        }
        // 创建固定公开前缀。
        let mut session_id = String::from("s2:bs:");
        // 编码所有随机字节。
        for byte in bytes {
            // 导入 String 格式化 trait。
            use std::fmt::Write as _;
            // 编码失败时拒绝生成 identity。
            if write!(&mut session_id, "{byte:02x}").is_err() {
                // String 格式化异常失败闭合。
                return None;
            }
        }
        // 返回唯一公开 opaque ID。
        Some(session_id)
    }

    // 构造真实协议 accepted frame。
    fn accepted(nonce: &str) -> Value {
        // 返回与 parent parser 对齐的 accepted shape。
        json!({
            // 声明 accepted kind。
            "kind": "open-accepted",
            // 回显协议版本。
            "contractVersion": CONTRACT_VERSION,
            // 回显 canonical request nonce。
            "requestNonce": nonce,
            // 声明 worker 已接受 dispatch。
            "dispatchAccepted": true,
            // accepted 还不是终态。
            "completed": false,
        })
    }

    // 构造真实协议 ready final frame。
    fn ready(nonce: &str, session_id: &str) -> Value {
        // 返回与 parent parser 对齐的 ready final shape。
        json!({
            // 声明 open final。
            "kind": "open-final",
            // 回显协议版本。
            "contractVersion": CONTRACT_VERSION,
            // 回显 canonical request nonce。
            "requestNonce": nonce,
            // 宣告 ready outcome。
            "outcome": "ready",
            // ready 是已完成终态。
            "completed": true,
            // ready session 不能安全自动重试。
            "retrySafe": false,
            // accepted 事实已经建立。
            "acceptedMayHaveOccurred": true,
            // 提供 OS CNG 生成的 opaque session identity。
            "sessionId": session_id,
            // ready 无错误投影。
            "error": Value::Null,
        })
    }

    // 从严格页面 command 读取固定 operation 标签。
    fn page_operation<'a>(
        // 借用待验证页面 command。
        value: &'a Value,
        // 借用当前 live session identity。
        session_id: &str,
    ) -> Option<(&'a str, &'a str, u64)> {
        // 页面 command 必须只含冻结字段。
        if !exact_keys(
            // 核对完整 command frame。
            value,
            // 拒绝 transport、path 或其他扩展字段。
            &[
                // 固定 frame 类别。
                "kind",
                // 固定协议版本。
                "contractVersion",
                // 当前公开 session。
                "sessionId",
                // 命令 nonce。
                "requestNonce",
                // 剩余总预算。
                "timeoutMs",
                // 私有 page ref。
                "pageRef",
                // 导航代际。
                "navigationGeneration",
                // provider-neutral operation。
                "operation",
            ],
        ) {
            // 字段漂移失败闭合。
            return None;
        }
        // 核对共享 frame、版本、session 与预算。
        if value.get("kind").and_then(Value::as_str) != Some("command")
        // 页面命令必须使用独立版本。
        || value.get("contractVersion").and_then(Value::as_str) != Some(PAGE_CONTRACT_VERSION)
        // 页面命令必须绑定当前 live session。
        || value.get("sessionId").and_then(Value::as_str) != Some(session_id)
        // 页面预算必须处于冻结范围。
        || !matches!(value.get("timeoutMs").and_then(Value::as_u64), Some(1..=30_000))
        {
            // 共享字段漂移失败闭合。
            return None;
        }
        // 读取并验证 canonical command nonce。
        let request_nonce = value
            // 读取 request nonce。
            .get("requestNonce")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 复核 canonical 形状。
            .filter(|nonce| canonical_nonce(nonce))?;
        // 读取有界导航代际。
        let generation = value
            // 读取 navigationGeneration。
            .get("navigationGeneration")
            // 只接受无符号整数。
            .and_then(Value::as_u64)
            // 收窄到 worker 协议上限。
            .filter(|value| *value <= u64::from(u32::MAX))?;
        // 读取 provider-neutral operation 对象。
        let operation = value
            // 取得 operation。
            .get("operation")
            // 根必须为对象。
            .and_then(Value::as_object)?;
        // 读取固定 operation kind。
        let kind = operation
            // 取得 kind。
            .get("kind")
            // 只接受字符串。
            .and_then(Value::as_str)?;
        // 核对 operation 专属字段与 page 绑定。
        let valid = match kind {
            // navigate 只携带 HTTP(S) URL 且从初始空 page 开始。
            "navigate" => {
                exact_keys(&Value::Object(operation.clone()), &["kind", "url"])
            // 初次导航没有私有 page ref。
            && value.get("pageRef").is_some_and(Value::is_null)
            // fixture 只接受初始零代际。
            && generation == 0
            // URL 必须是明确 HTTP(S)。
            && operation.get("url").and_then(Value::as_str).is_some_and(|url| url.starts_with("http://") || url.starts_with("https://"))
            }
            // wait 只携带固定文本条件且绑定当前 page。
            "wait" => {
                exact_keys(&Value::Object(operation.clone()), &["kind", "condition"])
            // 必须绑定 fixture 私有 page。
            && value.get("pageRef").and_then(Value::as_str) == Some(PAGE_REF)
            // 必须保持首个正代际。
            && generation == 1
            }
            // query 只携带 selector 与有界上限且绑定当前 page。
            "query" => {
                exact_keys(&Value::Object(operation.clone()), &["kind", "selector", "maxResults"])
            // 必须绑定 fixture 私有 page。
            && value.get("pageRef").and_then(Value::as_str) == Some(PAGE_REF)
            // 必须保持首个正代际。
            && generation == 1
            // 结果上限必须处于冻结范围。
            && matches!(operation.get("maxResults").and_then(Value::as_u64), Some(1..=100))
            }
            // fixture 不接受其他页面 mutation 或截图。
            _ => false,
        };
        // 只返回已经严格验证的 operation 事实。
        valid.then_some((kind, request_nonce, generation))
    }

    // 构造页面 command accepted frame。
    fn page_accepted(operation: &str, nonce: &str) -> Value {
        // 返回与生产 parser 对齐的 accepted shape。
        json!({
            // 声明页面 accepted kind。
            "kind": "command-accepted",
            // 回显页面协议版本。
            "contractVersion": PAGE_CONTRACT_VERSION,
            // 回显 command nonce。
            "requestNonce": nonce,
            // 回显固定 operation。
            "operation": operation,
            // 声明 worker 已接受 dispatch。
            "dispatchAccepted": true,
            // accepted 尚未完成。
            "completed": false
        })
    }

    // 构造页面 command completed final frame。
    fn page_final(operation: &str, nonce: &str, generation: u64) -> Value {
        // 按 operation 构造冻结成功数据与代际。
        let (data, final_generation) = match operation {
            // navigate 换发私有 page ref 并推进一代。
            "navigate" => (
                // 构造导航成功数据。
                json!({ "kind": "navigate", "pageRef": PAGE_REF, "navigated": true }),
                // 初始导航推进到第一代。
                generation.saturating_add(1),
            ),
            // wait 只返回条件满足事实。
            "wait" => (
                // 构造等待成功数据。
                json!({ "kind": "wait", "conditionMet": true }),
                // Query 不推进导航代际。
                generation,
            ),
            // query 返回一个 provider-neutral 命中。
            "query" => (
                // 构造严格有界查询成功数据。
                json!({
                    // 固定 query kind。
                    "kind": "query",
                    // 返回一个私有 element ref 与中立摘要。
                    "matches": [{
                        // 保存 worker 私有 element ref。
                        "elementRef": ELEMENT_REF,
                        // 保存语义 role。
                        "role": "button",
                        // 保存可访问名称。
                        "name": "Fixture Button",
                        // 保存可见文本。
                        "text": "fixture text",
                        // 保存可用事实。
                        "enabled": true
                    }],
                    // 总命中数为一。
                    "matchCount": 1,
                    // 未发生截断。
                    "truncated": false
                }),
                // Query 不推进导航代际。
                generation,
            ),
            // 调用方只会传入已验证 operation。
            _ => (Value::Null, generation),
        };
        // 返回与生产 parser 对齐的 completed final。
        json!({
            // 声明页面 final kind。
            "kind": "command-final",
            // 回显页面协议版本。
            "contractVersion": PAGE_CONTRACT_VERSION,
            // 回显 command nonce。
            "requestNonce": nonce,
            // 回显固定 operation。
            "operation": operation,
            // 声明确定完成。
            "outcome": "completed",
            // final 已可信完成。
            "completed": true,
            // 页面动作不得自动重派。
            "retrySafe": false,
            // accepted 事实已经建立。
            "acceptedMayHaveOccurred": true,
            // 保存当前导航代际。
            "navigationGeneration": final_generation,
            // 保存 operation-specific 成功数据。
            "data": data,
            // completed 无错误投影。
            "error": Value::Null
        })
    }

    // 服务页面 command，直到收到与 open nonce 关联的 close cancel 或 EOF。
    fn serve_until_close(
        // 可变借用 open 阶段创建的唯一输入 reader。
        input: &mut impl BufRead,
        // 借用原 open nonce。
        open_nonce: &str,
        // 借用当前 live session identity。
        session_id: &str,
    ) -> bool {
        // 持续处理严格页面 command 或最终 close。
        loop {
            // 为每条后续 JSON Lines frame 分配独立缓冲区。
            let mut line = String::new();
            // EOF 是可信的 worker 生命周期结束。
            if input.read_line(&mut line).ok() == Some(0) {
                // parent 已关闭持久管道。
                return true;
            }
            // 后续输入必须是有界 JSON。
            if line.len() > 64 * 1024 {
                // 超限失败闭合。
                return false;
            }
            // 解析唯一后续帧。
            let Ok(value) = serde_json::from_str::<Value>(line.trim_end()) else {
                // 非 JSON 后续输入不能可信完成。
                return false;
            };
            // 生命周期 cancel 结束 live session。
            if exact_keys(&value, &["kind", "contractVersion", "requestNonce"])
            // 必须是固定 cancel kind。
            && value.get("kind").and_then(Value::as_str) == Some("cancel")
            // 必须使用 open worker 协议版本。
            && value.get("contractVersion").and_then(Value::as_str) == Some(CONTRACT_VERSION)
            // 必须关联原 open nonce。
            && value.get("requestNonce").and_then(Value::as_str) == Some(open_nonce)
            {
                // close 生命周期可信完成。
                return true;
            }
            // 其余输入必须是严格页面 command。
            let Some((operation, nonce, generation)) = page_operation(&value, session_id) else {
                // 未知或漂移输入失败闭合。
                return false;
            };
            // 先发送与 command 严格关联的 accepted。
            if !write_frame(&page_accepted(operation, nonce)) {
                // parent 已断开。
                return false;
            }
            // 再发送唯一 completed final。
            if !write_frame(&page_final(operation, nonce, generation)) {
                // parent 已断开。
                return false;
            }
        }
    }

    // 运行无参数 fixed sibling worker 并保持 stdin 直到真实 close 或 EOF。
    pub(super) fn entry() {
        // 专用生产替身不接受 argv、模式、路径或环境覆盖。
        if std::env::args_os().nth(1).is_some() {
            // 参数漂移使用固定失败码。
            std::process::exit(2);
        }
        // 取得 worker 全生命周期唯一 stdin owner。
        let stdin = std::io::stdin();
        // 从 open 到 close 持续复用同一个 reader。
        let mut input = stdin.lock();
        // 读取并严格验证唯一 open request。
        let Some(nonce) = read_open_nonce(&mut input) else {
            // 输入协议失败。
            std::process::exit(2);
        };
        // 先报告真实 accepted 事实。
        if !write_frame(&accepted(&nonce)) {
            // parent 已断开。
            std::process::exit(2);
        }
        // 为真实 broker 客户端保留确定性的 accepted 观察窗口。
        std::thread::sleep(ACCEPTED_READY_GATE);
        // 在 accepted 门控完成后生成独立于请求 nonce 的 OS 随机 opaque identity。
        let Some(session_id) = random_session_id() else {
            // CNG 不可用时不得伪造会话。
            std::process::exit(2);
        };
        // 再报告真实 ready final。
        if !write_frame(&ready(&nonce, &session_id)) {
            // parent 已断开。
            std::process::exit(2);
        }
        // 等待严格 cancel 或 parent EOF 完成真实 close 生命周期。
        if !serve_until_close(&mut input, &nonce, &session_id) {
            // 关联漂移不得伪造可信 close。
            std::process::exit(2);
        }
    }
}

#[cfg(target_os = "windows")]
fn main() {
    windows_fixture::entry();
}
