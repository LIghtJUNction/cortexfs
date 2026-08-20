use std::io::Write;

use cortexfs::{
    AuthWireRequest, AuthWireResponse, Credential, configured_registry, current_time_unix,
    http_transport, store_auth_profile,
};

#[expect(
    clippy::redundant_pub_crate,
    reason = "binary root dispatches its private device handler"
)]
pub(super) fn login(output: &mut impl Write, request: AuthWireRequest) -> Result<(), ()> {
    let AuthWireRequest::Device {
        request_id,
        provider,
        profile,
        base_url,
        methods,
        oauth,
        timeout_secs,
    } = request
    else {
        return Err(());
    };
    let registry = configured_registry(&provider, &base_url, methods, Some(*oauth)).ok_or(())?;
    let adapter = registry.get(&provider).ok_or(())?;
    let mut transport = http_transport().map_err(|_error| ())?;
    let mut emit = |challenge: &cortexfs::DeviceChallenge| {
        let _ignored = super::write_frame(
            output,
            AuthWireResponse::Progress {
                request_id: request_id.clone(),
                state: "waiting_user".to_owned(),
                detail: Some(format!(
                    "{}\t{}",
                    challenge.verification_uri, challenge.user_code
                )),
            },
        );
    };
    let mut pause = |seconds| std::thread::sleep(std::time::Duration::from_secs(seconds));
    let credential = adapter
        .device_login_with(
            timeout_secs,
            &mut transport,
            current_time_unix(),
            &mut emit,
            &mut pause,
        )
        .map_err(|_error| ())?;
    let ok = matches!(credential, Credential::OAuth { .. })
        && store_auth_profile(&provider, &profile, credential).is_ok();
    super::write_frame(
        output,
        AuthWireResponse::Result {
            request_id,
            ok,
            code: (!ok).then_some("AUTH_STORE_FAILED".to_owned()),
        },
    )
}
