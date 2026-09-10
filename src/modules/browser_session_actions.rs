//! 聚合 Browser Session Module 的页面读取、元素身份、确认式写入与截图语义。

// 导入 worker 私有 selector 与 wait 条件。
use crate::components::browser_page_protocol::{BrowserElementSelector, BrowserWaitCondition};

// 导入同一 Module 的通用结果类型。
use super::{BrowserSessionCommandOutcome, BrowserSessionFailure};

// 表示 Module 允许调用方表达的 provider-neutral selector。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSemanticSelector {
    // 保存可选语义 role。
    role: Option<String>,
    // 保存可选可访问名称。
    name: Option<String>,
    // 保存可选可见文本。
    text: Option<String>,
    // 保存逐字匹配要求。
    exact: bool,
}

// 为 Module selector 提供封闭构造。
impl BrowserSemanticSelector {
    // 建立不含 native 查询语言的 selector。
    pub(crate) fn new(
        // 接收可选 role。
        role: Option<String>,
        // 接收可选名称。
        name: Option<String>,
        // 接收可选文本。
        text: Option<String>,
        // 接收逐字匹配要求。
        exact: bool,
    ) -> Self {
        // 保存 provider-neutral 字段。
        Self {
            // 保存 role。
            role,
            // 保存名称。
            name,
            // 保存文本。
            text,
            // 保存匹配方式。
            exact,
        }
    }

    // 转换为 worker 协议 selector，边界随后由 operation 统一验证。
    pub(super) fn into_worker(self) -> BrowserElementSelector {
        // 不附加 CSS、XPath 或脚本字段。
        BrowserElementSelector::new(self.role, self.name, self.text, self.exact)
    }
}

// 表示 Module 允许的封闭等待条件。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSemanticWaitCondition {
    // 等待文档 readyState complete。
    DocumentReady,
    // 等待语义元素出现。
    ElementPresent(BrowserSemanticSelector),
    // 等待有界文本出现。
    TextPresent {
        // 保存文本。
        text: String,
        // 保存逐字匹配要求。
        exact: bool,
    },
}

// 把 Module wait 条件转换为 worker 协议条件。
impl BrowserSemanticWaitCondition {
    // 执行封闭转换。
    pub(super) fn into_worker(self) -> BrowserWaitCondition {
        // 穷举全部允许条件。
        match self {
            // 映射文档 ready。
            Self::DocumentReady => BrowserWaitCondition::DocumentReady {},
            // 映射元素出现。
            Self::ElementPresent(selector) => BrowserWaitCondition::ElementPresent {
                // 转换 selector。
                selector: selector.into_worker(),
            },
            // 映射文本出现。
            Self::TextPresent { text, exact } => BrowserWaitCondition::TextPresent {
                // 保存文本。
                text,
                // 保存匹配方式。
                exact,
            },
        }
    }
}

// 保存任一页面命令的 Module 聚合结果。
pub(crate) struct BrowserSessionCommandReport<T> {
    // 保存封闭结果类别。
    pub(super) outcome: BrowserSessionCommandOutcome,
    // 保存可信完成事实。
    pub(super) completed: bool,
    // 保存安全重试事实。
    pub(super) retry_safe: bool,
    // 保存 worker 可能接受事实。
    pub(super) accepted_may_have_occurred: bool,
    // 保存当前导航代际。
    pub(super) navigation_generation: u64,
    // 保存可选成功数据。
    pub(super) data: Option<T>,
    // 保存可选安全错误。
    pub(super) error: Option<BrowserSessionFailure>,
    // 保存 parent 是否强制回收。
    pub(super) forced_reap: bool,
}

// 为页面命令结果提供只读投影。
impl<T> BrowserSessionCommandReport<T> {
    // 返回封闭结果类别。
    pub(crate) const fn outcome(&self) -> BrowserSessionCommandOutcome {
        // 复制枚举。
        self.outcome
    }

    // 返回可信完成事实。
    pub(crate) const fn completed(&self) -> bool {
        // 复制布尔值。
        self.completed
    }

    // 返回安全重试事实。
    pub(crate) const fn retry_safe(&self) -> bool {
        // 复制布尔值。
        self.retry_safe
    }

    // 返回 worker 可能接受事实。
    pub(crate) const fn accepted_may_have_occurred(&self) -> bool {
        // 复制布尔值。
        self.accepted_may_have_occurred
    }

    // 返回当前导航代际。
    pub(crate) const fn navigation_generation(&self) -> u64 {
        // 复制代际。
        self.navigation_generation
    }

    // 返回可选成功数据。
    pub(crate) const fn data(&self) -> Option<&T> {
        // 借用数据。
        self.data.as_ref()
    }

    // 返回可选安全错误。
    pub(crate) const fn error(&self) -> Option<&BrowserSessionFailure> {
        // 借用错误。
        self.error.as_ref()
    }

    // 返回强制回收事实。
    pub(crate) const fn forced_reap(&self) -> bool {
        // 复制布尔值。
        self.forced_reap
    }
}

// 保存 wait 成功事实。
pub(crate) struct BrowserWaitData {
    // 保存条件已满足事实。
    pub(super) condition_met: bool,
}

// 提供 wait 成功只读投影。
impl BrowserWaitData {
    // 返回条件满足事实。
    pub(crate) const fn condition_met(&self) -> bool {
        // 复制布尔值。
        self.condition_met
    }
}

// 保存一个公开元素摘要。
pub(crate) struct BrowserElementMatch {
    // 保存 Module 公开元素身份。
    pub(super) element_id: String,
    // 保存可选 role。
    pub(super) role: Option<String>,
    // 保存可选名称。
    pub(super) name: Option<String>,
    // 保存可选文本。
    pub(super) text: Option<String>,
    // 保存可用事实。
    pub(super) enabled: bool,
}

// 提供元素摘要只读投影。
impl BrowserElementMatch {
    // 返回公开元素身份。
    pub(crate) fn element_id(&self) -> &str {
        // 借用公开身份。
        &self.element_id
    }

    // 返回可选 role。
    pub(crate) fn role(&self) -> Option<&str> {
        // 借用 role。
        self.role.as_deref()
    }

    // 返回可选名称。
    pub(crate) fn name(&self) -> Option<&str> {
        // 借用名称。
        self.name.as_deref()
    }

    // 返回可选文本。
    pub(crate) fn text(&self) -> Option<&str> {
        // 借用文本。
        self.text.as_deref()
    }

    // 返回可用事实。
    pub(crate) const fn enabled(&self) -> bool {
        // 复制布尔值。
        self.enabled
    }
}

// 保存 query 成功数据。
pub(crate) struct BrowserQueryData {
    // 保存有界公开命中。
    pub(super) matches: Vec<BrowserElementMatch>,
    // 保存 worker 报告总命中数。
    pub(super) match_count: u32,
    // 保存是否截断。
    pub(super) truncated: bool,
}

// 提供 query 数据只读投影。
impl BrowserQueryData {
    // 返回公开命中切片。
    pub(crate) fn matches(&self) -> &[BrowserElementMatch] {
        // 借用有界集合。
        &self.matches
    }

    // 返回总命中数。
    pub(crate) const fn match_count(&self) -> u32 {
        // 复制计数。
        self.match_count
    }

    // 返回截断事实。
    pub(crate) const fn truncated(&self) -> bool {
        // 复制布尔值。
        self.truncated
    }
}

// 保存确认式点击成功事实。
pub(crate) struct BrowserClickData {
    // 保存点击完成事实。
    pub(super) clicked: bool,
}

// 提供点击数据只读投影。
impl BrowserClickData {
    // 返回点击完成事实。
    pub(crate) const fn clicked(&self) -> bool {
        // 复制布尔值。
        self.clicked
    }
}

// 保存确认式文本输入请求。
pub(crate) struct BrowserTypeRequest {
    // 保存待输入文本。
    pub(super) text: String,
    // 保存是否替换现有内容。
    pub(super) replace: bool,
    // 保存显式确认。
    pub(super) confirmed: bool,
}

// 为文本输入请求提供封闭构造。
impl BrowserTypeRequest {
    // 建立不含 native 键码或脚本的输入请求。
    pub(crate) fn new(text: String, replace: bool, confirmed: bool) -> Self {
        // 保存领域字段。
        Self {
            // 保存文本。
            text,
            // 保存替换要求。
            replace,
            // 保存确认。
            confirmed,
        }
    }
}

// 保存确认式输入成功事实。
pub(crate) struct BrowserTypeData {
    // 保存输入完成事实。
    pub(super) typed: bool,
    // 保存 UTF-8 字节数。
    pub(super) utf8_bytes: u32,
}

// 提供输入数据只读投影。
impl BrowserTypeData {
    // 返回输入完成事实。
    pub(crate) const fn typed(&self) -> bool {
        // 复制布尔值。
        self.typed
    }

    // 返回 UTF-8 字节数。
    pub(crate) const fn utf8_bytes(&self) -> u32 {
        // 复制字节数。
        self.utf8_bytes
    }
}

// 保存 PNG 截图成功数据。
pub(crate) struct BrowserScreenshotData {
    // 保存固定 MIME。
    pub(super) mime_type: String,
    // 保存有界 PNG Base64。
    pub(super) png_base64: String,
    // 保存原始字节数。
    pub(super) png_bytes: u64,
    // 保存宽度。
    pub(super) width: u32,
    // 保存高度。
    pub(super) height: u32,
    // 保存稳定摘要。
    pub(super) digest: String,
}

// 提供截图数据只读投影。
impl BrowserScreenshotData {
    // 返回 MIME。
    pub(crate) fn mime_type(&self) -> &str {
        // 借用固定 MIME。
        &self.mime_type
    }

    // 返回 PNG Base64。
    pub(crate) fn png_base64(&self) -> &str {
        // 借用有界内容。
        &self.png_base64
    }

    // 返回 PNG 字节数。
    pub(crate) const fn png_bytes(&self) -> u64 {
        // 复制字节数。
        self.png_bytes
    }

    // 返回宽度。
    pub(crate) const fn width(&self) -> u32 {
        // 复制宽度。
        self.width
    }

    // 返回高度。
    pub(crate) const fn height(&self) -> u32 {
        // 复制高度。
        self.height
    }

    // 返回摘要。
    pub(crate) fn digest(&self) -> &str {
        // 借用摘要。
        &self.digest
    }
}

// 动作执行与结果解析由同一 Module 的 sibling 文件承担。
