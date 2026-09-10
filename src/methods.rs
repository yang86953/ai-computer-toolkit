use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MethodDescriptor {
    pub id: &'static str,
    pub execution_scope: &'static str,
    pub availability: &'static str,
    pub summary: &'static str,
}

const METHODS: &[MethodDescriptor] = &[
    MethodDescriptor {
        id: "app-api",
        execution_scope: "background",
        availability: "extension-point",
        summary: "应用公开的 HTTP、SDK 或本地 API；按应用专用适配器注册。",
    },
    MethodDescriptor {
        id: "command-line",
        execution_scope: "background",
        availability: "extension-point",
        summary: "应用公开 CLI；固定程序与参数模型后注册，不经 shell 拼接。",
    },
    MethodDescriptor {
        id: "com-automation",
        execution_scope: "background",
        availability: "native-provider",
        summary: "已注册在版本化 app capability 后的 attach-only COM；公共输入不接受 ProgID、COM member 或任意脚本。",
    },
    MethodDescriptor {
        id: "ipc",
        execution_scope: "background",
        availability: "extension-point",
        summary: "已公开的命名管道、RPC 或 App Service 协议；禁止猜测消息格式。",
    },
    MethodDescriptor {
        id: "media-session",
        execution_scope: "background",
        // 阶段六隔离 worker 认证前不得把旧主进程实现报告为可用。
        availability: "candidate-not-certified",
        // 目录只描述目标能力，不承诺当前存在生产执行路径。
        summary: "Windows GSMTC 系统媒体会话候选；阶段六完成前生产执行失败闭合。",
    },
    MethodDescriptor {
        id: "cdp",
        execution_scope: "background",
        availability: "extension-point",
        summary: "Chrome DevTools Protocol；仅连接显式启用的远程调试端口。",
    },
    MethodDescriptor {
        id: "uia",
        execution_scope: "provider-dependent",
        availability: "read-native",
        summary: "UI Automation 控制模式；仅在应用实际暴露并认证后可写入。",
    },
    MethodDescriptor {
        id: "win32-message",
        execution_scope: "background",
        availability: "native",
        summary: "系统定义的标准控件消息；当前认证标准 Edit 的 WM_SETTEXT。",
    },
    MethodDescriptor {
        id: "windows-graphics-capture",
        execution_scope: "background",
        availability: "native",
        summary: "Windows Graphics Capture 精确窗口帧；不激活窗口，支持被遮挡窗口，最小化窗口无新帧。",
    },
    MethodDescriptor {
        id: "xdg-desktop-portal-screenshot",
        execution_scope: "foreground",
        availability: "native-consent-gated",
        summary: "Wayland 会话的标准 XDG Desktop Portal 交互式截图；系统选择器决定捕获源，无 X11 或私有 compositor 回退。",
    },
    MethodDescriptor {
        id: "media-foundation-h264",
        execution_scope: "background",
        availability: "native",
        summary: "项目自有 Rust Media Foundation H.264/MP4 编码；默认 2fps、960px、quality 75，不接受任意编码器参数。",
    },
    MethodDescriptor {
        id: "headless-browser",
        execution_scope: "background",
        availability: "native",
        summary: "隔离 profile 的 headless Chromium 操作。",
    },
    MethodDescriptor {
        id: "file-automation",
        execution_scope: "background",
        availability: "native",
        summary: "应用支持的文档或配置文件自动化；必须验证对象所有权与回读。",
    },
    MethodDescriptor {
        id: "foreground-input",
        execution_scope: "foreground",
        availability: "native-consent-gated",
        summary: "精确窗口的恢复、激活和 SendInput；只在任务授权后作为最后回退。",
    },
];

pub fn all() -> &'static [MethodDescriptor] {
    METHODS
}

pub fn find(id: &str) -> Option<&'static MethodDescriptor> {
    METHODS.iter().find(|method| method.id == id)
}
