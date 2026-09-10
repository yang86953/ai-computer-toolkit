//! 精确窗口零帧捕获预检领域 Module。

// 导入 JSON 构造器。
use serde_json::{Value, json};

// 导入窗口重新发现、前景门禁与 Windows 预检 Component。
use crate::{
    // 导入私有原生事实边界。
    adapters::{
        // 导入窗口枚举、唯一解析与前景门禁。
        window::{capture_visible_titled_windows, ensure_foreground_unchanged, resolve_window},
        // 导入零帧 Windows 预检 Component。
        window_capture_preflight_windows::{self, CapturePreflightEvidence},
        // 导入前景只读快照。
        windows::foreground_hwnd,
    },
    // 导入稳定 capability ID。
    capabilities,
    // 导入语言中立结果类型。
    domain::AppResult,
};

// 对 canonical 窗口目标执行零帧捕获预检。
pub(crate) fn preflight(session_id: &str) -> AppResult<Value> {
    // 记录预检前前景窗口。
    let foreground_before = foreground_hwnd();
    // 每次使用都重新枚举当前可见有标题窗口。
    let windows = capture_visible_titled_windows()?;
    // 要求 opaque 目标在当前快照中唯一命中。
    let window = resolve_window(session_id, &windows)?;
    // 只把私有原生句柄交给同进程 Windows Component。
    let evidence = window_capture_preflight_windows::inspect(window.hwnd)?;
    // 记录全部只读查询完成后的前景窗口。
    let foreground_after = foreground_hwnd();
    // 前景变化必须失败闭合，不伪造 foregroundUnchanged。
    ensure_foreground_unchanged(foreground_before, foreground_after)?;
    // 投影稳定 control envelope 与 data 对象。
    Ok(render(session_id, &evidence))
}

// 把封闭证据投影为与 C++ 对照一致的公开 JSON。
fn render(session_id: &str, evidence: &CapturePreflightEvidence) -> Value {
    // 返回版本化 control envelope。
    json!({
        // 标记调用成功。
        "ok": true,
        // 固定跨实现控制协议版本。
        "contractVersion": "act/control/v1",
        // 声明当前直接执行实现。
        "implementation": "rust",
        // 输出满足 capture-preflight schema 的数据对象。
        "data": {
            // 输出稳定 capability ID。
            "capability": capabilities::WINDOW_CAPTURE_PREFLIGHT,
            // 仅回显 canonical opaque 目标。
            "targetId": session_id,
            // 固定窗口目标类别。
            "targetKind": "application-window",
            // 声明全流程只读。
            "readOnly": true,
            // 成功路径已通过前景门禁。
            "foregroundUnchanged": true,
            // 预检不需要前景。
            "foregroundRequired": false,
            // 预检不需要逐操作确认。
            "confirmationRequired": false,
            // 真实截图执行已经迁入 Rust 隔离 worker。
            "captureExecutionMigrated": true,
            // 声明当前 Rust 隔离捕获入口。
            "captureCompatibilityEntrypoint": "rust-isolated-worker",
            // 真实捕获可能显示系统隐私指示器，预检不会规避。
            "privacyIndicatorMayAppearOnCapture": true,
            // 输出封闭只读证据。
            "evidence": {
                // 输出窗口可见性。
                "visible": evidence.visible,
                // 输出最小化状态。
                "minimized": evidence.minimized,
                // 输出 cloaked 状态。
                "cloaked": evidence.cloaked,
                // 仅输出是否具有正尺寸。
                "nonzeroExtent": evidence.nonzero_extent,
                // 输出桌面组合状态。
                "desktopCompositionEnabled": evidence.desktop_composition_enabled,
                // 输出内容保护分类。
                "contentProtection": evidence.content_protection,
                // 输出 WGC runtime 分类。
                "wgcRuntime": evidence.wgc_runtime,
                // 输出 WGC item interop 分类。
                "wgcItemInterop": evidence.wgc_item_interop,
                // 仅输出 item 是否具有正尺寸。
                "captureItemNonzeroSize": evidence.capture_item_nonzero_size,
                // 输出固定优先级 eligibility。
                "eligibility": evidence.eligibility,
            },
            // 输出可机器核验的零副作用证据。
            "safety": {
                // 未读取像素。
                "pixelsRead": false,
                // 未启动捕获 session。
                "captureSessionStarted": false,
                // 未创建帧池。
                "framePoolCreated": false,
                // 未写文件。
                "fileWritten": false,
                // 未激活窗口。
                "windowActivated": false,
                // 未发送输入。
                "inputSent": false,
                // 未公开原生目标。
                "nativeTargetExposed": false,
            },
        },
    })
}

// 声明纯投影契约测试。
#[cfg(test)]
mod tests {
    // 导入父模块投影与证据类型。
    use super::{CapturePreflightEvidence, render};

    // 递归判断 JSON 是否含禁止字段。
    fn contains_key(value: &serde_json::Value, forbidden: &str) -> bool {
        // 按 JSON 类型递归扫描。
        match value {
            // 对象同时检查当前键与子值。
            serde_json::Value::Object(object) => object
                // 遍历全部字段。
                .iter()
                // 任一键或子树命中即返回真。
                .any(|(key, value)| key == forbidden || contains_key(value, forbidden)),
            // 数组递归扫描每个元素。
            serde_json::Value::Array(items) => items
                // 遍历数组元素。
                .iter()
                // 任一子树命中即返回真。
                .any(|value| contains_key(value, forbidden)),
            // 标量没有字段。
            _ => false,
        }
    }

    // 验证公开 envelope、安全常量和原生字段封闭性。
    #[test]
    fn render_matches_the_zero_frame_public_contract() {
        // 构造成功证据夹具。
        let evidence = CapturePreflightEvidence {
            // 窗口可见。
            visible: true,
            // 窗口未最小化。
            minimized: false,
            // 窗口未 cloaked。
            cloaked: false,
            // 窗口具有正尺寸。
            nonzero_extent: true,
            // 桌面组合可用。
            desktop_composition_enabled: true,
            // 无内容保护。
            content_protection: "none",
            // WGC runtime 支持。
            wgc_runtime: "supported",
            // item interop 可用。
            wgc_item_interop: "available",
            // item 具有正尺寸。
            capture_item_nonzero_size: true,
            // eligibility 成功。
            eligibility: "eligible-for-certified-capture-route",
        };
        // 投影 canonical 目标。
        let value = render("s2:w:0123456789abcdef", &evidence);
        // 验证 envelope 实现声明。
        assert_eq!(value["implementation"], "rust");
        // 验证稳定 capability。
        assert_eq!(value["data"]["capability"], "window.capture.preflight@1");
        // 验证全部零副作用常量。
        for field in [
            // 像素读取。
            "pixelsRead",
            // 捕获 session。
            "captureSessionStarted",
            // 帧池。
            "framePoolCreated",
            // 文件写入。
            "fileWritten",
            // 窗口激活。
            "windowActivated",
            // 输入发送。
            "inputSent",
            // 原生目标公开。
            "nativeTargetExposed",
        ] {
            // 每个安全字段都必须为 false。
            assert_eq!(value["data"]["safety"][field], false);
        }
        // 验证禁止原生字段不会出现在任意层级。
        for field in [
            // 窗口句柄。
            "hwnd",
            // 进程 ID。
            "pid",
            // 展开进程 ID。
            "processId",
            // 通用原生句柄。
            "nativeHandle",
            // 坐标与尺寸。
            "left",
            "top",
            "right",
            "bottom",
            "width",
            "height",
            // 文件路径。
            "path",
        ] {
            // 禁止字段不得存在。
            assert!(!contains_key(&value, field));
        }
    }
}
