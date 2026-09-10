//! 封装 UIA RuntimeId 所有权、验证与 opaque element ID 生成。

// 导入 UIA RuntimeId 所需的 Windows 私有类型与 SAFEARRAY API。
use windows::Win32::{
    // 导入通用失败 HRESULT。
    Foundation::E_FAIL,
    // 导入 SAFEARRAY 与读取函数。
    System::{
        // 导入 SAFEARRAY 类型。
        Com::SAFEARRAY,
        // 导入 SAFEARRAY 边界、元素、类型和释放函数。
        Ole::{
            // 导入 SAFEARRAY 释放函数。
            SafeArrayDestroy,
            // 导入维度读取函数。
            SafeArrayGetDim,
            // 导入元素读取函数。
            SafeArrayGetElement,
            // 导入下界读取函数。
            SafeArrayGetLBound,
            // 导入上界读取函数。
            SafeArrayGetUBound,
            // 导入元素类型读取函数。
            SafeArrayGetVartype,
        },
        // 导入 32 位整数 VARTYPE。
        Variant::VT_I4,
    },
    // 导入只读 UIA element 接口。
    UI::Accessibility::IUIAutomationElement,
};

// 导入 opaque element ID 生成器。
use crate::components::opaque_id::{OpaqueTargetId, OpaqueTargetKind};

// 限制 provider RuntimeId 的元素数量。
const MAXIMUM_RUNTIME_ID_PARTS: usize = 256;

// 持有 UIA 返回且由调用方负责释放的 SAFEARRAY。
struct OwnedSafeArray {
    // 保存非空 SAFEARRAY 指针。
    value: *mut SAFEARRAY,
}

// 确保所有成功接管的 RuntimeId 数组均被释放。
impl Drop for OwnedSafeArray {
    // 释放 SAFEARRAY 所有权。
    fn drop(&mut self) {
        // Drop 中无法上抛释放错误，但仍必须执行释放调用。
        let _ = unsafe { SafeArrayDestroy(self.value) };
    }
}

// 构造不公开 provider RuntimeId 的 canonical element ID。
fn opaque_runtime_element_id(session_id: &str, runtime_id: &[i32]) -> String {
    // 把有界整数序列编码为无歧义私有哈希输入。
    let runtime_key = runtime_id
        // 逐项转换为十进制文本。
        .iter()
        // 保留负号并避免原生字节序差异。
        .map(i32::to_string)
        // 收集有界中间序列。
        .collect::<Vec<_>>()
        // 用分隔符保留元素边界。
        .join(":");
    // 将精确窗口身份与 provider runtime identity 组合。
    let identity = format!("{session_id}:{runtime_key}");
    // 只返回不可逆 canonical opaque ID。
    OpaqueTargetId::new(OpaqueTargetKind::Element, &identity).to_string()
}

// 读取、验证并散列 UIA element 的 RuntimeId。
pub(crate) fn runtime_element_id(
    // 接收 worker 私有 UIA element。
    element: &IUIAutomationElement,
    // 接收精确父窗口 canonical ID。
    session_id: &str,
) -> windows::core::Result<String> {
    // 请求 provider RuntimeId 数组。
    let array = unsafe { element.GetRuntimeId() }?;
    // 空指针无法形成稳定 identity。
    if array.is_null() {
        // 返回稳定通用失败且不回显 provider 数据。
        return Err(windows::core::Error::from_hresult(E_FAIL));
    }
    // 立即接管 SAFEARRAY 所有权，覆盖后续全部失败路径。
    let _owned = OwnedSafeArray { value: array };
    // RuntimeId 必须是一维整数数组。
    if unsafe { SafeArrayGetDim(array) } != 1 {
        // 拒绝无法无歧义编码的 provider 数据。
        return Err(windows::core::Error::from_hresult(E_FAIL));
    }
    // 核对 SAFEARRAY 元素类型。
    if unsafe { SafeArrayGetVartype(array) }? != VT_I4 {
        // 拒绝非 32 位整数 RuntimeId。
        return Err(windows::core::Error::from_hresult(E_FAIL));
    }
    // 读取唯一维度的下界。
    let lower = unsafe { SafeArrayGetLBound(array, 1) }?;
    // 读取唯一维度的上界。
    let upper = unsafe { SafeArrayGetUBound(array, 1) }?;
    // 空数组无法作为稳定 identity。
    if upper < lower {
        // 拒绝空 RuntimeId。
        return Err(windows::core::Error::from_hresult(E_FAIL));
    }
    // 计算包含两端的元素数量。
    let count = i64::from(upper)
        // 减去下界。
        .checked_sub(i64::from(lower))
        // 包含上界元素。
        .and_then(|span| span.checked_add(1))
        // 转换为内存索引类型。
        .and_then(|value| usize::try_from(value).ok())
        // 溢出统一视为无效 provider identity。
        .ok_or_else(|| windows::core::Error::from_hresult(E_FAIL))?;
    // 限制恶意或损坏 provider 的数组大小。
    if count > MAXIMUM_RUNTIME_ID_PARTS {
        // 拒绝超出内部 identity 预算的数组。
        return Err(windows::core::Error::from_hresult(E_FAIL));
    }
    // 为已验证长度预分配结果。
    let mut parts = Vec::with_capacity(count);
    // 按 SAFEARRAY 原始索引顺序读取每个整数。
    for index in lower..=upper {
        // 准备单个 32 位元素缓冲区。
        let mut part = 0_i32;
        // 从 SAFEARRAY 复制当前元素。
        unsafe {
            // 传入索引指针与类型匹配的输出缓冲区。
            SafeArrayGetElement(array, &index, (&raw mut part).cast())
        }?;
        // 保存已验证 RuntimeId 部分。
        parts.push(part);
    }
    // 返回只包含 opaque 哈希结果的 element ID。
    Ok(opaque_runtime_element_id(session_id, &parts))
}

// 验证 RuntimeId 私有编码的稳定性与边界。
#[cfg(test)]
mod tests {
    // 导入纯 ID 生成 helper。
    use super::opaque_runtime_element_id;

    // 相同窗口和 RuntimeId 必须产生相同 opaque ID。
    #[test]
    fn runtime_element_id_is_stable_and_provider_private() {
        // 构造第一份相同 identity。
        let first = opaque_runtime_element_id("s2:w:0123456789abcdef", &[42, -7, 9]);
        // 构造第二份相同 identity。
        let second = opaque_runtime_element_id("s2:w:0123456789abcdef", &[42, -7, 9]);
        // 相同私有事实必须稳定。
        assert_eq!(first, second);
        // 输出必须保持 canonical element 类别。
        assert!(first.starts_with("s2:e:"));
        // 输出必须只有固定前缀与 16 位摘要。
        assert_eq!(first.len(), 21);
        // RuntimeId 任一部分变化必须改变 identity。
        assert_ne!(
            first,
            opaque_runtime_element_id("s2:w:0123456789abcdef", &[42, -7, 10])
        );
        // 父窗口变化必须改变 identity。
        assert_ne!(
            first,
            opaque_runtime_element_id("s2:w:fedcba9876543210", &[42, -7, 9])
        );
    }
}
