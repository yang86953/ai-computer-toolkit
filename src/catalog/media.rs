// 导入 catalog 结构与正式隔离执行域。
use crate::{
    // 导入操作描述结构。
    catalog::{FieldDescriptor, FieldValueType, OperationDescriptor},
    // 导入强类型执行域。
    domain::ExecutionRealm,
};

const MEDIA_TARGET: &[FieldDescriptor] = &[FieldDescriptor {
    name: "sessionId",
    // 使用封闭字符串类型，保持既有媒体 catalog 文本与行为。
    value_type: FieldValueType::String,
    required: true,
}];

pub(super) const MEDIA_OPERATIONS: &[OperationDescriptor] = &[
    operation(
        "media-session.toggle-play-pause",
        "toggle-play-pause",
        "通过 Windows 系统媒体会话切换播放/暂停，不激活播放器窗口。",
    ),
    operation(
        "media-session.play",
        "play",
        "通过 Windows 系统媒体会话开始播放，不激活播放器窗口。",
    ),
    operation(
        "media-session.pause",
        "pause",
        "通过 Windows 系统媒体会话暂停播放，不激活播放器窗口。",
    ),
    operation(
        "media-session.skip-next",
        "skip-next",
        "通过 Windows 系统媒体会话切换下一首，不激活播放器窗口。",
    ),
    operation(
        "media-session.skip-previous",
        "skip-previous",
        "通过 Windows 系统媒体会话切换上一首，不激活播放器窗口。",
    ),
];

const fn operation(
    id: &'static str,
    operation: &'static str,
    summary: &'static str,
) -> OperationDescriptor {
    OperationDescriptor {
        id,
        operation,
        summary,
        target_fields: MEDIA_TARGET,
        argument_fields: &[],
        mutates: true,
        requires_confirmation: true,
        requires_session_id: true,
        background_policy: "guaranteed",
        // 正式媒体控制要求独立隔离 worker。
        execution_realm: ExecutionRealm::IsolatedWorker,
        methods: &["media-session"],
    }
}
