//! 为 legacy `uia` / `accessibility` surface 提供同一只读兼容入口。

// 导入 JSON 构造与值类型。
use serde_json::{Value, json};

// 引入 UIA Adapter 私有封闭错误码实现。
#[path = "uia_error.rs"]
mod error_code;

// 导入 UIA Adapter 私有封闭错误码。
use error_code::AppUiaErrorCode;

// 导入窗口别名、capability、Module 与统一错误类型。
use crate::{
    // 导入 adapter trait 与窗口观察 adapter。
    adapters::{AppAdapter, WindowAdapter},
    // 导入 capability 单一注册表。
    capabilities,
    // 导入命令请求与结果类型。
    domain::{AppResult, CommandRequest},
    // 导入隔离可访问性 Module。
    modules::accessibility,
};

// 表示共享实现的 legacy 可访问性别名。
pub struct UiaAdapter {
    // 保存当前 CLI surface 名。
    app_id: &'static str,
}

// 提供两个别名的静态构造。
impl UiaAdapter {
    // 创建一个固定别名 adapter。
    pub const fn new(app_id: &'static str) -> Self {
        // 保存只读 surface ID。
        Self { app_id }
    }
}

// 从 inspect 请求读取 canonical sessionId。
fn required_session_id(request: &CommandRequest) -> AppResult<&str> {
    // 只接受非空字符串。
    request
        // 读取目标对象。
        .target
        // 读取 sessionId。
        .get("sessionId")
        // 要求字符串。
        .and_then(Value::as_str)
        // 拒绝空值。
        .filter(|value| !value.is_empty())
        // 映射参数错误。
        .ok_or_else(|| {
            // 指引调用方使用 opaque 目标。
            AppUiaErrorCode::InvalidArgument.error("target.sessionId is required.")
        })
}

// 从 inspect 请求读取 worker deadline。
fn timeout_ms(request: &CommandRequest) -> AppResult<u32> {
    // 读取 CLI 注入值或默认 5000ms。
    let value = request
        // 读取 args 对象。
        .args
        // 读取 timeoutMs。
        .get("timeoutMs")
        // 要求无符号整数。
        .and_then(Value::as_u64)
        // 缺失时使用兼容默认值。
        .unwrap_or(5_000);
    // 转换为 u32。
    let value = u32::try_from(value).map_err(|_| {
        // 返回参数错误。
        AppUiaErrorCode::InvalidArgument.error("timeoutMs must be between 1 and 30000.")
    })?;
    // 验证硬边界。
    if !(1..=30_000).contains(&value) {
        // 返回稳定参数错误。
        return Err(AppUiaErrorCode::InvalidArgument.error(
            // 说明允许范围。
            "timeoutMs must be between 1 and 30000.",
        ));
    }
    // 返回已验证 deadline。
    Ok(value)
}

// 实现 legacy 只读别名。
impl AppAdapter for UiaAdapter {
    // 返回当前别名 ID。
    fn app_id(&self) -> &'static str {
        // 输出构造时固定的别名。
        self.app_id
    }

    // 报告已迁回 Rust 的隔离可访问性能力。
    fn status(&self) -> AppResult<Value> {
        // 返回不触发 provider 的能力状态。
        Ok(json!({
            // 标记成功。
            "ok": true,
            // 输出当前别名。
            "app": self.app_id(),
            // 标记 Rust Job 隔离 backend。
            "backend": "Rust job-bounded UI Automation observation worker",
            // 声明后台保证。
            "backgroundPolicy": "guaranteed",
            // 标记只读。
            "readOnly": true,
            // 输出版本化 capability。
            "capabilities": [capabilities::ACCESSIBILITY_TREE_READ],
            // 声明 provider 隔离域。
            "executionDomain": "isolated-worker",
        }))
    }

    // 复用 canonical 窗口 sessions，不建立第二目标命名空间。
    fn sessions(&self, request: &CommandRequest) -> AppResult<Value> {
        // 委托窗口观察 adapter。
        WindowAdapter.sessions(request)
    }

    // 在隔离 worker 中执行 root-only 兼容检查。
    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        // 读取 canonical s2:w。
        let session_id = required_session_id(request)?;
        // 读取 worker deadline。
        let timeout_ms = timeout_ms(request)?;
        // 委托只读可访问性 Module。
        accessibility::inspect_root(session_id, timeout_ms)
    }

    // 可访问性别名永不提供写操作。
    fn run(&self, _: &CommandRequest) -> AppResult<Value> {
        // 返回后台能力缺口。
        Err(AppUiaErrorCode::BackgroundOperationUnavailable.error(
            // 明确写入必须走统一 facade。
            "Accessibility is read-only; writes require the policy-evaluated app facade.",
        ))
    }
}

// 验证别名与参数边界。
#[cfg(test)]
mod tests {
    // 导入待测函数。
    use super::*;
    // 导入请求 verb。
    use crate::domain::Verb;

    // 构造基础 inspect 请求。
    fn request() -> CommandRequest {
        // 从默认只读请求开始。
        let mut request = CommandRequest::read(Verb::Inspect, "uia");
        // 插入合成 sessionId。
        request.target.insert(
            // 使用公开字段名。
            "sessionId".to_owned(),
            // 使用字符串目标。
            Value::String("s2:w:0000000000000000".to_owned()),
        );
        // 返回夹具。
        request
    }

    // 验证两个 legacy 名称共享能力状态。
    #[test]
    fn aliases_share_the_same_capability() {
        // 创建 uia 别名。
        let uia = UiaAdapter::new("uia").status().unwrap_or(Value::Null);
        // 创建 accessibility 别名。
        let accessibility = UiaAdapter::new("accessibility")
            // 读取状态。
            .status()
            // 测试中避免 unwrap lint。
            .unwrap_or(Value::Null);
        // 两者 capability 必须一致。
        assert_eq!(uia["capabilities"], accessibility["capabilities"]);
    }

    // 验证 timeout 硬边界。
    #[test]
    fn timeout_is_bounded() {
        // 构造零 deadline 请求。
        let mut zero_request = request();
        // 注入零 deadline。
        zero_request
            // 写入公开参数字段。
            .args
            // 使用范围下界之外的值。
            .insert("timeoutMs".to_owned(), Value::from(0));
        // 零 deadline 必须拒绝。
        let zero_error = timeout_ms(&zero_request)
            // 范围外值不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("zero UIA timeout must fail"));
        // 保持稳定参数错误码。
        assert_eq!(zero_error.code, "INVALID_ARGUMENT");
        // 保持既有 deadline 消息。
        assert_eq!(zero_error.message, "timeoutMs must be between 1 and 30000.");

        // 构造超出 u32 的 deadline 请求。
        let mut overflow_request = request();
        // 注入无法转换为 u32 的 JSON 整数。
        overflow_request.args.insert(
            // 写入公开参数字段。
            "timeoutMs".to_owned(),
            // 使用比 u32 上界大一的值。
            Value::from(u64::from(u32::MAX) + 1),
        );
        // 转换溢出必须稳定拒绝。
        let overflow_error = timeout_ms(&overflow_request)
            // 溢出值不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("overflowing UIA timeout must fail"));
        // 保持稳定参数错误码。
        assert_eq!(overflow_error.code, "INVALID_ARGUMENT");
        // 转换与范围分支必须共享既有消息。
        assert_eq!(
            overflow_error.message,
            "timeoutMs must be between 1 and 30000."
        );
    }

    // 验证目标与只读门禁在 Window/Accessibility provider 前保持稳定。
    #[test]
    fn uia_owned_gates_fail_before_provider_access() {
        // 构造不含目标的 inspect 请求。
        let inspect_request = CommandRequest::read(Verb::Inspect, "uia");
        // 缺失 session 必须在隔离 worker 前失败。
        let missing_target = required_session_id(&inspect_request)
            // 缺失目标不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("missing UIA target must fail"));
        // 保持稳定参数错误码。
        assert_eq!(missing_target.code, "INVALID_ARGUMENT");
        // 保持既有目标缺失消息。
        assert_eq!(missing_target.message, "target.sessionId is required.");

        // 构造不含 mutation 参数的 run 请求。
        let run_request = CommandRequest::read(Verb::Run, "uia");
        // UIA surface 必须固定保持只读。
        let unavailable = UiaAdapter::new("uia")
            // 调用纯 Adapter 写门禁。
            .run(&run_request)
            // 写操作不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("UIA mutation must remain unavailable"));
        // 保持稳定后台操作缺口码。
        assert_eq!(unavailable.code, "BACKGROUND_OPERATION_UNAVAILABLE");
        // 保持既有只读消息。
        assert_eq!(
            unavailable.message,
            "Accessibility is read-only; writes require the policy-evaluated app facade."
        );
    }
}
