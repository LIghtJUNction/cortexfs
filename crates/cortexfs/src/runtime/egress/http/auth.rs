use super::{ProviderTarget, Request};
use crate::provider::auth::CredentialKind;

pub(super) fn bearer_matches(value: &str, token: &str) -> bool {
    let Some((scheme, credential)) = value.split_once(' ') else {
        return false;
    };
    let credential = credential.trim_start_matches(' ');
    !credential.is_empty() && scheme.eq_ignore_ascii_case("bearer") && credential == token
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
        let name = header.0.as_str();
        let value = header.1.as_str();
        if name == "authorization" {
            return bearer_matches(value, &credential.token) || bearer_matches(value, client_token);
        }
        name == "x-api-key" && value == credential.token
    }) {
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "invalid provider egress credential",
    ))
}

pub(super) fn inject_provider_credential(mut request: Request, target: &ProviderTarget) -> Request {
    let Some(credential) = target.credential.as_ref() else {
        return request;
    };
    request.headers.retain(|header| {
        let name = header.0.as_str();
        !matches!(name, "authorization" | "x-api-key" | "anthropic-version")
            && !matches!(
                name,
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
        request.headers.extend([
            ("chatgpt-account-id".to_owned(), account_id.to_owned()),
            ("originator".to_owned(), "ctx".to_owned()),
            ("session-id".to_owned(), credential.run.clone()),
            (
                "user-agent".to_owned(),
                concat!("cortexfs/", env!("CARGO_PKG_VERSION")).to_owned(),
            ),
        ]);
    }
    request
}

#[cfg(test)]
mod tests {
    use super::bearer_matches;

    #[test]
    fn bearer_matching_follows_http_scheme_rules() {
        assert!(bearer_matches("Bearer secret", "secret"));
        assert!(bearer_matches("bEaReR secret", "secret"));
        assert!(bearer_matches("BEARER   secret", "secret"));
        assert!(!bearer_matches("Basic secret", "secret"));
        assert!(!bearer_matches("Bearer wrong", "secret"));
        assert!(!bearer_matches("Bearer ", "secret"));
        assert!(!bearer_matches("Bearer\tsecret", "secret"));
    }
}
