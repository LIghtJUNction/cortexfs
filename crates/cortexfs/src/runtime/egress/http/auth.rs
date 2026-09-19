use super::{ProviderTarget, Request};
use crate::provider::auth::CredentialKind;
use std::io::ErrorKind;
pub(super) fn is_bearer(value: &str, token: &str) -> bool {
    value.split_once(' ').is_some_and(|(scheme, value)| {
        scheme.eq_ignore_ascii_case("bearer") && value.trim_start_matches(' ') == token
    })
}
pub(super) fn authorize_provider_credential(
    request: &Request,
    target: &ProviderTarget,
    client_token: &str,
) -> std::io::Result<()> {
    let Some(credential) = target.credential.as_ref() else {
        return Ok(());
    };
    if request.headers.iter().any(|header| {
        (header.0 == "authorization"
            && (is_bearer(&header.1, &credential.token) || is_bearer(&header.1, client_token)))
            || (header.0 == "x-api-key" && header.1 == credential.token)
    }) {
        return Ok(());
    }
    Err(std::io::Error::new(ErrorKind::PermissionDenied, "invalid provider egress credential"))
}
pub(super) fn inject_provider_credential(mut request: Request, target: &ProviderTarget) -> Request {
    let Some(credential) = target.credential.as_ref() else {
        return request;
    };
    request.headers.retain(|header| {
        !matches!(
            header.0.as_str(),
            "authorization" | "x-api-key" | "anthropic-version"
        ) && !matches!(
            header.0.as_str(),
            "chatgpt-account-id" | "originator" | "session-id" | "user-agent"
        )
    });
    if request.endpoint == "messages" && credential.kind == CredentialKind::ApiKey {
        request.headers.extend([
            ("x-api-key".to_owned(), credential.token.clone()),
            ("anthropic-version".to_owned(), "2023-06-01".to_owned()),
        ]);
        return request;
    }
    request.headers.push((
        "authorization".to_owned(),
        format!("Bearer {}", credential.token),
    ));
    if request.endpoint == "messages" {
        request
            .headers
            .push(("anthropic-version".to_owned(), "2023-06-01".to_owned()));
    }
    if let Some(account_id) = credential.codex_account_id.as_deref() {
        let user_agent = concat!("cortexfs/", env!("CARGO_PKG_VERSION")).to_owned();
        request.headers.extend([
            ("chatgpt-account-id".to_owned(), account_id.to_owned()),
            ("originator".to_owned(), "ctx".to_owned()),
            ("session-id".to_owned(), credential.run.clone()),
            ("user-agent".to_owned(), user_agent),
        ]);
    }
    request
}
#[cfg(test)]
mod tests {
    use super::is_bearer;
    #[test]
    fn bearer_matching_follows_http_rules() {
        assert!(is_bearer("bEaReR   secret", "secret"));
        assert!(!is_bearer("Basic secret", "secret") && !is_bearer("Bearer wrong", "secret"));
    }
}
