#![cfg(target_os = "linux")]

//! Cargo binary 角色门禁，防止默认 Linux build 重新产生 worker stub 或 fixture。

use std::collections::BTreeSet;

#[test]
fn cargo_declares_one_default_cli_and_gates_every_other_binary() {
    let manifest = include_str!("../../Cargo.toml");
    assert!(manifest.contains("autobins = false"));
    let blocks = manifest.split("[[bin]]").skip(1).collect::<Vec<_>>();
    assert_eq!(blocks.len(), 29);

    let mut default = Vec::new();
    let mut windows = Vec::new();
    let mut fixtures = Vec::new();
    let mut release = Vec::new();
    let mut linux_atspi_candidates = Vec::new();
    let mut linux_mpris_candidates = Vec::new();
    let mut paths = BTreeSet::new();
    for block in blocks {
        let block = block.split("\n\n").next().unwrap();
        let name = value(block, "name");
        let path = value(block, "path");
        assert!(paths.insert(path.to_owned()), "duplicate bin path {path}");
        if block.contains("required-features = [\"windows-workers\"]") {
            windows.push(name);
        } else if block.contains("required-features = [\"test-fixtures\"]") {
            fixtures.push(name);
        } else if block.contains("required-features = [\"linux-release-tools\"]") {
            release.push(name);
        } else if block.contains("required-features = [\"linux-atspi-candidate\"]") {
            linux_atspi_candidates.push(name);
        } else if block.contains("required-features = [\"linux-mpris-candidate\"]") {
            linux_mpris_candidates.push(name);
        } else {
            default.push(name);
        }
    }
    assert_eq!(default, ["ai-computer-toolkit"]);
    assert_eq!(release, ["ai-computer-toolkit-linux-release"]);
    assert_eq!(
        linux_atspi_candidates,
        ["ai-computer-toolkit-atspi-observation-worker"]
    );
    assert!(linux_mpris_candidates.is_empty());
    let main = include_str!("../../src/main.rs");
    assert!(main.contains("linux_media_observation_worker::HIDDEN_ARGUMENT"));
    assert!(main.contains("linux_media_control_worker::HIDDEN_ARGUMENT"));
    assert!(
        include_str!("../../src/linux_media_observation_worker.rs")
            .contains("__linux-mpris-observation-worker-v1")
    );
    assert!(
        include_str!("../../src/linux_media_control_worker.rs")
            .contains("__linux-mpris-control-worker-v1")
    );
    assert_eq!(windows.len(), 11);
    assert_eq!(fixtures.len(), 15);
    assert!(windows.iter().all(|name| !name.contains("fixture")));
    assert!(fixtures.iter().all(|name| name.contains("fixture")));
    assert!(include_str!("../../src/lib.rs").contains(
        "compile_error!(\"windows-workers feature is only supported on Windows targets\")"
    ));
    assert!(
        include_str!("../../src/linux_release.rs").lines().count() <= 1_500,
        "Linux release source exceeded the project line limit"
    );
}

fn value<'a>(block: &'a str, key: &str) -> &'a str {
    block
        .lines()
        .find_map(|line| {
            line.strip_prefix(&format!("{key} = \""))
                .and_then(|value| value.strip_suffix('"'))
        })
        .unwrap_or_else(|| panic!("missing {key} in bin block"))
}
