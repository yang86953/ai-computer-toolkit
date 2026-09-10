//! 生成 UIX 语义快照与节点的稳定不透明公开身份。

use super::opaque_id::{OpaqueTargetId, OpaqueTargetKind};

/// 从精确窗口代际与语义修订生成 snapshot-scoped 公开身份。
pub(crate) fn snapshot_id(session_id: &str, revision: u64, presented_revision: u64) -> String {
    let opaque = OpaqueTargetId::new(
        OpaqueTargetKind::Element,
        &format!("uix-agent-v1-snapshot\0{session_id}\0{revision}\0{presented_revision}"),
    )
    .to_string();
    format!("as3:{}", &opaque[5..])
}

/// 从 snapshot-scoped 身份与 provider 私有节点身份生成不透明元素身份。
pub(crate) fn element_id(snapshot_id: &str, native_id: &str) -> String {
    OpaqueTargetId::new(
        OpaqueTargetKind::Element,
        &format!("uix-agent-v1-node\0{snapshot_id}\0{native_id}"),
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_are_canonical_and_snapshot_scoped() {
        let first = snapshot_id("s2:w:0123456789abcdef", 7, 6);
        let second = snapshot_id("s2:w:0123456789abcdef", 8, 6);
        assert!(first.starts_with("as3:"));
        assert_eq!(first.len(), 20);
        assert_ne!(first, second);

        let first_element = element_id(&first, "7:1");
        let second_element = element_id(&second, "7:1");
        assert!(first_element.starts_with("s2:e:"));
        assert_ne!(first_element, second_element);
    }
}
