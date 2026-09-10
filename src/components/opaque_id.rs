// 导入稳定字符串格式化接口。
use std::fmt::{self, Display, Formatter};

// 定义 C++ 对照实现使用的 FNV-1a 64 位偏移基数。
const FNV_OFFSET_BASIS: u64 = 14_695_981_039_346_656_037;
// 定义 C++ 对照实现使用的 FNV-1a 64 位质数。
const FNV_PRIME: u64 = 1_099_511_628_211;

// 按 C++ normalized_name 规则生成保守的 ASCII 关联提示。
pub(crate) fn normalized_name(value: &str) -> String {
    // 只保留 ASCII 字母与数字，并统一为小写。
    value
        // 遍历 UTF-8 字符；非 ASCII 字符不会进入关联提示。
        .bytes()
        // 只保留 C++ std::isalnum 在 ASCII 范围内接受的字节。
        .filter(|byte| byte.is_ascii_alphanumeric())
        // 转换为 ASCII 小写。
        .map(|byte| byte.to_ascii_lowercase())
        // 转换为字符以收集 String。
        .map(char::from)
        // 收集稳定关联提示。
        .collect()
}

// 声明公开 opaque ID 可表达的精确目标类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// 限定 provider-neutral 的稳定目标类别集合。
pub(crate) enum OpaqueTargetKind {
    // 表示当前 Windows 主机会话。
    Host,
    // 表示精确的已安装或运行中应用。
    Application,
    // 表示精确的运行中进程。
    Process,
    // 表示精确的顶层应用窗口。
    Window,
    // 表示一次可访问性检查快照中的精确节点。
    Element,
    // 表示精确的可认证控件。
    Control,
    // 表示精确的系统媒体会话。
    Media,
    // 表示精确的结构化文档。
    Document,
    // 表示经过授权代际绑定的独立交互会话。
    InteractiveSession,
    // 表示同一登录会话内由长操作 broker 拥有的任务句柄。
    Operation,
    // 结束稳定目标类别定义。
}

// 为稳定目标类别提供单字节协议映射。
impl OpaqueTargetKind {
    // 返回 JSON over stdio 契约中的目标类别码。
    const fn code(self) -> u8 {
        // 将领域类别映射到稳定协议字符。
        match self {
            // 映射主机目标。
            Self::Host => b'h',
            // 映射应用目标。
            Self::Application => b'a',
            // 映射进程目标。
            Self::Process => b'p',
            // 映射窗口目标。
            Self::Window => b'w',
            // 映射可访问性节点目标。
            Self::Element => b'e',
            // 映射控件目标。
            Self::Control => b'c',
            // 映射媒体目标。
            Self::Media => b'm',
            // 映射文档目标。
            Self::Document => b'd',
            // 映射独立交互会话目标。
            Self::InteractiveSession => b'i',
            // 映射长操作任务句柄。
            Self::Operation => b'o',
            // 结束类别码映射。
        }
        // 结束类别码读取。
    }

    // 从协议字符解析已知目标类别。
    fn from_code(code: u8) -> Option<Self> {
        // 仅接受契约明确列出的类别码。
        match code {
            // 解析主机目标。
            b'h' => Some(Self::Host),
            // 解析应用目标。
            b'a' => Some(Self::Application),
            // 解析进程目标。
            b'p' => Some(Self::Process),
            // 解析窗口目标。
            b'w' => Some(Self::Window),
            // 解析可访问性节点目标。
            b'e' => Some(Self::Element),
            // 解析控件目标。
            b'c' => Some(Self::Control),
            // 解析媒体目标。
            b'm' => Some(Self::Media),
            // 解析文档目标。
            b'd' => Some(Self::Document),
            // 解析独立交互会话目标。
            b'i' => Some(Self::InteractiveSession),
            // 解析长操作任务句柄。
            b'o' => Some(Self::Operation),
            // 拒绝未知目标类别。
            _ => None,
            // 结束类别码解析。
        }
        // 结束类别解析函数。
    }
    // 结束目标类别行为实现。
}

// 保存解析后的版本化目标类别与不透明指纹。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// 保持公共边界不携带 Win32、COM 或 provider 私有类型。
pub(crate) struct OpaqueTargetId {
    // 保存 provider-neutral 目标类别。
    kind: OpaqueTargetKind,
    // 保存不公开原生身份的 64 位指纹。
    fingerprint: u64,
    // 结束 opaque target ID 数据定义。
}

// 表示一次当前 inventory 中的 opaque 目标匹配结果。
pub(crate) enum OpaqueTargetMatch<'candidate, Candidate> {
    // 表示目标已消失、身份已变化或输入不 canonical。
    Missing,
    // 表示唯一当前候选已重新解析。
    Unique(&'candidate Candidate),
    // 表示两个或更多候选生成同一公开指纹。
    Ambiguous,
    // 结束目标匹配结果定义。
}

// 在当前 inventory 中以 fail-closed 语义唯一匹配 opaque 目标。
pub(crate) fn match_opaque_target<'candidate, Candidate>(
    // 接收调用方提供的 canonical s2 目标。
    public_target: &str,
    // 接收同一快照中的候选集合。
    candidates: impl IntoIterator<Item = &'candidate Candidate>,
    // 由 provider 使用私有事实为候选重新生成公开目标。
    mut candidate_target: impl FnMut(&Candidate) -> Option<String>,
) -> OpaqueTargetMatch<'candidate, Candidate> {
    // 非 canonical 输入不得进入 provider 匹配。
    if OpaqueTargetId::parse(public_target).is_none() {
        // 将旧版本或宽松别名视为未命中。
        return OpaqueTargetMatch::Missing;
        // 结束非 canonical 输入分支。
    }
    // 保留第一个命中候选，但不恢复或公开其私有身份。
    let mut matched = None;
    // 遍历当前 provider inventory 的候选。
    for candidate in candidates {
        // 只比较从当前私有事实重新生成的 canonical 目标。
        if candidate_target(candidate).as_deref() != Some(public_target) {
            // 跳过不同身份或无法生成身份的候选。
            continue;
            // 结束未命中候选分支。
        }
        // 第二个命中证明公开指纹在当前快照中有歧义。
        if matched.is_some() {
            // 立即 fail closed，不任取一个候选。
            return OpaqueTargetMatch::Ambiguous;
            // 结束多命中分支。
        }
        // 保留唯一候选以供完成遍历后返回。
        matched = Some(candidate);
        // 结束首个命中记录。
    }
    // 将零命中与唯一命中显式分类。
    match matched {
        // 返回唯一当前候选。
        Some(candidate) => OpaqueTargetMatch::Unique(candidate),
        // 零命中表示目标在本次使用时已过期。
        None => OpaqueTargetMatch::Missing,
        // 结束匹配分类。
    }
    // 结束 fail-closed opaque 目标匹配。
}

// 为 opaque target ID 提供生成和严格解析。
impl OpaqueTargetId {
    // 从私有稳定身份生成与 C++ 等价的 s2 指纹。
    pub(crate) fn new(kind: OpaqueTargetKind, identity: &str) -> Self {
        // 从 FNV-1a 标准偏移基数开始累积。
        let mut fingerprint = FNV_OFFSET_BASIS;
        // 严格按 UTF-8 身份字节更新指纹。
        for byte in identity.as_bytes() {
            // 应用 FNV-1a 的异或步骤。
            fingerprint ^= u64::from(*byte);
            // 应用与 C++ 无符号溢出等价的环绕乘法。
            fingerprint = fingerprint.wrapping_mul(FNV_PRIME);
            // 结束身份字节累积。
        }
        // 返回不携带原生身份的精确目标 ID。
        Self { kind, fingerprint }
        // 结束 opaque ID 生成。
    }

    // 解析 canonical s2 目标 ID 并拒绝宽松别名。
    pub(crate) fn parse(value: &str) -> Option<Self> {
        // 使用字节视图验证固定 ASCII 外壳。
        let bytes = value.as_bytes();
        // 要求 s2、单字节类别和 16 位指纹组成固定 21 字节。
        if bytes.len() != 21 || !bytes.starts_with(b"s2:") || bytes[4] != b':' {
            // 拒绝版本、长度或分隔符不符合契约的输入。
            return None;
            // 结束固定外壳验证。
        }
        // 解析并验证目标类别。
        let kind = OpaqueTargetKind::from_code(bytes[3])?;
        // 固定 ASCII 前缀保证该字节边界可安全切片。
        let hexadecimal = &value[5..];
        // 只接受 canonical 小写十六进制指纹。
        if !hexadecimal
            // 遍历指纹的每个 ASCII 字节。
            .bytes()
            // 拒绝大写、非十六进制和多字节字符。
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        // 进入非 canonical 输入分支。
        {
            // 拒绝非 canonical 指纹。
            return None;
            // 结束指纹字符验证。
        }
        // 将已验证指纹解析为固定宽度整数。
        let fingerprint = u64::from_str_radix(hexadecimal, 16).ok()?;
        // 返回结构化且可精确比较的目标 ID。
        Some(Self { kind, fingerprint })
        // 结束 opaque ID 解析。
    }

    // 返回解析后的 provider-neutral 目标类别。
    pub(crate) const fn kind(self) -> OpaqueTargetKind {
        // 仅暴露稳定类别，不公开私有身份或指纹实现细节。
        self.kind
        // 结束目标类别读取。
    }
    // 结束 opaque target ID 行为实现。
}

// 实现稳定 canonical 文本输出。
impl Display for OpaqueTargetId {
    // 将结构化目标 ID 写成 s2 JSON 边界字符串。
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // 固定输出小写且补齐 16 位指纹。
        write!(
            // 把结果写入调用者提供的格式化缓冲区。
            formatter,
            // 固定版本、类别和指纹的稳定形状。
            "s2:{}:{:016x}",
            // 将 ASCII 类别码转换为显示字符。
            char::from(self.kind.code()),
            // 输出不透明指纹。
            self.fingerprint // 结束稳定格式化调用。
        )
        // 结束 Display 格式化。
    }
    // 结束 canonical 文本实现。
}

// 仅在单元测试中验证跨语言身份契约。
#[cfg(test)]
// 声明 opaque ID Component 的测试集合。
mod tests {
    // 导入当前 Component 的私有测试接口。
    use super::*;

    // 验证 Rust 与 C++ 对照算法共享固定 golden。
    #[test]
    // 使用 C++ TextDocumentModule 的稳定身份覆盖真实类别。
    fn matches_cpp_fnv1a_golden() {
        // 生成 C++ 对照实现中的文本创建器应用 ID。
        let target = OpaqueTargetId::new(
            // 使用应用目标类别。
            OpaqueTargetKind::Application,
            // 使用 C++ TextDocumentModule 的固定私有身份。
            "text-document-creator",
            // 结束 golden 目标构造。
        );
        // 断言版本、类别和 FNV-1a 指纹逐字节一致。
        assert_eq!(target.to_string(), "s2:a:7b71b19b11d72b2d");
        // 结束跨语言 golden 测试。
    }

    // 验证 Rust 与 C++ 使用相同的保守名称规范化规则。
    #[test]
    // 只保留 ASCII 字母数字，拒绝依赖本地化大小写猜测。
    fn normalized_name_matches_cpp_ascii_rule() {
        // 标点、空格与非 ASCII 字符都必须被移除。
        assert_eq!(
            normalized_name("Adobe Photoshop 2026 中文!"),
            "adobephotoshop2026"
        );
        // 扩展名分隔符不进入提示。
        assert_eq!(normalized_name("NOTEPAD.EXE"), "notepadexe");
        // 纯非 ASCII 名称没有可用于保守关联的提示。
        assert_eq!(normalized_name("设置"), "");
    }

    // 验证全部稳定目标类别都能 canonical 往返。
    #[test]
    // 覆盖当前协议允许的十类精确目标。
    fn all_target_kinds_round_trip() {
        // 列出协议允许的全部目标类别。
        let kinds = [
            // 覆盖主机目标。
            OpaqueTargetKind::Host,
            // 覆盖应用目标。
            OpaqueTargetKind::Application,
            // 覆盖进程目标。
            OpaqueTargetKind::Process,
            // 覆盖窗口目标。
            OpaqueTargetKind::Window,
            // 覆盖快照可访问性节点目标。
            OpaqueTargetKind::Element,
            // 覆盖控件目标。
            OpaqueTargetKind::Control,
            // 覆盖媒体目标。
            OpaqueTargetKind::Media,
            // 覆盖文档目标。
            OpaqueTargetKind::Document,
            // 覆盖独立交互会话目标。
            OpaqueTargetKind::InteractiveSession,
            // 覆盖同会话长操作任务句柄。
            OpaqueTargetKind::Operation,
            // 结束目标类别清单。
        ];
        // 逐类别验证生成、格式化和解析。
        for kind in kinds {
            // 为当前类别生成稳定目标。
            let target = OpaqueTargetId::new(kind, "stable-private-identity");
            // 把目标格式化为公共字符串。
            let encoded = target.to_string();
            // 断言严格解析恢复同一结构化目标。
            assert_eq!(OpaqueTargetId::parse(&encoded), Some(target));
            // 结束逐类别验证。
        }
        // 结束全类别往返测试。
    }

    // 验证独立交互会话身份同时绑定系统会话代际与授权代际。
    #[test]
    // 重新授权必须生成新公开目标且不得暴露私有代际文本。
    fn interactive_session_identity_changes_with_authorization_generation() {
        // 生成第一代授权目标。
        let first = OpaqueTargetId::new(
            // 使用独立交互会话类别。
            OpaqueTargetKind::InteractiveSession,
            // 私有身份同时携带系统与授权代际。
            "system-generation-7|authorization-generation-a",
        )
        // 输出 canonical 公共目标。
        .to_string();
        // 生成同一系统会话的第二代授权目标。
        let second = OpaqueTargetId::new(
            // 使用相同目标类别。
            OpaqueTargetKind::InteractiveSession,
            // 只改变私有授权代际。
            "system-generation-7|authorization-generation-b",
        )
        // 输出 canonical 公共目标。
        .to_string();
        // 重新授权必须改变目标。
        assert_ne!(first, second);
        // 公共形状必须使用独立会话类别码。
        assert!(first.starts_with("s2:i:"));
        // 公共目标不得包含私有授权文本。
        assert!(!first.contains("authorization"));
    }

    // 验证解析器拒绝非 canonical 或未知目标。
    #[test]
    // 覆盖版本、类别、大小写、长度和尾随数据边界。
    fn rejects_noncanonical_ids() {
        // 列出必须拒绝的输入形状。
        let invalid = [
            // 拒绝旧 s1 版本。
            "s1:w:7b71b19b11d72b2d",
            // 拒绝未知目标类别。
            "s2:x:7b71b19b11d72b2d",
            // 拒绝大写十六进制别名。
            "s2:a:7B71B19B11D72B2D",
            // 拒绝不足 16 位的指纹。
            "s2:a:7b71b19b11d72b2",
            // 拒绝尾随数据。
            "s2:a:7b71b19b11d72b2d0",
            // 拒绝非十六进制字符。
            "s2:a:7b71b19b11d72b2g",
            // 结束无效输入清单。
        ];
        // 逐项验证严格拒绝。
        for value in invalid {
            // 断言无效输入不会产生目标 ID。
            assert_eq!(OpaqueTargetId::parse(value), None);
            // 结束无效输入验证。
        }
        // 结束严格解析测试。
    }

    // 验证公开指纹匹配的零、唯一和多命中语义。
    #[test]
    // 使用合成候选直接覆盖散列碰撞的 fail-closed 路径。
    fn matching_is_missing_unique_or_ambiguous() {
        // 生成当前测试使用的 canonical 目标。
        let target = OpaqueTargetId::new(OpaqueTargetKind::Window, "private-window").to_string();
        // 构造一个不同的 canonical 目标。
        let other = OpaqueTargetId::new(OpaqueTargetKind::Window, "other-window").to_string();
        // 构造一个未命中候选。
        let missing_candidates = [("other", other.as_str())];
        // 零命中必须分类为 Missing。
        assert!(matches!(
            // 执行零命中匹配。
            match_opaque_target(&target, &missing_candidates, |candidate| {
                // 返回候选夹具指定的公开目标。
                Some(candidate.1.to_owned())
                // 结束候选目标生成。
            }),
            // 要求 Missing 分类。
            OpaqueTargetMatch::Missing // 结束零命中断言。
        ));
        // 构造唯一命中候选。
        let unique_candidates = [("only", target.as_str())];
        // 执行唯一命中匹配。
        let unique = match_opaque_target(&target, &unique_candidates, |candidate| {
            // 返回候选夹具指定的公开目标。
            Some(candidate.1.to_owned())
            // 结束候选目标生成。
        });
        // 唯一命中必须返回当前候选引用。
        assert!(matches!(unique, OpaqueTargetMatch::Unique(candidate) if candidate.0 == "only"));
        // 构造两个生成同一公开指纹的碰撞候选。
        let ambiguous_candidates = [("first", target.as_str()), ("second", target.as_str())];
        // 多命中必须分类为 Ambiguous。
        assert!(matches!(
            // 执行碰撞候选匹配。
            match_opaque_target(&target, &ambiguous_candidates, |candidate| {
                // 返回候选夹具指定的公开目标。
                Some(candidate.1.to_owned())
                // 结束候选目标生成。
            }),
            // 要求 Ambiguous 分类。
            OpaqueTargetMatch::Ambiguous // 结束多命中断言。
        ));
        // 旧 s1 目标必须在候选枚举前当作 Missing。
        assert!(matches!(
            // 尝试匹配非 canonical 输入。
            match_opaque_target(
                "s1:w:0000000000000000",
                &ambiguous_candidates,
                |candidate| {
                    // 候选生成器不得改变输入版本判定。
                    Some(candidate.1.to_owned())
                    // 结束候选目标生成。
                }
            ),
            // 要求 Missing 分类。
            OpaqueTargetMatch::Missing // 结束非 canonical 断言。
        ));
        // 结束 opaque 匹配分类测试。
    }
    // 结束测试集合。
}
