//! 跨平台桌面会话的 provider-neutral opaque 身份。

use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    components::opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    domain::AppResult,
};

#[cfg(target_os = "linux")]
use std::{fmt::Write as _, fs::File, io::Read};

#[cfg(target_os = "linux")]
const RANDOM_BYTES: usize = 16;
static NEXT_SESSION_GENERATION: AtomicU64 = AtomicU64::new(1);

/// 为当前 Module 代际生成不携带 Portal 路径或本机身份的 `s2:i`。
pub(crate) fn new_session_target() -> AppResult<String> {
    let random = random_nonce()?;
    let generation = NEXT_SESSION_GENERATION.fetch_add(1, Ordering::Relaxed);
    let mut private_identity = format!("desktop-session:{generation:016x}:");
    private_identity.push_str(&random);
    Ok(OpaqueTargetId::new(OpaqueTargetKind::InteractiveSession, &private_identity).to_string())
}

/// 为 broker epoch 生成不携带本机事实的 128 位 canonical nonce。
#[cfg(target_os = "linux")]
pub(crate) fn random_nonce() -> AppResult<String> {
    let mut random = [0_u8; RANDOM_BYTES];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|_| identity_error())?;
    let mut encoded = String::with_capacity(RANDOM_BYTES.saturating_mul(2));
    for byte in random {
        write!(&mut encoded, "{byte:02x}").map_err(|_| identity_error())?;
    }
    Ok(encoded)
}

#[cfg(target_os = "linux")]
fn identity_error() -> crate::domain::AppControlError {
    crate::domain::AppControlError::new(
        "SESSION_IDENTITY_FAILED",
        "A new desktop session identity could not be generated.",
    )
}

#[cfg(test)]
mod tests {
    use super::{new_session_target, random_nonce};
    use crate::components::opaque_id::{OpaqueTargetId, OpaqueTargetKind};

    #[test]
    fn generated_targets_are_canonical_interactive_sessions_and_distinct() {
        let first = new_session_target()
            .unwrap_or_else(|error| panic!("first desktop session identity failed: {error}"));
        let second = new_session_target()
            .unwrap_or_else(|error| panic!("second desktop session identity failed: {error}"));
        assert_ne!(first, second);
        assert_eq!(
            OpaqueTargetId::parse(&first).map(OpaqueTargetId::kind),
            Some(OpaqueTargetKind::InteractiveSession)
        );
        assert_eq!(first.len(), 21);
    }

    #[test]
    fn broker_nonce_is_fixed_lowercase_hex() {
        let nonce =
            random_nonce().unwrap_or_else(|error| panic!("desktop broker nonce failed: {error}"));
        assert_eq!(nonce.len(), 32);
        assert!(
            nonce
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn random_nonce() -> AppResult<String> {
    crate::components::secure_nonce_windows::random_nonce()
}
