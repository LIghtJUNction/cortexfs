use std::{
    fmt::Write as _,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

use super::WebhookConfig;
use crate::config::Platform;
use cortexfs::channel::http::HttpRequest;

#[derive(Debug, thiserror::Error)]
pub(super) enum SignatureError {
    #[error("webhook signing secret is missing")]
    MissingSecret,
    #[error("webhook nonce generation failed")]
    Random(String),
}

pub(super) fn nonce() -> Result<String, SignatureError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| SignatureError::Random(error.to_string()))?;
    Ok(hex(&bytes))
}

pub(super) fn hmac_hex(secret: &str, message: &str) -> String {
    hex(&hmac_bytes(secret, message))
}

pub(super) fn hmac_base64(secret: &str, message: &str) -> String {
    STANDARD.encode(hmac_bytes(secret, message))
}

fn hmac_bytes(secret: &str, message: &str) -> [u8; 32] {
    let mut key = [0_u8; 64];
    if secret.len() > key.len() {
        let digest = Sha256::digest(secret.as_bytes());
        for (slot, byte) in key.iter_mut().zip(digest) {
            *slot = byte;
        }
    } else {
        for (slot, byte) in key.iter_mut().zip(secret.bytes()) {
            *slot = byte;
        }
    }
    let mut inner = Sha256::new();
    let mut outer = Sha256::new();
    for byte in key {
        inner.update([byte ^ 0x36]);
        outer.update([byte ^ 0x5c]);
    }
    inner.update(message.as_bytes());
    outer.update(inner.finalize());
    outer.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ignored = write!(output, "{byte:02x}");
    }
    output
}

pub(super) fn verify(config: &WebhookConfig, request: &HttpRequest) -> bool {
    if !matches!(
        config.platform,
        Platform::Line | Platform::Nextcloud | Platform::Linq
    ) {
        return true;
    }
    let Some(secret) = config.verify_token.as_deref() else {
        return true;
    };
    match config.platform {
        Platform::Line => request
            .headers
            .get("x-line-signature")
            .is_some_and(|signature| {
                constant_time_equal(signature, &hmac_base64(secret, &request.body))
            }),
        Platform::Nextcloud => {
            let Some(random) = request.headers.get("x-nextcloud-talk-random") else {
                return false;
            };
            let Some(signature) = request.headers.get("x-nextcloud-talk-signature") else {
                return false;
            };
            constant_time_equal(
                signature,
                &hmac_hex(secret, &format!("{random}{}", request.body)),
            )
        }
        Platform::Linq => {
            let Some(timestamp) = request.headers.get("x-linq-timestamp") else {
                return false;
            };
            let Ok(timestamp_value) = timestamp.parse::<i64>() else {
                return false;
            };
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_secs().cast_signed());
            if (now - timestamp_value).unsigned_abs() > 300 {
                return false;
            }
            let Some(signature) = request.headers.get("x-linq-signature") else {
                return false;
            };
            let signature = signature.strip_prefix("sha256=").unwrap_or(signature);
            constant_time_equal(
                signature,
                &hmac_hex(secret, &format!("{timestamp}.{}", request.body)),
            )
        }
        _ => true,
    }
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{WebhookConfig, hmac_base64, hmac_hex, verify};
    use crate::config::Platform;
    use cortexfs::channel::http::HttpRequest;

    #[test]
    fn hmac_matches_rfc4231_vector() {
        assert_eq!(
            hmac_hex("Jefe", "what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn hmac_base64_is_stable() {
        assert_eq!(
            hmac_base64("Jefe", "what do ya want for nothing?"),
            "W9zBRr9gdU5qBCQmCJV1x1oAPwidJzmDnexYuWTsOEM="
        );
    }

    #[test]
    fn linq_signature_covers_timestamp_and_body() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        let body = "{}";
        let signature = hmac_hex("secret", &format!("{timestamp}.{body}"));
        let request = HttpRequest {
            method: "POST".to_owned(),
            path: "/webhook".to_owned(),
            headers: BTreeMap::from([
                ("x-linq-timestamp".to_owned(), timestamp.to_string()),
                ("x-linq-signature".to_owned(), signature),
            ]),
            body: body.to_owned(),
        };
        let config = WebhookConfig {
            bind: std::net::SocketAddr::from(([127, 0, 0, 1], 8765)),
            path: "/webhook".to_owned(),
            platform: Platform::Linq,
            outbound_url: "https://example.invalid/{path}".to_owned(),
            token: None,
            verify_token: Some("secret".to_owned()),
            channel: None,
        };
        assert!(verify(&config, &request));
    }
}
