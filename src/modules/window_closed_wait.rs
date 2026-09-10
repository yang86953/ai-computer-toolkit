//! 组合窗口重新发现与稳定条件 Component，实现精确窗口关闭等待。

// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "window_closed_wait_error.rs"]
mod error_code;

// 导入单调时钟与有界持续时间。
use std::time::{Duration, Instant};

// 导入严格输入反序列化。
use serde::Deserialize;
// 导入公开 JSON 值与构造器。
use serde_json::{Value, json};

// 导入窗口观察、等待 Component、opaque 匹配和统一错误。
use crate::{
    // 导入当前窗口 inventory 与前景检查。
    adapters::{
        // 导入公开窗口观察边界。
        window::{capture_visible_titled_windows, ensure_foreground_unchanged},
        // 导入私有窗口事实和前景查询。
        windows::{WindowRecord, foreground_hwnd, opaque_window_session_id},
    },
    // 导入取消、稳定状态与 opaque 唯一匹配 Components。
    components::{
        // 导入进程级协作取消信号。
        cancellation,
        // 导入 opaque 目标唯一匹配分类。
        opaque_id::{OpaqueTargetMatch, match_opaque_target},
        // 导入布尔稳定状态机和可取消暂停。
        stable_wait::{
            // 导入连续条件跟踪器。
            StableConditionTracker,
            // 导入稳定判定。
            WaitDecision,
            // 导入短切片可取消暂停。
            cancellable_pause,
        },
    },
    // 导入统一错误和结果类型。
    domain::{AppControlError, AppResult},
};

// 导入当前 Module 私有封闭错误码。
use error_code::WindowClosedWaitErrorCode;

// 固定默认总等待时间。
const DEFAULT_TIMEOUT_MS: u32 = 5_000;
// 固定默认轮询间隔。
const DEFAULT_POLL_INTERVAL_MS: u32 = 100;
// 固定默认连续缺失时间。
const DEFAULT_STABLE_FOR_MS: u32 = 100;

// 返回默认总等待时间。
const fn default_timeout_ms() -> u32 {
    // 使用公开契约默认值。
    DEFAULT_TIMEOUT_MS
}

// 返回默认轮询间隔。
const fn default_poll_interval_ms() -> u32 {
    // 使用公开契约默认值。
    DEFAULT_POLL_INTERVAL_MS
}

// 返回默认连续缺失时间。
const fn default_stable_for_ms() -> u32 {
    // 使用公开契约默认值。
    DEFAULT_STABLE_FOR_MS
}

// 保存版本一关闭等待输入。
#[derive(Clone, Copy, Debug, Deserialize)]
// 使用 camelCase 并拒绝调用方扩展未认证字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WindowClosedWaitInput {
    // 保存总等待 deadline。
    #[serde(default = "default_timeout_ms")]
    // 总等待时间以毫秒计。
    timeout_ms: u32,
    // 保存每次重新发现之间的暂停时间。
    #[serde(default = "default_poll_interval_ms")]
    // 轮询间隔以毫秒计。
    poll_interval_ms: u32,
    // 保存目标必须连续无法解析的时间。
    #[serde(default = "default_stable_for_ms")]
    // 稳定时间以毫秒计。
    stable_for_ms: u32,
}

// 表示一次当前窗口 inventory 的封闭匹配分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowPresence {
    // 当前 inventory 没有匹配目标。
    Missing,
    // 当前 inventory 唯一命中目标。
    Unique,
    // 当前 inventory 出现 opaque 指纹碰撞。
    Ambiguous,
}

// 严格解析并验证公开输入。
fn parse_input(value: &Value) -> AppResult<WindowClosedWaitInput> {
    // 按版本一 schema 反序列化。
    let input = serde_json::from_value::<WindowClosedWaitInput>(value.clone()).map_err(|_| {
        // 返回不回显输入内容的固定参数错误。
        WindowClosedWaitErrorCode::InvalidArgument.error(
            // 指向版本化输入契约。
            "Window closed wait input violates schema://window/closed-wait/v1.",
        )
    })?;
    // 总等待必须有界且足以容纳可取消采样。
    if !(50..=30_000).contains(&input.timeout_ms)
        // 轮询不能形成忙循环或不可取消的长暂停。
        || !(25..=1_000).contains(&input.poll_interval_ms)
        // 轮询间隔不得超过总 deadline。
        || input.poll_interval_ms > input.timeout_ms
        // 连续缺失时间不得超过总 deadline。
        || input.stable_for_ms > input.timeout_ms
    {
        // 返回统一时间边界错误。
        return Err(WindowClosedWaitErrorCode::InvalidArgument.error(
            // 说明全部封闭时间范围。
            "Window closed wait requires timeoutMs 50..30000, pollIntervalMs 25..1000, pollIntervalMs <= timeoutMs, and stableForMs 0..timeoutMs.",
        ));
    }
    // 返回已验证输入。
    Ok(input)
}

// 从一份当前窗口 inventory 分类 opaque 目标存在性。
fn classify_presence(session_id: &str, windows: &[WindowRecord]) -> WindowPresence {
    // 使用统一 opaque Component 保证零一多语义。
    match match_opaque_target(session_id, windows, |window| {
        // 从当前私有事实重新生成 canonical s2:w。
        Some(opaque_window_session_id(window))
    }) {
        // 零命中表示当前公开 inventory 已无法解析目标。
        OpaqueTargetMatch::Missing => WindowPresence::Missing,
        // 唯一命中表示目标当前仍可解析。
        OpaqueTargetMatch::Unique(_) => WindowPresence::Unique,
        // 多命中必须由 Module 立即失败闭合。
        OpaqueTargetMatch::Ambiguous => WindowPresence::Ambiguous,
    }
}

// 执行一次不激活窗口的当前 inventory 采样。
fn sample_presence(session_id: &str) -> AppResult<WindowPresence> {
    // 记录采样前的私有前景身份。
    let foreground_before = foreground_hwnd();
    // 只读枚举可见有标题顶层窗口。
    let windows = capture_visible_titled_windows()?;
    // 从当前快照分类目标存在性。
    let presence = classify_presence(session_id, &windows);
    // 记录采样后的私有前景身份。
    let foreground_after = foreground_hwnd();
    // 本次观察若干扰前景则不得作为证据。
    ensure_foreground_unchanged(foreground_before, foreground_after)?;
    // 返回当前封闭分类。
    Ok(presence)
}

// 构造不泄漏目标细节的取消错误。
fn cancelled(samples: u64) -> AppControlError {
    // 返回只含安全计数的结构化错误。
    WindowClosedWaitErrorCode::Cancelled.with_details(
        // 说明未建立关闭等待结果。
        "The exact window closed wait was cancelled before absence became stable.",
        // 仅公开完成采样次数。
        json!({ "samples": samples }),
    )
}

// 构造不泄漏窗口身份事实的总超时错误。
fn timed_out(
    // 接收已验证输入。
    input: &WindowClosedWaitInput,
    // 接收完成采样次数。
    samples: u64,
    // 接收封闭最后观察分类。
    last_observation: &str,
) -> AppControlError {
    // 返回有界等待证据。
    WindowClosedWaitErrorCode::Timeout.with_details(
        // 说明目标没有达到连续缺失条件。
        "The exact window did not remain unresolvable for the required stable duration.",
        // 仅公开契约级时间与分类。
        json!({
            // 输出总 deadline。
            "timeoutMs": input.timeout_ms,
            // 输出要求的连续缺失时间。
            "stableForMs": input.stable_for_ms,
            // 输出完成采样次数。
            "samples": samples,
            // 输出安全的最后分类。
            "lastObservation": last_observation,
        }),
    )
}

// 以可替换采样器执行完整等待循环。
fn wait_with_sampler(
    // 接收调用方已知 opaque 目标。
    session_id: &str,
    // 接收已验证时间配置。
    input: WindowClosedWaitInput,
    // 接收当前 inventory 采样能力。
    mut sample: impl FnMut() -> AppResult<WindowPresence>,
    // 接收无副作用取消探针。
    mut is_cancelled: impl FnMut() -> bool,
) -> AppResult<Value> {
    // 记录单调操作起点。
    let started = Instant::now();
    // 构造总等待 deadline。
    let timeout = Duration::from_millis(u64::from(input.timeout_ms));
    // 创建连续缺失状态机。
    let mut tracker = StableConditionTracker::new(u64::from(input.stable_for_ms));
    // 初始化完成采样计数。
    let mut samples = 0_u64;
    // 初始化安全最后观察分类。
    let mut last_observation = "not-sampled";
    // 只在成功、错误、取消或超时后退出。
    loop {
        // 每次采样前响应取消。
        if is_cancelled() {
            // 返回结构化取消结果。
            return Err(cancelled(samples));
        }
        // 总 deadline 到达后停止采样。
        if started.elapsed() >= timeout {
            // 返回结构化超时证据。
            return Err(timed_out(&input, samples, last_observation));
        }
        // 执行一次当前窗口 inventory 采样。
        let presence = sample()?;
        // 记录完成采样次数。
        samples = samples.saturating_add(1);
        // 把当前封闭分类映射为公开安全文本。
        last_observation = match presence {
            // 目标当前无法解析。
            WindowPresence::Missing => "missing",
            // 目标当前唯一存在。
            WindowPresence::Unique => "unique",
            // opaque 指纹发生碰撞。
            WindowPresence::Ambiguous => "ambiguous",
        };
        // 多命中必须立即失败，不能等待歧义自行消失。
        if presence == WindowPresence::Ambiguous {
            // 返回不公开内部候选的歧义错误。
            return Err(WindowClosedWaitErrorCode::AmbiguousTarget.with_details(
                // 说明当前 opaque 目标无法唯一解析。
                "The exact window session matched more than one current window.",
                // 只公开采样次数。
                json!({ "samples": samples }),
            ));
        }
        // 计算从操作起点开始的单调毫秒数。
        let now_ms = started
            // 读取当前经过时间。
            .elapsed()
            // 转换为毫秒。
            .as_millis()
            // 饱和限制到 u64。
            .min(u128::from(u64::MAX)) as u64;
        // 只有连续 Missing 才推进成功状态。
        match tracker.observe(now_ms, presence == WindowPresence::Missing) {
            // 达到连续缺失时间后返回只读结果。
            WaitDecision::Satisfied { stable_for_ms } => {
                // 构造不夸大原生窗口销毁事实的成功证据。
                return Ok(json!({
                    // 回显调用方已知目标供 facade 一致性核对。
                    "targetId": session_id,
                    // 公开状态明确限定为不可解析。
                    "state": "not-resolvable",
                    // 输出实际连续缺失时间。
                    "stableForMs": stable_for_ms,
                    // 输出完成采样次数。
                    "samples": samples,
                    // 输出总经过时间。
                    "elapsedMs": now_ms,
                    // 标记 capability 只读。
                    "readOnly": true,
                    // 每次采样均通过前景不变门禁。
                    "foregroundUnchanged": true,
                    // 输出关闭等待的精确证据范围。
                    "closureEvidence": "opaque-target-missing-from-visible-titled-window-inventory",
                    // 输出可机器验证的安全声明。
                    "safety": {
                        // 每次采样都重新发现目标。
                        "targetRefreshedPerSample": true,
                        // 全局取消与短切片暂停均生效。
                        "cancellable": true,
                        // 未发送任何窗口消息。
                        "windowMessagesSent": false,
                        // 未执行任何输入。
                        "inputEventsSent": false,
                        // 未写入任何文件。
                        "filesWritten": false,
                    },
                }));
            }
            // 条件未稳定时继续采样。
            WaitDecision::Pending => {}
            // 布尔状态机不会产生歧义判定。
            WaitDecision::Ambiguous => {
                // 防御性拒绝不可能的状态机输出。
                return Err(WindowClosedWaitErrorCode::OperationFailed.error(
                    // 不泄漏内部状态。
                    "The window closed wait state machine produced an invalid decision.",
                ));
            }
        }
        // 计算总 deadline 剩余时间。
        let remaining = timeout.saturating_sub(started.elapsed());
        // 暂停不超过轮询间隔或剩余总时间。
        let pause = remaining.min(Duration::from_millis(u64::from(input.poll_interval_ms)));
        // 使用短切片等待以响应取消。
        if !cancellable_pause(pause, &mut is_cancelled) {
            // 返回结构化取消结果。
            return Err(cancelled(samples));
        }
    }
}

// 对精确窗口公开执行只读关闭等待。
pub(crate) fn wait_for_closed(session_id: &str, input: &Value) -> AppResult<Value> {
    // 严格解析输入后组合真实窗口采样与取消能力。
    wait_with_sampler(
        // 传入调用方 opaque 目标。
        session_id,
        // 解析版本化输入。
        parse_input(input)?,
        // 每次重新枚举当前窗口 inventory。
        || sample_presence(session_id),
        // 复用进程级协作取消信号。
        cancellation::is_cancelled,
    )
}

// 验证输入、匹配、稳定、超时和取消边界。
#[cfg(test)]
mod tests {
    // 导入被测 Module 私有辅助函数。
    use super::*;
    // 导入正式 app facade、capability 和请求类型。
    use crate::{
        // 导入 app adapter 契约与统一 facade。
        adapters::{AppAdapter, AppFacadeAdapter},
        // 导入版本化 capability ID。
        capabilities,
        // 导入请求与 verb。
        domain::{CommandRequest, Verb},
    };

    // 构造稳定私有窗口夹具。
    fn fixture_window() -> WindowRecord {
        // 返回只用于纯匹配测试的窗口事实。
        WindowRecord {
            // legacy 字段不参与 canonical 匹配。
            session_id: "window:4660".to_owned(),
            // 保存私有窗口值。
            hwnd: 4660,
            // 保存公开标题。
            title: "Fixture Window".to_owned(),
            // 保存私有窗口类名。
            class_name: "FixtureClass".to_owned(),
            // 保存私有进程 ID。
            process_id: 42,
            // 保存公开进程名。
            process_name: Some("fixture.exe".to_owned()),
            // 标记窗口可见。
            visible: true,
            // 保存私有进程创建代际。
            process_creation_time: 123,
        }
    }

    // 构造最短合法等待输入。
    fn input(stable_for_ms: u32) -> WindowClosedWaitInput {
        // 返回只用于纯循环测试的配置。
        WindowClosedWaitInput {
            // 使用最短总超时。
            timeout_ms: 50,
            // 使用最短轮询间隔。
            poll_interval_ms: 25,
            // 使用调用方稳定时间。
            stable_for_ms,
        }
    }

    // 验证严格输入默认值与未知字段拒绝。
    #[test]
    fn input_contract_is_bounded_and_rejects_unknown_fields() {
        // 空对象取得版本一默认值。
        let defaults = match parse_input(&json!({})) {
            // 保存成功解析的默认输入。
            Ok(value) => value,
            // 默认输入失败时输出稳定测试诊断。
            Err(error) => panic!("default input failed: {}", error.message),
        };
        // 核对默认总超时。
        assert_eq!(defaults.timeout_ms, DEFAULT_TIMEOUT_MS);
        // 核对默认轮询间隔。
        assert_eq!(defaults.poll_interval_ms, DEFAULT_POLL_INTERVAL_MS);
        // 核对默认稳定时间。
        assert_eq!(defaults.stable_for_ms, DEFAULT_STABLE_FOR_MS);
        // 未知字段必须失败闭合。
        let unknown = parse_input(&json!({ "sleepMs": 10 }));
        // 核对稳定参数错误码。
        assert_eq!(
            unknown.err().map(|error| error.code),
            Some("INVALID_ARGUMENT")
        );
        // 越界稳定时间必须拒绝。
        let invalid = parse_input(&json!({ "timeoutMs": 50, "stableForMs": 51 }));
        // 核对稳定参数错误码。
        assert_eq!(
            invalid.err().map(|error| error.code),
            Some("INVALID_ARGUMENT")
        );
    }

    // 验证当前 inventory 的零一多分类。
    #[test]
    fn presence_classification_is_missing_unique_or_ambiguous() {
        // 构造唯一当前窗口。
        let window = fixture_window();
        // 生成 canonical 目标。
        let session_id = opaque_window_session_id(&window);
        // 空 inventory 必须分类为缺失。
        assert_eq!(classify_presence(&session_id, &[]), WindowPresence::Missing);
        // 单项 inventory 必须分类为唯一。
        assert_eq!(
            classify_presence(&session_id, std::slice::from_ref(&window)),
            WindowPresence::Unique
        );
        // 重复候选必须分类为歧义。
        assert_eq!(
            classify_presence(&session_id, &[window.clone(), window]),
            WindowPresence::Ambiguous
        );
    }

    // 验证零稳定时间在首次缺失时成功。
    #[test]
    fn immediate_missing_target_returns_scoped_closure_evidence() {
        // 执行不访问 Windows 的纯采样等待。
        let result = match wait_with_sampler(
            // 使用调用方已知的 canonical 形状。
            "s2:w:0123456789abcdef",
            // 不要求额外稳定时间。
            input(0),
            // 首次采样直接缺失。
            || Ok(WindowPresence::Missing),
            // 测试不请求取消。
            || false,
        ) {
            // 保存首次缺失的成功证据。
            Ok(value) => value,
            // 零稳定时间失败时输出固定诊断。
            Err(error) => panic!("zero-stability wait failed: {}", error.message),
        };
        // 成功状态不得夸大为原生窗口销毁证明。
        assert_eq!(result["state"], "not-resolvable");
        // 核对精确证据范围。
        assert_eq!(
            result["closureEvidence"],
            "opaque-target-missing-from-visible-titled-window-inventory"
        );
    }

    // 验证已缺失 canonical 窗口仍能进入专属等待 Module。
    #[test]
    fn facade_allows_missing_window_to_complete_closed_wait() {
        // 构造统一 app.read 请求。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // 选择只读 generic verb。
        request.operation = Some("read".to_owned());
        // 绑定不会命中当前 inventory 的 canonical 目标。
        request.target.insert(
            // 使用固定 sessionId 字段。
            "sessionId".to_owned(),
            // 使用合法 s2:w 形状。
            json!("s2:w:0000000000000000"),
        );
        // 声明关闭等待 capability。
        request.args.insert(
            // 使用固定 capability 字段。
            "capability".to_owned(),
            // 使用版本化 ID。
            json!(capabilities::WINDOW_CLOSED_WAIT),
        );
        // 要求首次缺失立即满足。
        request.args.insert(
            // 使用固定 input 包装字段。
            "input".to_owned(),
            // 提供合法零稳定时间。
            json!({ "timeoutMs": 50, "pollIntervalMs": 25, "stableForMs": 0 }),
        );
        // 通过正式 facade 执行只读等待。
        let result = match AppFacadeAdapter::new()
            // 调用统一 app surface。
            .run(&request)
        {
            // 已缺失窗口应成功返回 scoped evidence。
            Ok(value) => value,
            // facade 路由失败时输出固定诊断。
            Err(error) => panic!("closed wait facade failed: {}", error.message),
        };
        // 核对公开 facade data 中的状态。
        assert_eq!(result["data"]["state"], "not-resolvable");
        // 严格禁止公开原生身份字段。
        assert!(!result.to_string().contains("hwnd"));
    }

    // 验证歧义和取消均立即结构化失败。
    #[test]
    fn ambiguity_and_cancellation_fail_closed() {
        // 歧义采样不得继续等待。
        let ambiguous = wait_with_sampler(
            // 使用测试目标。
            "s2:w:0123456789abcdef",
            // 使用零稳定时间。
            input(0),
            // 返回歧义分类。
            || Ok(WindowPresence::Ambiguous),
            // 不请求取消。
            || false,
        );
        // 核对歧义错误码。
        assert_eq!(
            ambiguous.err().map(|error| error.code),
            Some("AMBIGUOUS_TARGET")
        );
        // 采样前取消必须返回取消错误。
        let cancelled = wait_with_sampler(
            // 使用测试目标。
            "s2:w:0123456789abcdef",
            // 使用零稳定时间。
            input(0),
            // 取消路径不得调用采样器。
            || panic!("cancelled wait must not sample"),
            // 立即请求取消。
            || true,
        );
        // 核对取消错误码。
        assert_eq!(cancelled.err().map(|error| error.code), Some("CANCELLED"));
    }

    // 验证持续存在的窗口按总 deadline 超时。
    #[test]
    fn present_target_times_out_with_safe_evidence() {
        // 持续返回唯一命中直到最短 deadline。
        let result = wait_with_sampler(
            // 使用测试目标。
            "s2:w:0123456789abcdef",
            // 要求完整五十毫秒缺失。
            input(50),
            // 模拟目标始终存在。
            || Ok(WindowPresence::Unique),
            // 不请求取消。
            || false,
        );
        // 提取预期超时错误。
        let error = match result {
            // 成功表示总 deadline 门禁失效。
            Ok(_) => panic!("present target must time out"),
            // 保存预期结构化超时。
            Err(error) => error,
        };
        // 核对统一超时码。
        assert_eq!(error.code, "TIMEOUT");
        // 核对最后观察不泄漏窗口事实。
        assert_eq!(error.details["lastObservation"], "unique");
    }
}
