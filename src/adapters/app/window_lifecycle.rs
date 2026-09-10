//! 把统一 app 请求原样交给 Window Lifecycle Module 执行策略与领域状态机。

// 导入公开 JSON 值。
use serde_json::Value;

// 导入请求、结果与窗口生命周期 Module。
use crate::{
    // 导入统一 app 请求和结果类型。
    domain::{AppResult, CommandRequest},
    // 只调用领域 Module，不在 Adapter 复制窗口控制规则。
    modules::window_lifecycle,
};

// 执行一次通用窗口状态或几何生命周期请求。
pub(super) fn execute(request: &CommandRequest) -> AppResult<Value> {
    // 只读取公开 opaque sessionId，不恢复任何原生目标。
    let session_id = request
        // 从公开 target 对象读取字段。
        .target
        // 保留缺失目标供 Module 按策略顺序分类。
        .get("sessionId")
        // 只接受字符串形状。
        .and_then(Value::as_str);
    // 只读取 provider-neutral input，并保留缺失状态。
    let input = request.args.get("input");
    // 委托 Module 统一执行确认、同意、目标、权限与状态机。
    window_lifecycle::perform(
        // 传播可能缺失的 opaque 目标。
        session_id,
        // 传播逐操作确认事实。
        request.confirmed,
        // 传播预先前景影响同意。
        request.foreground_consent,
        // 传播可能缺失的公开输入。
        input,
    )
}
