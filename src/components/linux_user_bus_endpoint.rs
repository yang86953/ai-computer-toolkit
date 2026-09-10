//! Linux 当前有效用户的标准 D-Bus endpoint 身份 Component。

use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
};

/// 只向 Adapter 交付已认证地址和不透明父代际材料，不公开文件系统元数据。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LinuxUserBusEndpoint {
    address: String,
    runtime_generation: String,
}

impl LinuxUserBusEndpoint {
    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    pub(crate) fn runtime_generation(&self) -> &str {
        &self.runtime_generation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LinuxUserBusEndpointFailure {
    IdentityUnavailable,
    UnsafeRuntime,
    BusUnavailable,
}

/// 不读取环境变量，只用内核有效 UID 解析固定的 XDG user bus 路径。
pub(crate) fn resolve_current() -> Result<LinuxUserBusEndpoint, LinuxUserBusEndpointFailure> {
    let effective_uid = rustix::process::geteuid().as_raw();
    let runtime = PathBuf::from(format!("/run/user/{effective_uid}"));
    resolve_at(&runtime, effective_uid)
}

fn resolve_at(
    runtime: &Path,
    effective_uid: u32,
) -> Result<LinuxUserBusEndpoint, LinuxUserBusEndpointFailure> {
    if !runtime.is_absolute() {
        return Err(LinuxUserBusEndpointFailure::IdentityUnavailable);
    }
    let runtime_metadata = fs::symlink_metadata(runtime)
        .map_err(|_| LinuxUserBusEndpointFailure::IdentityUnavailable)?;
    if !runtime_metadata.file_type().is_dir()
        || runtime_metadata.file_type().is_symlink()
        || runtime_metadata.uid() != effective_uid
        || runtime_metadata.mode() & 0o777 != 0o700
    {
        return Err(LinuxUserBusEndpointFailure::UnsafeRuntime);
    }

    let bus = runtime.join("bus");
    let bus_metadata =
        fs::symlink_metadata(&bus).map_err(|_| LinuxUserBusEndpointFailure::BusUnavailable)?;
    if !bus_metadata.file_type().is_socket()
        || bus_metadata.file_type().is_symlink()
        || bus_metadata.uid() != effective_uid
    {
        return Err(LinuxUserBusEndpointFailure::UnsafeRuntime);
    }

    Ok(LinuxUserBusEndpoint {
        address: format!("unix:path={}", bus.display()),
        runtime_generation: format!(
            "u{effective_uid}-d{:x}-i{:x}-bd{:x}-bi{:x}",
            runtime_metadata.dev(),
            runtime_metadata.ino(),
            bus_metadata.dev(),
            bus_metadata.ino(),
        ),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        os::unix::{fs::PermissionsExt, net::UnixListener},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    struct RuntimeFixture {
        root: PathBuf,
        _listener: Option<UnixListener>,
    }

    impl RuntimeFixture {
        fn new() -> Result<Self, Box<dyn std::error::Error>> {
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let root = std::env::temp_dir().join(format!(
                "ai-computer-toolkit-user-bus-{}-{stamp}",
                std::process::id()
            ));
            fs::create_dir(&root)?;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
            let listener = UnixListener::bind(root.join("bus"))?;
            Ok(Self {
                root,
                _listener: Some(listener),
            })
        }

        fn uid(&self) -> Result<u32, Box<dyn std::error::Error>> {
            Ok(fs::symlink_metadata(&self.root)?.uid())
        }
    }

    impl Drop for RuntimeFixture {
        fn drop(&mut self) {
            self._listener.take();
            let _ = fs::remove_file(self.root.join("bus"));
            let _ = fs::remove_dir(&self.root);
        }
    }

    #[test]
    fn accepts_owner_only_runtime_and_owned_socket() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = RuntimeFixture::new()?;
        let endpoint = match resolve_at(&fixture.root, fixture.uid()?) {
            Ok(endpoint) => endpoint,
            Err(failure) => return Err(format!("安全 endpoint 解析失败: {failure:?}").into()),
        };
        assert_eq!(
            endpoint.address(),
            format!("unix:path={}", fixture.root.join("bus").display())
        );
        assert!(endpoint.runtime_generation().starts_with('u'));
        Ok(())
    }

    #[test]
    fn rejects_group_accessible_runtime_directory() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = RuntimeFixture::new()?;
        fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o750))?;
        assert_eq!(
            resolve_at(&fixture.root, fixture.uid()?),
            Err(LinuxUserBusEndpointFailure::UnsafeRuntime)
        );
        Ok(())
    }

    #[test]
    fn rejects_missing_or_non_socket_bus() -> Result<(), Box<dyn std::error::Error>> {
        let mut fixture = RuntimeFixture::new()?;
        fixture._listener.take();
        fs::remove_file(fixture.root.join("bus"))?;
        assert_eq!(
            resolve_at(&fixture.root, fixture.uid()?),
            Err(LinuxUserBusEndpointFailure::BusUnavailable)
        );
        fs::write(fixture.root.join("bus"), b"not-a-socket")?;
        assert_eq!(
            resolve_at(&fixture.root, fixture.uid()?),
            Err(LinuxUserBusEndpointFailure::UnsafeRuntime)
        );
        Ok(())
    }

    #[test]
    fn rejects_uid_mismatch_without_exposing_owner() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = RuntimeFixture::new()?;
        let uid = fixture.uid()?;
        assert_eq!(
            resolve_at(&fixture.root, uid.wrapping_add(1)),
            Err(LinuxUserBusEndpointFailure::UnsafeRuntime)
        );
        Ok(())
    }

    #[test]
    fn rejects_bus_symlink_masquerading_as_socket() -> Result<(), Box<dyn std::error::Error>> {
        let mut fixture = RuntimeFixture::new()?;
        let uid = fixture.uid()?;
        fixture._listener.take();
        let bus = fixture.root.join("bus");
        fs::remove_file(&bus)?;

        let target = fixture.root.join("target-bus");
        let _target_listener = UnixListener::bind(&target)?;
        std::os::unix::fs::symlink(&target, &bus)?;
        assert_eq!(
            resolve_at(&fixture.root, uid),
            Err(LinuxUserBusEndpointFailure::UnsafeRuntime)
        );

        fs::remove_file(target)?;
        Ok(())
    }

    #[test]
    fn re_reads_rebuilt_socket_metadata_without_changing_endpoint_address()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut fixture = RuntimeFixture::new()?;
        let uid = fixture.uid()?;
        let first = match resolve_at(&fixture.root, uid) {
            Ok(endpoint) => endpoint,
            Err(failure) => return Err(format!("初始 endpoint 解析失败: {failure:?}").into()),
        };

        fixture._listener.take();
        let bus = fixture.root.join("bus");
        fs::remove_file(&bus)?;
        fixture._listener = Some(UnixListener::bind(&bus)?);

        let rebuilt = match resolve_at(&fixture.root, uid) {
            Ok(endpoint) => endpoint,
            Err(failure) => return Err(format!("重建 socket 后解析失败: {failure:?}").into()),
        };
        assert_eq!(rebuilt.address(), first.address());

        // inode 可能被内核复用；再次解析必须读取当前 socket 元数据，而不是缓存旧代际。
        let reread = match resolve_at(&fixture.root, uid) {
            Ok(endpoint) => endpoint,
            Err(failure) => return Err(format!("重新读取 socket 元数据失败: {failure:?}").into()),
        };
        assert_eq!(reread.runtime_generation(), rebuilt.runtime_generation());

        Ok(())
    }
}
