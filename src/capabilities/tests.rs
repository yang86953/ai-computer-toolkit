// 使用有序集合生成稳定动作顺序。
use std::collections::BTreeSet;

// 导入本模块全部注册表符号。
use super::*;

// 验证注册表 ID 唯一且都能反向解析。
#[test]
fn capability_ids_are_unique_and_resolvable() {
    // 创建空集合保存已见 ID。
    let mut ids = BTreeSet::new();
    // 逐项核对注册表。
    for capability in ALL {
        // 重复 ID 必须立即使门禁失败。
        assert!(
            ids.insert(capability.id),
            "duplicate capability: {}",
            capability.id
        );
        // ID 必须解析回同一静态定义。
        assert_eq!(definition(capability.id), Some(capability));
    }
}

// 验证 surface action verb 只由实际 registry 定义投影且保持唯一顺序。
#[test]
fn app_action_verbs_are_registry_backed_unique_and_stable() {
    // 从生产单一注册表取得 App surface 的 generic verb。
    let verbs = action_verbs_for_surface(CapabilitySurface::App);
    // 锁定现有公开 generic verb 的兼容顺序和值。
    assert_eq!(
        // 使用实际 registry 投影。
        verbs,
        // 不包含 facade 自有 sessions 和 inspect。
        vec![
            // MPRIS v3 的发现从同一生产注册表投影。
            "discover",
            // 原有 verb 的相对顺序保持不变。
            "read",
            "create",
            "apply",
            "save",
            "export",
            "close",
            "screenshot",
            "record",
        ]
    );
    // 再次投影供覆盖和唯一性检查使用。
    let verbs = action_verbs_for_surface(CapabilitySurface::App);
    // 收集为集合以检测重复动作。
    let unique = verbs.iter().copied().collect::<BTreeSet<_>>();
    // 每个动作只能在公开列表出现一次。
    assert_eq!(unique.len(), verbs.len());
    // 每个 App capability 的动作都必须进入投影。
    for definition in ALL
        // 遍历单一 registry。
        .iter()
        // 只核对 App surface 所有权。
        .filter(|definition| definition.surface == CapabilitySurface::App)
    {
        // 新增 App capability action 若未进入规范顺序会使门禁失败。
        assert!(verbs.contains(&definition.action.as_str()));
    }
    // discover 必须来自实际 App 所有者，不从其他 surface 隐式借入。
    assert!(ALL.iter().any(|definition| definition.id == "media.session.discover@3"
        && definition.surface == CapabilitySurface::App
        && definition.action.as_str() == "discover"));
}
