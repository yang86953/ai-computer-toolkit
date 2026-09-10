//! 版本化 capability 的 Rust 运行时单一注册表。

// 导入跨 catalog、assessment 与运行时策略共享的执行域枚举。
use crate::domain::ExecutionRealm;

#[path = "capabilities/ids.rs"]
mod ids;

pub(crate) use self::ids::*;

// 表示 capability 所属的公开 surface。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CapabilitySurface {
    // 表示统一应用 facade surface。
    App,
    // 表示隔离浏览器截图 surface。
    Browser,
    // 表示只读应用关系图发现 surface。
    ApplicationDiscovery,
    // 表示只读统一应用 session 聚合 surface。
    ApplicationSessionDiscovery,
    // 表示认证独立交互会话发现 surface。
    InteractiveSessionDiscovery,
    // 表示隔离只读可访问性观察 surface。
    Accessibility,
    // 表示只读进程观察 surface。
    Process,
    // 表示只读窗口观察 surface。
    Window,
    // 表示媒体会话只读与控制候选 surface。
    Media,
}

// 表示 facade generic verb 对应的 capability 动作类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CapabilityAction {
    // 表示发现类 capability。
    Discover,
    // 表示只读元数据类 capability。
    Read,
    // 表示创建类 capability。
    Create,
    // 表示应用变更类 capability。
    Apply,
    // 表示保存类 capability。
    Save,
    // 表示导出类 capability。
    Export,
    // 表示关闭类 capability。
    Close,
    // 表示截图类 capability。
    Screenshot,
    // 表示录制类 capability。
    Record,
}

// 为动作类别提供稳定的 JSON verb 文本。
impl CapabilityAction {
    // 定义公开 generic verb 的规范顺序。
    const PUBLIC_ORDER: &'static [Self] = &[
        // 发现动作在只读动作之前。
        Self::Discover,
        // 普通只读动作保持现有首个 generic verb 位置。
        Self::Read,
        // 创建动作排在只读动作之后。
        Self::Create,
        // 应用变更动作排在创建之后。
        Self::Apply,
        // 保存动作保持现有公开顺序。
        Self::Save,
        // 导出动作保持现有公开顺序。
        Self::Export,
        // 关闭动作保持现有公开顺序。
        Self::Close,
        // 截图动作保持现有公开顺序。
        Self::Screenshot,
        // 录制动作保持现有公开顺序。
        Self::Record,
    ];

    // 返回公开 generic verb。
    pub(crate) const fn as_str(self) -> &'static str {
        // 将封闭动作枚举映射到契约字符串。
        match self {
            // 输出发现 verb。
            Self::Discover => "discover",
            // 输出读取 verb。
            Self::Read => "read",
            // 输出创建 verb。
            Self::Create => "create",
            // 输出应用 verb。
            Self::Apply => "apply",
            // 输出保存 verb。
            Self::Save => "save",
            // 输出导出 verb。
            Self::Export => "export",
            // 输出关闭 verb。
            Self::Close => "close",
            // 输出截图 verb。
            Self::Screenshot => "screenshot",
            // 输出录制 verb。
            Self::Record => "record",
        }
    }

    // 返回动作是否会改变外部状态。
    pub(crate) const fn mutates(self) -> bool {
        // 发现与读取保持无副作用，其余动作均为 mutation。
        !matches!(self, Self::Discover | Self::Read)
    }
}

// 保存运行时路由与前台策略所需的最小 capability 元数据。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CapabilityDefinition {
    // 保存 capability 所属公开 surface。
    pub(crate) surface: CapabilitySurface,
    // 保存稳定版本化 ID。
    pub(crate) id: &'static str,
    // 保存 generic verb 动作类别。
    pub(crate) action: CapabilityAction,
    // 保存公开 descriptor 的执行方式。
    pub(crate) execution: &'static str,
    // 保存运行时策略使用的精确执行域。
    pub(crate) execution_realm: ExecutionRealm,
    // 标记公开 descriptor 是否声明可能需要前台确认。
    pub(crate) requires_foreground_consent: bool,
    // 标记执行前是否必须已有显式前台确认。
    pub(crate) requires_upfront_foreground_consent: bool,
    // 保存公开 descriptor 的输入 schema ID。
    pub(crate) input_schema: &'static str,
}

#[path = "capabilities/registry.rs"]
mod registry;

pub(crate) use self::registry::ALL;

// 按稳定 ID 查找运行时 capability 定义。
pub(crate) fn definition(id: &str) -> Option<&'static CapabilityDefinition> {
    // 只从单一注册表解析 capability。
    ALL.iter().find(|definition| definition.id == id)
}

// 在指定公开 surface 内查找 capability，阻止跨 surface 路由。
pub(crate) fn definition_for_surface(
    surface: CapabilitySurface,
    id: &str,
) -> Option<&'static CapabilityDefinition> {
    // 先按稳定 ID 查找，再核对 surface 所有权。
    definition(id).filter(|definition| definition.surface == surface)
}

// 从单一注册表投影指定 surface 实际公开的去重 action verb。
pub(crate) fn action_verbs_for_surface(surface: CapabilitySurface) -> Vec<&'static str> {
    // 按规范顺序筛选该 surface 至少一个 capability 使用的动作。
    CapabilityAction::PUBLIC_ORDER
        // 遍历封闭动作集合。
        .iter()
        // 复制轻量强类型枚举值。
        .copied()
        // 只保留当前 registry 确实使用的动作。
        .filter(|action| {
            // 任一同 surface 定义命中即可公开该 verb。
            ALL.iter().any(|definition| {
                // 同时核对 surface 所有权和强类型动作。
                definition.surface == surface && definition.action == *action
            })
        })
        // 在唯一边界把动作映射为稳定公开文本。
        .map(CapabilityAction::as_str)
        // 返回无重复且顺序稳定的小集合。
        .collect()
}

// 判断公共输入是否为 canonical 版本化 capability ID。
pub(crate) fn is_versioned_capability_id(value: &str) -> bool {
    // 要求名称与十进制版本由最后一个 @ 分隔。
    let Some((name, version)) = value.rsplit_once('@') else {
        // 缺少版本分隔符时拒绝。
        return false;
    };
    // 名称必须以小写 ASCII 字母开始。
    let starts_with_lowercase = name
        // 读取第一个字符。
        .chars()
        // 只接受小写 ASCII 字母。
        .next()
        // 空名称直接拒绝。
        .is_some_and(|character| character.is_ascii_lowercase());
    // 名称剩余字符只允许稳定 ID 字符集。
    let valid_name = name.chars().all(|character| {
        // 接受小写字母、数字和三种分隔符。
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '.' | '_' | '-')
    });
    // 版本必须为非空十进制整数且不得以零开头。
    let valid_version = version
        // 读取首字符。
        .chars()
        // 要求首位为 1..9。
        .next()
        // 空版本直接拒绝。
        .is_some_and(|character| matches!(character, '1'..='9'))
        // 同时要求全部字符为十进制数字。
        && version.chars().all(|character| character.is_ascii_digit());
    // 同时满足名称与版本约束才接受。
    starts_with_lowercase && valid_name && valid_version
}

// 声明注册表自身的不变量测试。
#[cfg(test)]
mod tests;
