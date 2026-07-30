use super::{ProviderTarget, Request};

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
