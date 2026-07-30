use std::io;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::process::CurlProcess;
use super::transfer::{self, CurlStop};
use super::{ProviderTarget, Request, invalid};

pub(super) fn run_curl(
    local: UnixStream,
    target: &ProviderTarget,
    request: &Request,
    shutdown: &Arc<AtomicBool>,
) -> io::Result<()> {
    let config = curl_config(target, request)?;
    let monitor = local.try_clone()?;
    let mut process = CurlProcess::spawn()?;
    #[cfg(test)]
    super::tests::record_monitor(&local, &monitor, &mut process)?;
    let closed = Arc::new(AtomicBool::new(false));
    let run = transfer::run(&mut process, local, &config, &monitor, shutdown, &closed);
    let finished = process.finish(!matches!(run, Ok(CurlStop::Exited)));
    let stop = run?;
    let (status, copied) = finished?;
    copied?;
    if status.success()
        || stop == CurlStop::Cancelled
        || shutdown.load(Ordering::Acquire)
        || closed.load(Ordering::Acquire)
    {
        Ok(())
    } else {
        Err(io::Error::other("provider curl failed"))
    }
}

pub(super) fn curl_config(target: &ProviderTarget, request: &Request) -> io::Result<String> {
    let url = format!("{}/{}", target.base_url, request.endpoint);
    let mut config = String::from(
        "request = \"POST\"\nhttp1.1\ninclude\nraw\nno-buffer\nno-location\nconnect-timeout = 5\nmax-time = 300\nsilent\nshow-error\n",
    );
    config.push_str("url = ");
    config.push_str(&curl_quote(url.as_bytes())?);
    config.push('\n');
    for header in &request.headers {
        config.push_str("header = ");
        config.push_str(&curl_quote(
            format!("{}: {}", header.0, header.1).as_bytes(),
        )?);
        config.push('\n');
    }
    config.push_str("data-binary = ");
    config.push_str(&curl_quote(&request.body)?);
    config.push('\n');
    Ok(config)
}

fn curl_quote(value: &[u8]) -> io::Result<String> {
    let value =
        std::str::from_utf8(value).map_err(|_error| invalid("provider HTTP value is not UTF-8"))?;
    let mut quoted = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' | '"' => {
                quoted.extend(['\\', character]);
            }
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character if !character.is_control() => quoted.push(character),
            _ => return Err(invalid("provider HTTP value is not curl-config safe")),
        }
    }
    quoted.push('"');
    Ok(quoted)
}
