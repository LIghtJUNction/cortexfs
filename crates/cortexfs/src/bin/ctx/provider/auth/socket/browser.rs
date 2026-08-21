use crate::*;

#[expect(
    clippy::redundant_pub_crate,
    reason = "provider OAuth dispatch reaches socketpair client"
)]
pub(crate) fn oauth_browser_login(
    provider: &str,
    profile: &str,
    config: &CtxProviderConfig,
    oauth: cortexfs::OAuthProviderConfig,
    timeout_secs: u64,
) -> Result<(), CliError> {
    let request_id = format!("auth-{}", hex_bytes(&read_system_entropy(16)?));
    let frame = cortexfs::AuthWireFrame::new(cortexfs::AuthWireRequest::Browser {
        request_id: request_id.clone(),
        provider: provider.to_owned(),
        profile: profile.to_owned(),
        base_url: config.base_url.clone(),
        methods: config.auth_methods(),
        oauth: Box::new(oauth),
        timeout_secs,
    });
    super::run_auth_request(&frame, &request_id, |detail| {
        detail.map_or(Ok(()), print_line)
    })
}
