use std::io::{Error, ErrorKind, Result};

use super::{ProviderTarget, Request};
use crate::provider::auth::CredentialKind;

pub(super) fn bearer_matches(value: &str, token: &str) -> bool {
    value.split_once(' ').is_some_and(|(scheme, value)| {
        let value = value.trim_start_matches(' ');
        !value.is_empty() && scheme.eq_ignore_ascii_case("bearer") && value == token
    })
}

pub(super) fn authorize_provider_credential(
    request: &Request,
    target: &ProviderTarget,
    client_token: &str,
) -> Result<()> {
    let Some(credential) = target.credential.as_ref() else {
        return Ok(());
    };
    request
        .headers
        .iter()
        .any(|(name, value)| match name.as_str() {
            "authorization" => {
                bearer_matches(value, &credential.token) || bearer_matches(value, client_token)
            }
            "x-api-key" => value == &credential.token,
            _ => false,
        })
        .then_some(())
        .ok_or_else(|| Error::new(ErrorKind::PermissionDenied, "invalid provider egress credential"))
}

pub(super) fn inject_provider_credential(mut request: Request, target: &ProviderTarget) -> Request {
    let Some(credential) = target.credential.as_ref() else {
        return request;
    };
    request.headers.retain(|(name, _)| {
        !matches!(
            name.as_str(),
            "authorization"
                | "x-api-key"
                | "anthropic-version"
                | "chatgpt-account-id"
                | "originator"
                | "session-id"
                | "user-agent"
        )
    });
    if request.endpoint == "messages" && credential.kind == CredentialKind::ApiKey {
        request.headers.extend([
            ("x-api-key".to_owned(), credential.token.clone()),
            ("anthropic-version".to_owned(), "2023-06-01".to_owned()),
        ]);
        return request;
    }
    request.headers.push(("authorization".to_owned(), format!("Bearer {}", credential.token)));
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
