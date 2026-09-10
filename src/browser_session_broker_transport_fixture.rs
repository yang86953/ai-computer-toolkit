//! 为集成测试运行 provider-neutral browser-session transport ready 探针。

// 导入唯一 stdout 输出。
use std::io::Write;

// 导入 JSON 构造。
use serde_json::json;
// 导入固定 broker Adapter 与安全错误投影。
use crate::{
    // 只调用无 path、endpoint、argv 或环境覆盖的固定 transport Adapter。
    adapters::browser_session_broker_windows,
    // 将内部失败投影为稳定 JSON。
    domain::error_json,
};

// 固定 transport 探测总预算。
const PROBE_TIMEOUT_MS: u32 = 5_000;

// 运行一次固定 server-first ready 探测并写出 provider-neutral 结果。
pub fn run_stdio() -> i32 {
    // 连接、双向认证并严格解析 ready。
    match browser_session_broker_windows::probe_ready(PROBE_TIMEOUT_MS) {
        // 成功只公开协议 epoch。
        Ok(broker_epoch) => {
            // 构造隐藏 fixture 结果。
            let value = json!({
                // 标记探测成功。
                "ok": true,
                // 返回当前 broker 代际。
                "brokerEpoch": broker_epoch,
            });
            // 序列化固定 JSON 对象。
            let text = value.to_string();
            // 写入单行 fixture 输出。
            let _ = writeln!(std::io::stdout(), "{text}");
            // 返回成功退出码。
            0
        }
        // 失败保持统一安全错误 envelope。
        Err(error) => {
            // 投影不含 endpoint、PID、session 或镜像路径的错误。
            let text = error_json(&error).to_string();
            // 写入单行 fixture 输出。
            let _ = writeln!(std::io::stdout(), "{text}");
            // 返回失败退出码。
            2
        }
    }
}
