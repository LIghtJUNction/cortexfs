use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use cortexfs::{
    AuthRequest, AuthWireRequest, AuthWireResponse, OAuthPkce, configured_registry,
    current_time_unix, http_transport, store_auth_profile,
};

const MAX_CALLBACK_BYTES: u64 = 8 * 1024;

#[expect(
    clippy::redundant_pub_crate,
    reason = "binary root dispatches browser login"
)]
pub(crate) fn login(output: &mut impl Write, request: AuthWireRequest) -> Result<(), ()> {
    let AuthWireRequest::Browser {
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
    let redirect = reqwest::Url::parse(&oauth.redirect_uri).map_err(|_error| ())?;
    let host = redirect
        .host_str()
        .filter(|host| matches!(*host, "localhost" | "127.0.0.1"))
        .ok_or(())?;
    let port = redirect.port().ok_or(())?;
    let path = redirect.path().to_owned();
    let registry = configured_registry(&provider, &base_url, methods, Some(*oauth)).ok_or(())?;
    let adapter = registry.get(&provider).ok_or(())?;
    let mut entropy = [0_u8; 32];
    getrandom::fill(&mut entropy).map_err(|_error| ())?;
    let pkce = OAuthPkce::from_entropy(&entropy).map_err(|_error| ())?;
    let state = hex(&entropy[..16]);
    let url = adapter
        .authorization_url(&state, &pkce)
        .map_err(|_error| ())?;
    super::write_frame(
        output,
        AuthWireResponse::Progress {
            request_id: request_id.clone(),
            state: "authorizing".to_owned(),
            detail: Some(url),
        },
    )?;
    let listener = TcpListener::bind((host, port)).map_err(|_error| ())?;
    listener.set_nonblocking(true).map_err(|_error| ())?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return Err(()),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|_error| ())?;
    let mut raw = Vec::new();
    Read::by_ref(&mut stream)
        .take(MAX_CALLBACK_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|_error| ())?;
    if raw.len() > usize::try_from(MAX_CALLBACK_BYTES).map_err(|_error| ())? {
        return Err(());
    }
    let request = String::from_utf8(raw).map_err(|_error| ())?;
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or(())?;
    let callback =
        reqwest::Url::parse(&format!("http://localhost{target}")).map_err(|_error| ())?;
    let callback_state = callback
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then_some(value.into_owned()));
    if callback.path() != path || callback_state.as_deref() != Some(state.as_str()) {
        return Err(());
    }
    let code = callback
        .query_pairs()
        .find_map(|(key, value)| (key == "code").then_some(value.into_owned()))
        .filter(|value| !value.is_empty())
        .ok_or(())?;
    let mut transport = http_transport().map_err(|_error| ())?;
    let credential = adapter
        .login_with(
            AuthRequest::AuthorizationCodePkce {
                code,
                verifier: pkce.verifier().to_owned(),
            },
            &mut transport,
            current_time_unix(),
        )
        .map_err(|_error| ())?;
    let ok = store_auth_profile(&provider, &profile, credential).is_ok();
    super::write_frame(
        output,
        AuthWireResponse::Result {
            request_id,
            ok,
            code: (!ok).then_some("AUTH_STORE_FAILED".to_owned()),
        },
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut value, byte| {
        let _ignored = write!(value, "{byte:02x}");
        value
    })
}
