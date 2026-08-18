use super::{ProviderTarget, Request};
pub(super) fn authorize_provider_credential(
    request: &Request,
    target: &ProviderTarget,
    client_token: &str,
) -> std::io::Result<()> {
    let Some(credential) = target.credential.as_ref() else {
        return Ok(());
    };
    let bearer = format!("Bearer {}", credential.token);
    let client_bearer = format!("Bearer {client_token}");
    if request.headers.iter().any(|header| {
        let name = header.0.as_str();
        let value = header.1.as_str();
        (name == "authorization" && (value == bearer || value == client_bearer))
            || (name == "x-api-key" && value == credential.token)
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
    if request.endpoint == "messages" {
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
    if target.provider == "codex"
        && let Some(account_id) = credential.codex_account_id.as_deref()
    {
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
