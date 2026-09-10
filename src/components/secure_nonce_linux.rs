//! 使用 Linux 内核随机源生成不携带身份事实的一次性 nonce。

use std::{fmt::Write as _, fs::File, io::Read};

use crate::domain::{AppControlError, AppResult};

const NONCE_BYTES: usize = 16;

/// 从固定内核随机设备生成 canonical 128 位小写十六进制 nonce。
pub(crate) fn random_nonce() -> AppResult<String> {
    let mut bytes = [0_u8; NONCE_BYTES];
    File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .map_err(|_| authentication_error("generated"))?;
    let mut encoded = String::with_capacity(NONCE_BYTES * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").map_err(|_| authentication_error("encoded"))?;
    }
    Ok(encoded)
}

fn authentication_error(action: &'static str) -> AppControlError {
    AppControlError::new(
        "ENDPOINT_AUTHENTICATION_FAILED",
        format!("A cryptographically random endpoint nonce could not be {action}."),
    )
}

#[cfg(test)]
mod tests {
    use super::random_nonce;

    #[test]
    fn random_nonces_are_canonical_and_distinct() {
        let first = random_nonce().unwrap_or_else(|error| panic!("first nonce failed: {error}"));
        let second = random_nonce().unwrap_or_else(|error| panic!("second nonce failed: {error}"));
        for value in [&first, &second] {
            assert_eq!(value.len(), 32);
            assert!(
                value
                    .bytes()
                    .all(|byte| { byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) })
            );
        }
        assert_ne!(first, second);
    }
}
