//! 组合隔离树采样与稳定状态 Component，实现 provider-neutral UIA 元素等待。

// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "accessibility_wait_error.rs"]
mod error_code;

// 导入单调时钟与持续时间。
use std::time::{Duration, Instant};

// 导入严格输入反序列化。
use serde::Deserialize;
// 导入 JSON 值与构造器。
use serde_json::{Value, json};

// 导入树观察 Module、等待 Component、取消信号和统一错误。
use crate::{
    // 导入稳定等待与进程取消 Component。
    components::{
        // 导入只读 Accessibility 查询共用的 selector 语义。
        accessibility_selector::{AccessibilitySelector, AccessibilitySelectorError},
        // 导入当前调用取消信号。
        cancellation,
        // 导入稳定状态机和可取消暂停。
        stable_wait::{
            // 导入稳定判定。
            StableMatchTracker,
            // 导入状态机判定。
            WaitDecision,
            // 导入封闭采样结果。
            WaitObservation,
            // 导入可取消短切片暂停。
            cancellable_pause,
        },
    },
    // 导入统一错误类型。
    domain::{AppControlError, AppResult},
    // 导入现有隔离可访问性树 Module。
    modules::accessibility,
};

// 导入当前 Module 私有封闭错误码。
use error_code::AccessibilityWaitErrorCode;

// 固定默认总超时。
pub(crate) const DEFAULT_TIMEOUT_MS: u32 = 5_000;
// 固定默认轮询间隔。
pub(crate) const DEFAULT_POLL_INTERVAL_MS: u32 = 100;
// 固定默认稳定持续时间。
pub(crate) const DEFAULT_STABLE_FOR_MS: u32 = 300;
// 固定默认查询深度。
pub(crate) const DEFAULT_MAXIMUM_DEPTH: usize = 8;
// 固定默认查询节点数。
pub(crate) const DEFAULT_MAXIMUM_ITEMS: usize = 1_024;
// 限制单次 provider 树采样 deadline。
const MAXIMUM_SAMPLE_TIMEOUT_MS: u32 = 2_000;

// 返回默认总超时。
const fn default_timeout_ms() -> u32 {
    // 使用公开默认值。
    DEFAULT_TIMEOUT_MS
}

// 返回默认轮询间隔。
const fn default_poll_interval_ms() -> u32 {
    // 使用公开默认值。
    DEFAULT_POLL_INTERVAL_MS
}

// 返回默认稳定时间。
const fn default_stable_for_ms() -> u32 {
    // 使用公开默认值。
    DEFAULT_STABLE_FOR_MS
}

// 返回默认查询深度。
const fn default_maximum_depth() -> usize {
    // 使用公开默认值。
    DEFAULT_MAXIMUM_DEPTH
}

// 返回默认查询节点数。
const fn default_maximum_items() -> usize {
    // 使用公开默认值。
    DEFAULT_MAXIMUM_ITEMS
}

// 返回默认 ControlView。
fn default_view() -> String {
    // 创建独立字符串所有权。
    "control".to_owned()
}

// 保存调用方要求连续满足的公开状态。
#[derive(Debug, Deserialize)]
// 使用 camelCase 并拒绝未知字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RequiredElementState {
    // 要求 enabled 的目标布尔值。
    enabled: bool,
    // 要求 visible 的目标布尔值。
    visible: bool,
}

// 保存完整版本一 wait 输入。
#[derive(Debug, Deserialize)]
// 使用 camelCase 并拒绝未知字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ElementWaitInput {
    // 保存语义 selector。
    selector: AccessibilitySelector,
    // 保存状态条件。
    state: RequiredElementState,
    // 保存总超时。
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
    // 保存轮询间隔。
    #[serde(default = "default_poll_interval_ms")]
    poll_interval_ms: u32,
    // 保存连续稳定时间。
    #[serde(default = "default_stable_for_ms")]
    stable_for_ms: u32,
    // 保存查询深度边界。
    #[serde(default = "default_maximum_depth")]
    maximum_depth: usize,
    // 保存查询节点边界。
    #[serde(default = "default_maximum_items")]
    maximum_items: usize,
    // 保存 UIA view。
    #[serde(default = "default_view")]
    view: String,
}

// 验证 selector 至少包含一个有界语义字段。
fn validate_selector(selector: &AccessibilitySelector) -> AppResult<()> {
    // 把共享 Component 的封闭错误映射到当前 Module 公开消息。
    let message = match selector.validate() {
        // 合法 selector 不产生错误。
        Ok(()) => return Ok(()),
        // 保持既有字符串边界消息。
        Err(AccessibilitySelectorError::InvalidString) => {
            // 返回既有稳定诊断。
            "Element wait selector strings must contain 1..=512 UTF-8 bytes."
        }
        // 保持既有 control type 边界消息。
        Err(AccessibilitySelectorError::InvalidControlType) => {
            // 返回既有稳定诊断。
            "Element wait selector controlType must be positive."
        }
        // 保持既有非空 selector 消息。
        Err(AccessibilitySelectorError::Empty) => {
            // 返回既有稳定诊断。
            "Element wait selector requires at least one semantic field."
        }
    };
    // 返回不回显 selector 的参数错误。
    Err(AccessibilityWaitErrorCode::InvalidArgument.error(message))
}

// 严格解析并验证公开输入。
fn parse_input(value: &Value) -> AppResult<ElementWaitInput> {
    // 按 deny_unknown_fields 契约反序列化。
    let input = serde_json::from_value::<ElementWaitInput>(value.clone()).map_err(|_| {
        // 不回显潜在敏感 selector。
        AccessibilityWaitErrorCode::InvalidArgument.error(
            // 返回固定 schema 诊断。
            "Element wait input violates schema://ui/element-wait/v1.",
        )
    })?;
    // 验证 selector。
    validate_selector(&input.selector)?;
    // 验证所有时间边界及相互关系。
    if !(50..=30_000).contains(&input.timeout_ms)
        // 轮询不能形成忙循环或长时间不可取消。
        || !(25..=1_000).contains(&input.poll_interval_ms)
        // 稳定持续时间不能超过总 timeout。
        || input.stable_for_ms > input.timeout_ms
        // 轮询间隔不能超过总 timeout。
        || input.poll_interval_ms > input.timeout_ms
        // 查询深度保持与树 capability 一致。
        || input.maximum_depth > 20
        // 查询节点数保持与树 capability 一致。
        || !(1..=4_096).contains(&input.maximum_items)
        // 只允许 ControlView 或 RawView。
        || !matches!(input.view.as_str(), "control" | "raw")
    {
        // 返回统一有界参数错误。
        return Err(AccessibilityWaitErrorCode::InvalidArgument.error(
            // 说明全部封闭范围。
            "Element wait requires timeoutMs 50..30000, pollIntervalMs 25..1000, stableForMs 0..timeoutMs, maximumDepth 0..20, maximumItems 1..4096, and view control|raw.",
        ));
    }
    // 返回已验证输入。
    Ok(input)
}

// 核对一个公开节点是否满足 selector 的 AND 语义。
fn selector_matches(node: &Value, selector: &AccessibilitySelector) -> bool {
    // 委托共享 Component 保持等待与定位的 AND 语义一致。
    selector.matches_node(node)
}

// 核对唯一节点是否满足调用方状态条件。
fn state_matches(node: &Value, state: &RequiredElementState) -> bool {
    // enabled 必须与要求一致。
    node.get("enabled").and_then(Value::as_bool) == Some(state.enabled)
        // visible 映射为 offscreen 的逻辑反值。
        && node.get("offscreen").and_then(Value::as_bool) == Some(!state.visible)
}

// 从一份已验证公共树投影封闭匹配结果。
fn sample_observation<'tree>(
    // 接收当前树结果。
    tree: &'tree Value,
    // 接收 selector。
    selector: &AccessibilitySelector,
    // 接收状态条件。
    state: &RequiredElementState,
) -> AppResult<(WaitObservation<'tree>, Option<&'tree Value>)> {
    // 截断树无法证明唯一性。
    if tree.get("truncated").and_then(Value::as_bool) != Some(false) {
        // 返回明确不完整错误。
        return Err(AccessibilityWaitErrorCode::SearchIncomplete.error(
            // 不回显树内容。
            "The bounded accessibility sample could not prove selector uniqueness.",
        ));
    }
    // 读取已验证节点数组。
    let nodes = tree.get("nodes").and_then(Value::as_array).ok_or_else(|| {
        // 防御性收敛内部形状漂移。
        AccessibilityWaitErrorCode::OperationFailed.error(
            // 返回固定诊断。
            "The accessibility sample omitted its node array.",
        )
    })?;
    // 任一属性读取不完整都会破坏 zero/unique 证明。
    if nodes.iter().any(|node| {
        // 只接受明确完整值。
        node.get("propertyReadComplete").and_then(Value::as_bool) != Some(true)
    }) {
        // 返回搜索不完整错误。
        return Err(AccessibilityWaitErrorCode::SearchIncomplete.error(
            // 不猜测缺失属性。
            "The accessibility provider did not return every required structural property.",
        ));
    }
    // 收集满足 selector 的节点。
    let matches = nodes
        // 遍历有界节点。
        .iter()
        // 应用 AND selector。
        .filter(|node| selector_matches(node, selector))
        // 收集借用以分类零一多。
        .collect::<Vec<_>>();
    // 按匹配数量封闭分类。
    match matches.as_slice() {
        // 零匹配继续等待出现。
        [] => Ok((WaitObservation::Missing, None)),
        // 唯一匹配进入状态条件判定。
        [node] => {
            // 读取 canonical element ID。
            let element_id = node
                // 读取固定字段。
                .get("nodeId")
                // 要求字符串。
                .and_then(Value::as_str)
                // 内部树已验证，缺失仍防御性失败。
                .ok_or_else(|| {
                    // 返回内部操作失败。
                    AccessibilityWaitErrorCode::OperationFailed.error(
                        // 不回显节点。
                        "The accessibility sample omitted its element identity.",
                    )
                })?;
            // 返回唯一元素与状态事实。
            Ok((
                // 构造唯一采样。
                WaitObservation::Unique {
                    // 传入 opaque ID。
                    element_id,
                    // 计算状态条件。
                    condition_met: state_matches(node, state),
                },
                // 同时返回成功时需要的节点证据。
                Some(*node),
            ))
        }
        // 两个或更多匹配立即歧义失败。
        _ => Ok((WaitObservation::Ambiguous, None)),
    }
}

// 构造结构化取消错误。
fn cancelled(samples: u64) -> AppControlError {
    // 返回不含 selector 的取消证据。
    AccessibilityWaitErrorCode::Cancelled.with_details(
        // 说明等待未产生成功结果。
        "The UI element wait was cancelled before the condition became stable.",
        // 只公开安全计数。
        json!({ "samples": samples }),
    )
}

// 构造结构化总超时错误。
fn timed_out(input: &ElementWaitInput, samples: u64, last_observation: &str) -> AppControlError {
    // 返回不含 selector 或 provider identity 的超时证据。
    AccessibilityWaitErrorCode::Timeout.with_details(
        // 说明稳定条件未达成。
        "The UI element condition was not stable before timeout.",
        // 输出有界契约事实。
        json!({
            // 输出总 timeout。
            "timeoutMs": input.timeout_ms,
            // 输出要求的稳定持续时间。
            "stableForMs": input.stable_for_ms,
            // 输出采样次数。
            "samples": samples,
            // 输出封闭的最后观察分类。
            "lastObservation": last_observation,
        }),
    )
}

// 执行内部等待循环。
fn wait_internal(session_id: &str, input: &Value) -> AppResult<Value> {
    // 严格解析输入。
    let input = parse_input(input)?;
    // 记录单调操作起点。
    let started = Instant::now();
    // 保存总 timeout。
    let timeout = Duration::from_millis(u64::from(input.timeout_ms));
    // 创建稳定状态机。
    let mut tracker = StableMatchTracker::new(u64::from(input.stable_for_ms));
    // 初始化采样计数。
    let mut samples = 0_u64;
    // 初始化安全最后观察分类。
    let mut last_observation = "not-sampled";
    // 只在成功、错误、取消或 timeout 时退出。
    loop {
        // 在每次 provider 调用前检查取消。
        if cancellation::is_cancelled() {
            // 返回结构化取消。
            return Err(cancelled(samples));
        }
        // 计算已用时间。
        let elapsed = started.elapsed();
        // 总 timeout 到达时停止。
        if elapsed >= timeout {
            // 返回结构化 timeout 证据。
            return Err(timed_out(&input, samples, last_observation));
        }
        // 计算本次 provider 调用可用的剩余毫秒。
        let remaining_ms = timeout
            // 扣除已用时间。
            .saturating_sub(elapsed)
            // 转换为毫秒并限制到 u32。
            .as_millis()
            // 至少保留 1ms。
            .clamp(1, u128::from(u32::MAX)) as u32;
        // 单次 worker deadline 不能吞掉无限预算。
        let sample_timeout_ms = remaining_ms.min(MAXIMUM_SAMPLE_TIMEOUT_MS);
        // 通过现有 Job-bounded tree worker 重新发现并读取一次。
        let tree = accessibility::inspect_tree_internal(
            // 传入精确窗口目标。
            session_id,
            // 传入深度边界。
            input.maximum_depth,
            // 传入数量边界。
            input.maximum_items,
            // 传入 view。
            &input.view,
            // 传入本次硬 deadline。
            sample_timeout_ms,
        )?;
        // 记录完成的采样次数。
        samples = samples.saturating_add(1);
        // 将当前树分类为零一多及状态满足事实。
        let (observation, node) = sample_observation(&tree, &input.selector, &input.state)?;
        // 保存安全最后观察分类。
        last_observation = match observation {
            // 记录零匹配。
            WaitObservation::Missing => "missing",
            // 记录歧义。
            WaitObservation::Ambiguous => "ambiguous",
            // 记录唯一且满足。
            WaitObservation::Unique {
                condition_met: true,
                ..
            } => "unique-condition-met",
            // 记录唯一但状态未满足。
            WaitObservation::Unique {
                condition_met: false,
                ..
            } => "unique-condition-not-met",
        };
        // 计算从操作起点起的单调毫秒。
        let now_ms = started
            // 读取当前经过时间。
            .elapsed()
            // 转换为毫秒。
            .as_millis()
            // 饱和转换到 u64。
            .min(u128::from(u64::MAX)) as u64;
        // 推进稳定状态机。
        match tracker.observe(now_ms, observation) {
            // 多命中立即 fail closed。
            WaitDecision::Ambiguous => {
                // 返回不回显 selector 的歧义错误。
                return Err(AccessibilityWaitErrorCode::AmbiguousTarget.with_details(
                    // 说明语义 selector 不唯一。
                    "The UI element selector matched more than one element.",
                    // 只公开采样计数。
                    json!({ "samples": samples }),
                ));
            }
            // 达到稳定时间时返回成功证据。
            WaitDecision::Satisfied { stable_for_ms } => {
                // 唯一成功判定保证节点存在。
                let node = node.ok_or_else(|| {
                    // 防御性返回内部失败。
                    AccessibilityWaitErrorCode::OperationFailed.error(
                        // 不回显内部采样。
                        "The stable UI element sample omitted its node evidence.",
                    )
                })?;
                // 返回 provider-neutral 成功结果。
                return Ok(json!({
                    // 回显调用方已知目标供 facade 核对。
                    "targetId": session_id,
                    // 标记稳定成功。
                    "state": "stable",
                    // 输出安全节点证据。
                    "element": node,
                    // 输出实际稳定持续时间。
                    "stableForMs": stable_for_ms,
                    // 输出采样次数。
                    "samples": samples,
                    // 输出总经过时间。
                    "elapsedMs": now_ms,
                    // 标记只读。
                    "readOnly": true,
                    // 标记每次采样均通过前景门禁。
                    "foregroundUnchanged": true,
                    // 输出状态契约而不回显 selector。
                    "condition": {
                        // 输出 enabled 要求。
                        "enabled": input.state.enabled,
                        // 输出 visible 要求。
                        "visible": input.state.visible,
                    },
                    // 输出可机器验证的安全声明。
                    "safety": {
                        // 每次采样都重新发现窗口。
                        "targetRefreshedPerSample": true,
                        // provider 仅在隔离 worker 读取。
                        "providerTimeoutIsolation": "job-bounded-worker-per-sample",
                        // 全局取消和短切片暂停均生效。
                        "cancellable": true,
                        // 未查询写 pattern。
                        "writePatternsQueried": false,
                        // 未执行 UIA 写方法。
                        "uiaWritesPerformed": false,
                    },
                }));
            }
            // 未达稳定时间时继续。
            WaitDecision::Pending => {}
        }
        // 重新计算剩余总时间。
        let remaining = timeout.saturating_sub(started.elapsed());
        // 总时间耗尽时下一轮会返回 timeout。
        let pause = remaining.min(Duration::from_millis(u64::from(input.poll_interval_ms)));
        // 以短切片暂停并响应取消。
        if !cancellable_pause(pause, cancellation::is_cancelled) {
            // 返回结构化取消。
            return Err(cancelled(samples));
        }
    }
}

// 对精确窗口公开执行只读元素等待。
pub(crate) fn wait_for_element(session_id: &str, input: &Value) -> AppResult<Value> {
    // 执行并收敛共享 worker 私有错误。
    wait_internal(session_id, input).map_err(accessibility::public_error)
}

// 验证输入、匹配和错误边界。
#[cfg(test)]
mod tests {
    // 导入被测 helper。
    use super::*;

    // 构造最小合法输入。
    fn input() -> Value {
        // 返回严格 schema 对象。
        json!({
            // 使用单一 automation ID selector。
            "selector": { "automationId": "ready" },
            // 要求可见且 enabled。
            "state": { "enabled": true, "visible": true },
            // 使用短但合法 timeout。
            "timeoutMs": 100,
            // 使用最小轮询间隔。
            "pollIntervalMs": 25,
            // 无需额外稳定时间。
            "stableForMs": 0,
        })
    }

    // 构造已验证树形状。
    fn tree(nodes: Vec<Value>, truncated: bool) -> Value {
        // 返回 Module 公共树最小相关字段。
        json!({ "nodes": nodes, "truncated": truncated })
    }

    // 构造完整节点。
    fn node(id: &str, automation_id: &str, enabled: bool, offscreen: bool) -> Value {
        // 返回 wait 查询需要的全部公开字段。
        json!({
            "nodeId": id,
            "name": "fixture",
            "automationId": automation_id,
            "className": "Button",
            "frameworkId": "fixture",
            "controlType": 50000,
            "enabled": enabled,
            "offscreen": offscreen,
            "propertyReadComplete": true,
        })
    }

    // 验证严格参数边界。
    #[test]
    fn input_is_bounded_and_rejects_unknown_fields() {
        // 最小输入必须成功。
        assert!(parse_input(&input()).is_ok());
        // 注入未知字段。
        let mut unknown = input();
        // 添加 schema 外字段。
        unknown["sleepMs"] = Value::from(1);
        // 未知字段必须拒绝。
        assert!(parse_input(&unknown).is_err());
        // 注入空 selector。
        let mut empty = input();
        // 清空 selector。
        empty["selector"] = json!({});
        // 空 selector 必须拒绝。
        assert!(parse_input(&empty).is_err());
        // 注入超出总 timeout 的稳定时间。
        let mut invalid_stable = input();
        // 设置非法稳定时间。
        invalid_stable["stableForMs"] = Value::from(101);
        // 相互关系错误必须拒绝。
        assert!(parse_input(&invalid_stable).is_err());
    }

    // 验证取消与超时终止证据只包含有界安全事实。
    #[test]
    fn terminal_errors_keep_safe_bounded_evidence() -> AppResult<()> {
        // 解析一份合法输入供 timeout 证据使用。
        let input = parse_input(&input())?;
        // 构造三次采样后的取消错误。
        let cancelled = cancelled(3);
        // 取消错误必须保持稳定公开码。
        assert_eq!(cancelled.code, "CANCELLED");
        // 取消详情只需公开安全采样计数。
        assert_eq!(cancelled.details, json!({ "samples": 3 }));
        // 取消详情不得公开 selector。
        assert!(cancelled.details.get("selector").is_none());

        // 构造四次采样后仍缺失目标的总超时错误。
        let timed_out = timed_out(&input, 4, "missing");
        // 超时错误必须保持稳定公开码。
        assert_eq!(timed_out.code, "TIMEOUT");
        // 超时详情必须保持版本化安全事实。
        assert_eq!(
            timed_out.details,
            // 只包含调用方已知边界、计数与封闭观察分类。
            json!({
                "timeoutMs": 100,
                "stableForMs": 0,
                "samples": 4,
                "lastObservation": "missing",
            })
        );
        // 超时详情不得公开 selector。
        assert!(timed_out.details.get("selector").is_none());
        // 报告终止证据门禁通过。
        Ok(())
    }

    // 验证零一多匹配与状态条件。
    #[test]
    fn samples_are_classified_without_guessing() -> AppResult<()> {
        // 解析 selector 与状态。
        let input = parse_input(&input())?;
        // 零匹配必须返回 Missing。
        assert_eq!(
            sample_observation(&tree(Vec::new(), false), &input.selector, &input.state)?.0,
            WaitObservation::Missing
        );
        // 唯一满足节点必须返回 condition_met。
        let unique_tree = tree(vec![node("s2:e:one", "ready", true, false)], false);
        // 保存借用树寿命。
        let unique = sample_observation(&unique_tree, &input.selector, &input.state)?.0;
        // 核对唯一分类。
        assert_eq!(
            unique,
            WaitObservation::Unique {
                element_id: "s2:e:one",
                condition_met: true,
            }
        );
        // 多匹配必须返回 Ambiguous。
        let ambiguous_tree = tree(
            vec![
                node("s2:e:one", "ready", true, false),
                node("s2:e:two", "ready", true, false),
            ],
            false,
        );
        // 核对歧义分类。
        assert_eq!(
            sample_observation(&ambiguous_tree, &input.selector, &input.state)?.0,
            WaitObservation::Ambiguous
        );
        // 截断树必须返回搜索不完整。
        assert_eq!(
            sample_observation(&tree(Vec::new(), true), &input.selector, &input.state)
                .err()
                .map(|error| error.code),
            Some("SEARCH_INCOMPLETE")
        );
        // 返回测试成功。
        Ok(())
    }
}
