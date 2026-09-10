#![cfg(target_os = "linux")]

//! Linux XDG walker 的跨 root 全局 dirent 预算回归。

use std::{
    fs,
    os::unix::{fs::symlink, net::UnixListener},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

const LIMIT: usize = 32_768;

fn fixture_directory() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!(
        "ai-computer-toolkit-dirent-limit-{}-{nonce}",
        std::process::id()
    ))
}

fn run_discovery(user: &Path, system: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ai-computer-toolkit"))
        .args([
            "discover",
            "app",
            "--max-applications",
            "1",
            "--max-processes",
            "1",
            "--max-windows",
            "1",
        ])
        .env_clear()
        .env("XDG_DATA_HOME", user)
        .env("XDG_DATA_DIRS", system)
        .env("PATH", "/usr/bin")
        .env("LANG", "C")
        .output()
        .unwrap_or_else(|error| panic!("启动 dirent fixture CLI 失败：{error}"));
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("解析 dirent fixture JSON 失败：{error}"))
}

#[test]
fn global_dirent_budget_counts_all_types_before_collection() {
    let directory = fixture_directory();
    let user = directory.join("user");
    let system = directory.join("system");
    let user_apps = user.join("applications");
    let system_apps = system.join("applications");
    fs::create_dir_all(user_apps.join("directory-noise"))
        .unwrap_or_else(|error| panic!("建立目录噪声失败：{error}"));
    fs::create_dir_all(&system_apps)
        .unwrap_or_else(|error| panic!("建立系统 fixture 失败：{error}"));
    fs::write(user_apps.join("symlink-source"), b"")
        .unwrap_or_else(|error| panic!("写入 symlink 源失败：{error}"));
    symlink("symlink-source", user_apps.join("symlink-noise"))
        .unwrap_or_else(|error| panic!("建立 symlink 噪声失败：{error}"));
    let fifo_status = Command::new("/usr/bin/mkfifo")
        .arg(user_apps.join("fifo-noise"))
        .status()
        .unwrap_or_else(|error| panic!("建立 FIFO 噪声失败：{error}"));
    assert!(fifo_status.success());
    let _socket = UnixListener::bind(user_apps.join("socket-noise"))
        .unwrap_or_else(|error| panic!("建立 socket 噪声失败：{error}"));

    // user 根已有五项；其余普通未知扩展与 system 根共同填满全局预算。
    for index in 5..(LIMIT / 2) {
        fs::write(user_apps.join(format!("noise-{index:05}")), b"")
            .unwrap_or_else(|error| panic!("写入用户噪声失败：{error}"));
    }
    for index in 0..(LIMIT / 2) {
        fs::write(system_apps.join(format!("noise-{index:05}")), b"")
            .unwrap_or_else(|error| panic!("写入系统噪声失败：{error}"));
    }
    let exact = run_discovery(&user, &system);
    assert_eq!(exact["data"]["complete"]["applications"], true);
    assert_eq!(exact["data"]["warnings"], serde_json::json!([]));

    fs::write(system_apps.join("overflow-noise"), b"")
        .unwrap_or_else(|error| panic!("写入预算溢出噪声失败：{error}"));
    let overflow = run_discovery(&user, &system);
    assert_eq!(overflow["data"]["complete"]["applications"], false);
    assert!(
        overflow["data"]["warnings"]
            .as_array()
            .is_some_and(|warnings| warnings
                .contains(&serde_json::json!("desktop-entry-scan-limit-reached")))
    );

    fs::remove_dir_all(directory)
        .unwrap_or_else(|error| panic!("清理 dirent fixture 失败：{error}"));
}
