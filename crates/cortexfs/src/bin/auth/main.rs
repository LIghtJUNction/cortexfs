#![forbid(unsafe_code)]

use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;

use cortexfs::{AuthWireFrame, AuthWireRequest, AuthWireResponse, store_auth_profile};

mod browser;
mod device;

const MAX_FRAME_BYTES: usize = 16 * 1024;

fn main() -> ExitCode {
    match run(io::stdin().lock(), io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::from(69),
    }
}

fn run(mut input: impl BufRead, mut output: impl Write) -> Result<(), ()> {
    let mut line = String::new();
    input
        .by_ref()
        .take(u64::try_from(MAX_FRAME_BYTES + 1).map_err(|_error| ())?)
        .read_line(&mut line)
        .map_err(|_error| ())?;
    if line.is_empty() || line.len() > MAX_FRAME_BYTES {
        return Err(());
    }
    let AuthWireFrame { frame, .. } =
        AuthWireFrame::<AuthWireRequest>::decode(&line).map_err(|_error| ())?;
    match frame {
        AuthWireRequest::ApiKey {
            request_id,
            provider,
            profile,
            key,
        } => api_key_login(&mut output, request_id, &provider, &profile, key),
        request @ AuthWireRequest::Device { .. } => device::login(&mut output, request),
        request @ AuthWireRequest::Browser { .. } => browser::login(&mut output, request),
    }
}

fn api_key_login(
    output: &mut impl Write,
    request_id: String,
    provider: &str,
    profile: &str,
    key: String,
) -> Result<(), ()> {
    write_frame(
        output,
        AuthWireResponse::Progress {
            request_id: request_id.clone(),
            state: "storing".to_owned(),
            detail: None,
        },
    )?;
    let credential = cortexfs::Credential::ApiKey {
        provider: provider.to_owned(),
        key,
        slot: Some(profile.to_owned()),
    };
    let ok = store_auth_profile(provider, profile, credential).is_ok();
    write_frame(
        output,
        AuthWireResponse::Result {
            request_id,
            ok,
            code: (!ok).then_some("AUTH_STORE_FAILED".to_owned()),
        },
    )
}

pub(crate) fn write_frame(output: &mut impl Write, frame: AuthWireResponse) -> Result<(), ()> {
    let encoded = serde_json::to_string(&AuthWireFrame::new(frame)).map_err(|_error| ())?;
    output.write_all(encoded.as_bytes()).map_err(|_error| ())?;
    output.write_all(b"\n").map_err(|_error| ())?;
    output.flush().map_err(|_error| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_rejects_an_invalid_abi_without_writing_a_response() {
        let input = io::Cursor::new(
            b"{\"abi\":\"wrong\",\"frame\":{\"type\":\"api_key_login\",\"request_id\":\"r\",\"provider\":\"p\",\"profile\":\"q\",\"key\":\"secret\"}}\n",
        );
        let mut output = Vec::new();
        assert_eq!(run(input, &mut output), Err(()));
        assert!(output.is_empty());
    }
}
