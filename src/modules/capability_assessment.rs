//! provider-neutral capability assessment Module。

// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "capability_assessment_error.rs"]
mod error_code;

// 导入有序集合以合并多个只读 provider 的 capability 事实。
use std::collections::BTreeSet;

// 导入 JSON 构造接口。
use serde_json::{Value, json};

// 导入目标重新发现、注册表和结构化错误边界。
use crate::{
    // 只调用现有只读 provider 与 Windows Component。
    adapters::{
        // app facade 负责其当前 session 的 provider 唯一解析。
        AppFacadeAdapter,
        // Windows Component 负责 control 清单、前景与进程权限事实。
        windows::{
            IntegrityRelation, ProcessMetadataAccess, enumerate_process_inventory,
            filter_standard_edit_controls, foreground_hwnd, opaque_control_session_id,
        },
    },
    // 读取 Rust 主注册表中的稳定 capability ID。
    capabilities,
    // 解析两类互不兼容的 canonical 目标且不公开原生身份。
    components::{
        // 分类 Browser Session Module 自有的 128 位随机身份。
        browser_session_identity::{BrowserSessionIdentityShape, classify_browser_session_id},
        // 解析产品级通用 opaque 目标。
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
        // 投影窗口目标的保守身份强度。
        window_target_identity,
    },
    // 返回稳定 JSON over stdio 错误并共享强类型执行域。
    domain::{AppControlError, AppResult, ExecutionRealm, JsonMap},
};

// 导入当前 Module 私有封闭错误码。
use error_code::CapabilityAssessmentErrorCode;

// 复用应用关系图的完整重新发现与权限分类。
use super::{
    // 复用应用关系图的完整重新发现与权限分类。
    application_discovery::{AssessmentAvailability, resolve_assessment_target},
    // 只调用 Browser Session Module 私有 client 的无副作用存活查询。
    browser_session_client,
};

// 表示 assessment schema 的八种封闭决策。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Decision {
    // 表示已认证的后台路径可执行。
    ExecutableBackground,
    // 表示路径可用但尚未取得逐操作确认。
    ConfirmationRequired,
    // 表示只能在取得前台影响同意后继续。
    ForegroundConsentRequired,
    // 表示必须交给认证隔离 worker。
    #[allow(dead_code)]
    IsolationRequired,
    // 表示当前权限阻止安全路径。
    PermissionBlocked,
    // 表示目标或依赖当前不可用。
    Unavailable,
    // 表示精确目标不发布该 capability。
    Unsupported,
    // 表示公共目录没有该 capability。
    CapabilityGap,
}

// 表示 assessment 私有边界允许解析的精确目标类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AssessmentTargetKind {
    // 包装产品级通用 opaque 目标类别而不改变其协议。
    Opaque(OpaqueTargetKind),
    // 表示 Browser Session Module 自有的 128 位随机会话身份。
    BrowserSession,
}

// 为封闭决策提供稳定契约文本。
impl Decision {
    // 返回 schema 中的 canonical decision 值。
    const fn as_str(self) -> &'static str {
        // 显式覆盖全部八种决策，禁止隐式默认。
        match self {
            // 输出后台可执行决策。
            Self::ExecutableBackground => "executable-background",
            // 输出需要确认决策。
            Self::ConfirmationRequired => "confirmation-required",
            // 输出需要前台同意决策。
            Self::ForegroundConsentRequired => "foreground-consent-required",
            // 输出需要隔离决策。
            Self::IsolationRequired => "isolation-required",
            // 输出权限阻塞决策。
            Self::PermissionBlocked => "permission-blocked",
            // 输出不可用决策。
            Self::Unavailable => "unavailable",
            // 输出目标不支持决策。
            Self::Unsupported => "unsupported",
            // 输出目录缺口决策。
            Self::CapabilityGap => "capability-gap",
        }
    }
}

// 描述单个 capability 在 Rust 主路径上的静态 assessment 规则。
#[derive(Clone, Copy)]
struct CapabilityRule {
    // 保存版本化 capability ID。
    id: &'static str,
    // 保存精确目标必须具有的 opaque 类别。
    target_kind: AssessmentTargetKind,
    // 保存认证路径的 execution realm。
    execution_realm: ExecutionRealm,
    // 标记 Rust 主实现是否已经承接该 capability。
    rust_available: bool,
    // 标记逐操作确认要求。
    requires_confirmation: bool,
    // 标记前台影响同意要求。
    requires_foreground_consent: bool,
    // 标记 capability 是否纯只读。
    read_only: bool,
    // 保存 provider-neutral 约束摘要。
    scope: &'static str,
}

// 构造简洁的静态规则常量。
#[allow(clippy::too_many_arguments)]
const fn rule(
    // 接收版本化 capability ID。
    id: &'static str,
    // 接收目标类别。
    target_kind: OpaqueTargetKind,
    // 接收 execution realm。
    execution_realm: ExecutionRealm,
    // 接收 Rust 承接状态。
    rust_available: bool,
    // 接收逐操作确认要求。
    requires_confirmation: bool,
    // 接收前台同意要求。
    requires_foreground_consent: bool,
    // 接收只读分类。
    read_only: bool,
    // 接收约束摘要。
    scope: &'static str,
) -> CapabilityRule {
    // 返回不可变规则值。
    CapabilityRule {
        // 保存 capability ID。
        id,
        // 保存目标类别。
        target_kind: AssessmentTargetKind::Opaque(target_kind),
        // 保存执行域。
        execution_realm,
        // 保存迁移状态。
        rust_available,
        // 保存确认策略。
        requires_confirmation,
        // 保存前台策略。
        requires_foreground_consent,
        // 保存只读分类。
        read_only,
        // 保存范围摘要。
        scope,
    }
}

// 把静态 capability 规则拆分到独立子模块，保持单文件边界。
#[path = "capability_assessment_rules.rs"]
mod capability_assessment_rules;

// 导入显式 Rust 与待迁移 capability 规则表。
use capability_assessment_rules::RULES;

// 保存重新发现后供纯决策器消费的目标事实。
#[derive(Debug)]
struct ResolvedTarget {
    // 保存 opaque 目标类别。
    kind: AssessmentTargetKind,
    // 保存当前可用性。
    availability: AssessmentAvailability,
    // 保存精确目标实际发布的 capability。
    capabilities: BTreeSet<String>,
}

// 返回契约使用的 provider-neutral 目标类别名称。
const fn target_kind_name(kind: AssessmentTargetKind) -> &'static str {
    // 显式映射全部 opaque 类别。
    match kind {
        // 输出主机 session 类别。
        AssessmentTargetKind::Opaque(OpaqueTargetKind::Host) => "host-session",
        // 输出已安装应用类别。
        AssessmentTargetKind::Opaque(OpaqueTargetKind::Application) => "installed-application",
        // 输出运行进程类别。
        AssessmentTargetKind::Opaque(OpaqueTargetKind::Process) => "running-process",
        // 输出应用窗口类别。
        AssessmentTargetKind::Opaque(OpaqueTargetKind::Window) => "application-window",
        // 输出快照可访问性节点类别。
        AssessmentTargetKind::Opaque(OpaqueTargetKind::Element) => "accessibility-element",
        // 输出标准控件类别。
        AssessmentTargetKind::Opaque(OpaqueTargetKind::Control) => "standard-edit-control",
        // 输出系统媒体 session 类别。
        AssessmentTargetKind::Opaque(OpaqueTargetKind::Media) => "media-session",
        // 输出结构化文档类别。
        AssessmentTargetKind::Opaque(OpaqueTargetKind::Document) => "structured-document",
        // 输出独立交互会话类别。
        AssessmentTargetKind::Opaque(OpaqueTargetKind::InteractiveSession) => "interactive-session",
        // 输出同会话长操作任务类别。
        AssessmentTargetKind::Opaque(OpaqueTargetKind::Operation) => "long-operation",
        // 输出 Browser Session Module 自有身份类别。
        AssessmentTargetKind::BrowserSession => "browser-session",
    }
}

// 查找 capability 的封闭静态规则。
fn find_rule(capability: &str) -> Option<&'static CapabilityRule> {
    // 只接受显式登记的规则。
    RULES.iter().find(|rule| rule.id == capability)
}

// 向目标事实中加入各只读观察 surface 发布的 capability。
fn add_observation_capabilities(target: &mut ResolvedTarget) {
    // 按目标类别投影 Stage 3 已迁移能力。
    match target.kind {
        // host 发布三个完整发现入口。
        AssessmentTargetKind::Opaque(OpaqueTargetKind::Host) => {
            // 加入应用关系图发现。
            target
                .capabilities
                .insert(capabilities::APPLICATION_DISCOVER.to_owned());
            // 加入统一应用 session 聚合发现。
            target
                // 访问 host 当前发布集合。
                .capabilities
                // 发布已经迁入 Rust registry 的稳定 ID。
                .insert(capabilities::APPLICATION_SESSION_DISCOVER.to_owned());
            // 加入进程发现。
            target
                .capabilities
                .insert(capabilities::PROCESS_DISCOVER.to_owned());
            // 加入窗口发现。
            target
                .capabilities
                .insert(capabilities::WINDOW_DISCOVER.to_owned());
            // 加入认证独立交互会话发现。
            target
                // 访问 host 当前发布集合。
                .capabilities
                // 只发布已进入 Rust registry 的稳定 ID。
                .insert(capabilities::INTERACTIVE_SESSION_DISCOVER.to_owned());
            // 加入固定 Browser Session 打开 capability。
            target
                // 访问当前主机发布集合。
                .capabilities
                // 发布已经登记到 Rust registry 的稳定 ID。
                .insert(capabilities::BROWSER_SESSION_OPEN.to_owned());
        }
        // process 只发布元数据读取。
        AssessmentTargetKind::Opaque(OpaqueTargetKind::Process) => {
            // 加入进程元数据读取。
            target
                .capabilities
                .insert(capabilities::PROCESS_METADATA_READ.to_owned());
        }
        // window 发布元数据与可访问性树读取。
        AssessmentTargetKind::Opaque(OpaqueTargetKind::Window) => {
            // 加入窗口元数据读取。
            target
                .capabilities
                .insert(capabilities::WINDOW_METADATA_READ.to_owned());
            // 加入隔离可访问性树读取。
            target
                .capabilities
                .insert(capabilities::ACCESSIBILITY_TREE_READ.to_owned());
            // 加入 provider-neutral 语义元素定位。
            target
                // 访问当前发布集合。
                .capabilities
                // 插入稳定版本化 ID。
                .insert(capabilities::UI_ELEMENT_LOCATE.to_owned());
            // 加入已确认的 provider-neutral 语义元素动作。
            target
                // 访问当前发布集合。
                .capabilities
                // 插入稳定版本化 ID。
                .insert(capabilities::UI_ELEMENT_ACTION.to_owned());
            // 加入有界且可取消的语义元素等待。
            target
                .capabilities
                .insert(capabilities::UI_ELEMENT_WAIT.to_owned());
            // 精确窗口还发布只读关闭等待 capability。
            target
                // 访问当前发布集合。
                .capabilities
                // 插入稳定版本化 ID。
                .insert(capabilities::WINDOW_CLOSED_WAIT.to_owned());
            // 加入零帧精确窗口捕获预检。
            target
                .capabilities
                .insert(capabilities::WINDOW_CAPTURE_PREFLIGHT.to_owned());
            // 加入确认式隔离首帧元数据探针。
            target
                // 访问当前发布集合。
                .capabilities
                // 发布已经迁入 Rust registry 的稳定 ID。
                .insert(capabilities::WINDOW_CAPTURE_FRAME_PROBE.to_owned());
        }
        // 其他类别由精确 app provider 决定发布集合。
        _ => {}
    }
}

// 重新发现 canonical 标准 Edit 控件并传播静态权限状态。
fn resolve_control(session_id: &str) -> AppResult<ResolvedTarget> {
    // 记录只读扫描前的前景窗口。
    let foreground_before = foreground_hwnd();
    // 枚举全部标准 Edit，不使用原生调用方选择器。
    let controls = filter_standard_edit_controls(&JsonMap::new())?;
    // 完整保留相同 opaque 指纹的候选以检测碰撞。
    let matches = controls
        // 创建当前控件迭代器。
        .iter()
        // 从私有事实重新生成 canonical s2:c。
        .filter(|control| opaque_control_session_id(control) == session_id)
        // 收集命中引用。
        .collect::<Vec<_>>();
    // 多命中必须 fail closed。
    if matches.len() > 1 {
        // 返回稳定歧义错误。
        return Err(CapabilityAssessmentErrorCode::AmbiguousTarget.error(
            // 不公开 HWND、PID 或碰撞候选。
            "The opaque control target resolves to multiple current records.",
        ));
    }
    // 零命中表示控件 stale。
    let control = matches.first().ok_or_else(|| {
        // 返回稳定 stale 错误。
        CapabilityAssessmentErrorCode::StaleSession.error(
            // 仅说明当前扫描无法解析。
            "The opaque control target no longer resolves.",
        )
    })?;
    // 枚举当前进程权限事实以避免主动写探针。
    let processes = enumerate_process_inventory(4096)?;
    // 以 PID 与创建时间绑定同一进程实例。
    let process = processes.records.iter().find(|process| {
        // 同时匹配私有 PID 与生命周期代际。
        process.process_id == control.process_id
            // 创建时间必须一致。
            && process.process_creation_time == control.process_creation_time
    });
    // 将进程权限与完整性关系映射为静态可用性。
    let availability = process.map_or(
        // 无法证明代际时保持 unavailable。
        AssessmentAvailability::Unavailable,
        // 唯一进程事实只读映射权限。
        |process| {
            // higher-integrity 与显式访问拒绝都不得试写。
            if process.integrity_relation == IntegrityRelation::Higher
                // 合并 Windows 明确拒绝状态。
                || process.metadata_access == ProcessMetadataAccess::PermissionBlocked
            {
                // 返回权限阻塞。
                AssessmentAvailability::PermissionBlocked
            // 元数据暂不可用时保持 unavailable。
            } else if process.metadata_access == ProcessMetadataAccess::Unavailable {
                // 返回不可用分类。
                AssessmentAvailability::Unavailable
            } else {
                // 同级或较低完整性且元数据可用。
                AssessmentAvailability::Available
            }
        },
    );
    // 记录只读扫描后的前景窗口。
    let foreground_after = foreground_hwnd();
    // 前景发生变化时 assessment 不得继续。
    if foreground_before != foreground_after {
        // 返回稳定宿主干扰错误。
        return Err(
            CapabilityAssessmentErrorCode::HostInterferenceDetected.error(
                // 明确没有触发写入降级。
                "The foreground target changed during control assessment.",
            ),
        );
    }
    // 构造精确控件发布集合。
    let capabilities = BTreeSet::from([capabilities::UI_TEXT_INPUT.to_owned()]);
    // 返回 provider-neutral 控件事实。
    Ok(ResolvedTarget {
        // 标记标准控件类别。
        kind: AssessmentTargetKind::Opaque(OpaqueTargetKind::Control),
        // 传播静态权限状态。
        availability,
        // 仅发布当前认证的标准 Edit capability。
        capabilities,
    })
}

// 通过私有 session.inspect 重新认证 Browser Session Module 自有身份。
fn resolve_browser_session(session_id: &str) -> AppResult<ResolvedTarget> {
    // 注入唯一允许的生产 Query，便于纯测试证明不会调用 mutation。
    resolve_browser_session_with(session_id, browser_session_client::inspect_session)
}

// 使用窄只读端口解析 Browser Session，保持错误码原样传播。
fn resolve_browser_session_with<Inspect>(
    // 借用已经完成形状分类的 canonical identity。
    session_id: &str,
    // 接收只能执行 session.inspect 的窄 Query callable。
    inspect_session: Inspect,
) -> AppResult<ResolvedTarget>
where
    // 端口只取得 identity 与有界预算并返回无数据存活事实。
    Inspect: FnOnce(&str, u32) -> AppResult<()>,
{
    // Query 只读取当前 Broker 代际 live registry，绝不调用 open 或 close mutation。
    inspect_session(
        // 只传递调用方已知的 canonical opaque identity。
        session_id,
        // 使用 client 与公开生命周期默认值同源的固定总预算。
        browser_session_client::DEFAULT_TIMEOUT_MS,
    )?;
    // 只有可信 live 查询完成后才发布当前会话的固定 Broker capability。
    Ok(ResolvedTarget {
        // 保持 Browser Session 身份为 assessment 私有类别。
        kind: AssessmentTargetKind::BrowserSession,
        // live registry 查询成功证明目标当前可用。
        availability: AssessmentAvailability::Available,
        // 发布当前精确会话支持的生命周期与页面 capability。
        capabilities: BTreeSet::from([
            // 发布固定 Broker 关闭 Command。
            capabilities::BROWSER_SESSION_CLOSE.to_owned(),
            // 发布确认式页面导航 Command。
            capabilities::BROWSER_PAGE_NAVIGATE.to_owned(),
            // 发布无隐藏副作用的页面等待 Query。
            capabilities::BROWSER_PAGE_WAIT.to_owned(),
            // 发布有界 provider-neutral 页面查询 Query。
            capabilities::BROWSER_PAGE_QUERY.to_owned(),
            // 发布确认式浏览器元素点击 Command。
            capabilities::BROWSER_ELEMENT_CLICK.to_owned(),
            // 发布确认式浏览器元素输入 Command。
            capabilities::BROWSER_ELEMENT_TYPE.to_owned(),
            // 发布有界只读页面截图 Query。
            capabilities::BROWSER_PAGE_SCREENSHOT.to_owned(),
        ]),
    })
}

// 重新发现目标并合并 app provider 与 Stage 3 观察 capability。
fn resolve_target(session_id: &str) -> AppResult<ResolvedTarget> {
    // 在通用 opaque parser 前分类不兼容的 Browser Session 128 位外壳。
    match classify_browser_session_id(session_id) {
        // canonical Browser Session 必须通过私有只读 Query 认证 live 事实。
        BrowserSessionIdentityShape::Canonical => return resolve_browser_session(session_id),
        // 看似 Browser Session 但外壳畸形时不得连接或启动 Broker。
        BrowserSessionIdentityShape::Malformed => {
            // 返回不回显输入的结构化参数错误。
            return Err(AppControlError::new(
                // 使用公开输入错误码。
                "INVALID_ARGUMENT",
                // 不解释随机 identity 的私有布局。
                "The browser session assessment target is not canonical.",
            ));
        }
        // 其他目标继续使用产品级通用 opaque 解析。
        BrowserSessionIdentityShape::NotBrowserSession => {}
    }
    // 严格解析 canonical 目标以确定 provider 路由。
    let parsed = OpaqueTargetId::parse(session_id).ok_or_else(|| {
        // 旧版本、别名或畸形 ID 均按 stale 拒绝。
        CapabilityAssessmentErrorCode::StaleSession.error(
            // 不回显或解释私有身份。
            "The assessment target is not a current canonical opaque session.",
        )
    })?;
    // 标准控件使用其专用只读重新发现路径。
    if parsed.kind() == OpaqueTargetKind::Control {
        // 返回控件解析结果。
        return resolve_control(session_id);
    }
    // 查询全部当前 app provider 的唯一 session 命中。
    let facade = AppFacadeAdapter::new();
    // 取得 app provider 当前发布的能力集合。
    let provider_session = facade.resolve_assessment_session(session_id)?;
    // 对关系图拥有的四类目标执行完整重新捕获。
    let inventory_target = if matches!(
        // 读取 opaque 类别。
        parsed.kind(),
        // 只选择关系图拥有的类别。
        OpaqueTargetKind::Host
            | OpaqueTargetKind::Application
            | OpaqueTargetKind::Process
            | OpaqueTargetKind::Window
    ) {
        // 捕获关系图并解析目标。
        match resolve_assessment_target(session_id) {
            // 保存唯一关系图命中。
            Ok(target) => Some(target),
            // 特殊 app provider session 可不出现在已安装应用图中。
            Err(error)
                if error.code == CapabilityAssessmentErrorCode::StaleSession.as_str()
                    && provider_session.is_some() =>
            {
                // 精确 app provider 已拥有当前目标时忽略关系图 stale。
                None
            }
            // 其他失败必须原样传播。
            Err(error) => return Err(error),
        }
    } else {
        // 文档等类别仅由精确 app provider 拥有。
        None
    };
    // 两类 provider 均未命中时目标已经 stale。
    if inventory_target.is_none() && provider_session.is_none() {
        // 快照节点不可跨 inspection 复用，媒体仍待 Rust 迁移。
        return Err(CapabilityAssessmentErrorCode::StaleSession.error(
            // 不把无法重新发现误报为可用。
            "The opaque target no longer resolves through a current Rust provider.",
        ));
    }
    // 优先采用关系图传播的权限状态。
    let availability = inventory_target.map_or(
        // app provider 唯一命中默认可用。
        AssessmentAvailability::Available,
        // 读取关系图事实。
        |target| target.availability,
    );
    // 创建精确目标发布集合。
    let mut published = provider_session
        // 取得 app provider 集合。
        .map(|session| session.capabilities)
        // 零 app provider 命中时使用空集合。
        .unwrap_or_default()
        // 转换为有序去重集合。
        .into_iter()
        // 收集能力 ID。
        .collect::<BTreeSet<_>>();
    // 目标类别必须来自 canonical ID，且 provider 命中不得改变类别。
    let mut target = ResolvedTarget {
        // 保存已解析类别。
        kind: AssessmentTargetKind::Opaque(parsed.kind()),
        // 保存当前可用性。
        availability,
        // 暂存 app provider 发布集合。
        capabilities: std::mem::take(&mut published),
    };
    // 加入 Stage 3 观察 surface 发布能力。
    add_observation_capabilities(&mut target);
    // 返回合并后的最小事实。
    Ok(target)
}

// 生成 assessment JSON，纯函数不触碰任何 provider。
fn evaluate(capability: &str, target_id: &str, target: &ResolvedTarget) -> Value {
    // 查找显式规则；未知 capability 稳定返回 gap。
    let Some(rule) = find_rule(capability) else {
        // 返回 capability-gap 结果。
        return assessment_json(
            // 传播调用方 capability。
            capability,
            // 传播 opaque 目标。
            target_id,
            // 传播实际目标类别。
            target.kind,
            // 使用目录缺口决策。
            Decision::CapabilityGap,
            // 缺口没有执行域。
            ExecutionRealm::None,
            // 不要求确认。
            false,
            // 不要求前台同意。
            false,
            // 提供稳定原因。
            &["capability-not-published-in-catalog"],
            // 使用空范围。
            "none",
            // 未发布 capability 不宣称只读。
            false,
            // 提供迁移状态证据。
            "not-published",
        );
    };
    // 精确目标类别不匹配时返回 unsupported。
    if rule.target_kind != target.kind {
        // 构造目标类别不支持结果。
        return assessment_json(
            // 传播 capability。
            capability,
            // 传播目标。
            target_id,
            // 传播实际类别。
            target.kind,
            // 使用不支持决策。
            Decision::Unsupported,
            // 不允许进入任何执行域。
            ExecutionRealm::None,
            // 传播 capability 确认元数据。
            rule.requires_confirmation,
            // 传播前台策略。
            rule.requires_foreground_consent,
            // 提供稳定不支持原因。
            &["capability-not-published-for-exact-target-kind"],
            // 传播规则范围。
            rule.scope,
            // 传播只读分类。
            rule.read_only,
            // 标记目标类别不支持。
            "target-kind-unsupported",
        );
    }
    // 权限阻塞必须优先于路径可用性。
    if target.availability == AssessmentAvailability::PermissionBlocked {
        // 返回不提权的权限阻塞结果。
        return assessment_json(
            // 传播 capability。
            capability,
            // 传播目标。
            target_id,
            // 传播类别。
            target.kind,
            // 使用权限阻塞决策。
            Decision::PermissionBlocked,
            // 权限阻塞时 realm 固定 none。
            ExecutionRealm::None,
            // 传播确认元数据。
            rule.requires_confirmation,
            // 传播前台元数据。
            rule.requires_foreground_consent,
            // 提供稳定权限原因。
            &["target-permission-denied-for-read-only-probe"],
            // 传播范围。
            rule.scope,
            // 传播只读分类。
            rule.read_only,
            // 标记权限阻塞。
            "permission-blocked",
        );
    }
    // 目标 provider 不可用时不得尝试替代路径。
    if target.availability == AssessmentAvailability::Unavailable {
        // 返回当前不可用结果。
        return assessment_json(
            // 传播 capability。
            capability,
            // 传播目标。
            target_id,
            // 传播类别。
            target.kind,
            // 使用 unavailable 决策。
            Decision::Unavailable,
            // 不进入执行域。
            ExecutionRealm::None,
            // 传播确认元数据。
            rule.requires_confirmation,
            // 传播前台元数据。
            rule.requires_foreground_consent,
            // 提供稳定不可用原因。
            &["target-read-only-provider-currently-unavailable"],
            // 传播范围。
            rule.scope,
            // 传播只读分类。
            rule.read_only,
            // 标记目标不可用。
            "target-unavailable",
        );
    }
    // 尚未迁回 Rust 的 capability 只能报告 unavailable。
    if !rule.rust_available {
        // 返回迁移待办结果。
        return assessment_json(
            // 传播 capability。
            capability,
            // 传播目标。
            target_id,
            // 传播类别。
            target.kind,
            // 使用 unavailable 决策。
            Decision::Unavailable,
            // 未迁移路径 realm 固定 none。
            ExecutionRealm::None,
            // 传播确认元数据。
            rule.requires_confirmation,
            // 传播前台元数据。
            rule.requires_foreground_consent,
            // 明确没有认证 Rust 路径且不回退。
            &[
                "exact-target-has-no-certified-rust-route",
                "cpp-capability-retained-as-compatibility-evidence",
            ],
            // 传播范围。
            rule.scope,
            // 传播只读分类。
            rule.read_only,
            // 标记迁移尚未认证。
            "rust-migration-not-yet-certified",
        );
    }
    // 已迁移 capability 仍必须由精确目标当前发布。
    if !target.capabilities.contains(capability) {
        // 返回精确 session 不支持结果。
        return assessment_json(
            // 传播 capability。
            capability,
            // 传播目标。
            target_id,
            // 传播类别。
            target.kind,
            // 使用 unsupported 决策。
            Decision::Unsupported,
            // 不允许进入执行域。
            ExecutionRealm::None,
            // 传播确认元数据。
            rule.requires_confirmation,
            // 传播前台元数据。
            rule.requires_foreground_consent,
            // 提供精确 session 发布缺口原因。
            &["exact-target-does-not-publish-capability"],
            // 传播范围。
            rule.scope,
            // 传播只读分类。
            rule.read_only,
            // 标记 session 不支持。
            "exact-target-unsupported",
        );
    }
    // 前台路径必须先返回独立 consent 决策。
    if rule.requires_foreground_consent {
        // 返回前台同意门禁。
        return assessment_json(
            // 传播 capability。
            capability,
            // 传播目标。
            target_id,
            // 传播类别。
            target.kind,
            // 使用 foreground consent 决策。
            Decision::ForegroundConsentRequired,
            // 传播认证前台域。
            rule.execution_realm,
            // 前台写仍要求逐操作确认。
            rule.requires_confirmation,
            // 标记需要前台同意。
            true,
            // 提供稳定前台原因。
            &["explicit-foreground-impact-consent-required"],
            // 传播范围。
            rule.scope,
            // 传播只读分类。
            rule.read_only,
            // 标记 Rust 路径等待前台授权。
            "rust-available-awaiting-foreground-consent",
        );
    }
    // 其他确认型能力返回 confirmation-required。
    if rule.requires_confirmation {
        // 返回逐操作确认门禁。
        return assessment_json(
            // 传播 capability。
            capability,
            // 传播目标。
            target_id,
            // 传播类别。
            target.kind,
            // 使用 confirmation 决策。
            Decision::ConfirmationRequired,
            // 传播认证执行域。
            rule.execution_realm,
            // 标记需要确认。
            true,
            // 无前台同意要求。
            false,
            // 提供稳定确认原因。
            &["explicit-confirmation-required-for-capability"],
            // 传播范围。
            rule.scope,
            // 传播只读分类。
            rule.read_only,
            // 标记 Rust 路径等待确认。
            "rust-available-awaiting-confirmation",
        );
    }
    // 无确认只读能力可在认证 realm 后台执行。
    assessment_json(
        // 传播 capability。
        capability,
        // 传播目标。
        target_id,
        // 传播类别。
        target.kind,
        // 使用后台可执行决策。
        Decision::ExecutableBackground,
        // 传播认证执行域。
        rule.execution_realm,
        // 不要求确认。
        false,
        // 不要求前台同意。
        false,
        // 提供稳定只读原因。
        &["read-only-certified-implementation-available"],
        // 传播范围。
        rule.scope,
        // 传播只读分类。
        rule.read_only,
        // 标记 Rust 路径可用。
        "rust-available",
    )
}

// 构造与 capability-assessment schema 一致的公共结果。
#[allow(clippy::too_many_arguments)]
fn assessment_json(
    // 接收 capability ID。
    capability: &str,
    // 接收 opaque 目标。
    target_id: &str,
    // 接收实际目标类别。
    target_kind: AssessmentTargetKind,
    // 接收封闭决策。
    decision: Decision,
    // 接收 execution realm。
    execution_realm: ExecutionRealm,
    // 接收确认标记。
    requires_confirmation: bool,
    // 接收前台标记。
    requires_foreground_consent: bool,
    // 接收稳定原因列表。
    reasons: &[&str],
    // 接收范围摘要。
    scope: &str,
    // 接收只读分类。
    read_only: bool,
    // 接收实现状态证据。
    implementation_state: &str,
) -> Value {
    // 返回不含任何原生事实的稳定对象。
    json!({
        // 标记成功 assessment envelope。
        "ok": true,
        // 固定公共控制契约版本。
        "contractVersion": "act/control/v1",
        // 回显调用方版本化 capability。
        "capability": capability,
        // 回显调用方已知 opaque 目标。
        "targetId": target_id,
        // 输出封闭决策。
        "decision": decision.as_str(),
        // 输出封闭执行域。
        "executionRealm": execution_realm,
        // 输出逐操作确认要求。
        "requiresConfirmation": requires_confirmation,
        // 输出前台影响同意要求。
        "requiresForegroundConsent": requires_foreground_consent,
        // 输出非空稳定原因。
        "reasons": reasons,
        // 输出 provider-neutral 约束。
        "constraints": {
            // 输出能力范围摘要。
            "scope": scope,
            // 输出只读分类。
            "readOnly": read_only,
            // 明确禁止未认证降级。
            "noFallback": true,
        },
        // 输出不含原生身份的审计证据。
        "evidence": {
            // 输出稳定目标类别。
            "targetKind": target_kind_name(target_kind),
            // 输出 Rust 迁移状态。
            "implementationState": implementation_state,
            // 窗口目标公开 token 回收停止线，其余类别为 null。
            "targetIdentityStrength": matches!(target_kind, AssessmentTargetKind::Opaque(OpaqueTargetKind::Window)).then(window_target_identity::public_assurance),
            // assessment 永不授权前台激活。
            "foregroundActivationAllowed": false,
            // assessment 永不授权输入。
            "inputAllowed": false,
        },
    })
}

// 执行公开、无副作用的 Rust capability assessment。
pub(crate) fn assess(capability: &str, session_id: &str) -> AppResult<Value> {
    // 每次调用先重新发现并唯一解析目标。
    let target = resolve_target(session_id)?;
    // 再执行纯 capability 决策，不触发任何写路径。
    Ok(evaluate(capability, session_id, &target))
}

// 把测试拆分到独立子模块，保持生产 Module 文件低于 900 行。
#[cfg(test)]
#[path = "capability_assessment_tests.rs"]
mod tests;
