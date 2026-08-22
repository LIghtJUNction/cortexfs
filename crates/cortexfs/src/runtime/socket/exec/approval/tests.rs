use super::{AgentToolCall, request_tool_approval};
use cortexfs_runtime_client::interaction::{
    InteractionFrame, InteractionRequest, InteractionResult,
};
use std::ffi::OsString;
use std::io::{self, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::time::Duration;

fn call() -> AgentToolCall {
    AgentToolCall {
        id: "call-1".to_owned(),
        name: "example.echo".to_owned(),
        args: vec![OsString::from("one")],
    }
}

fn approval(response: Option<&[u8]>) -> Result<bool, Box<dyn std::error::Error>> {
    let (mut client, mut server) = UnixStream::pair()?;
    if let Some(response) = response {
        client.write_all(response)?;
        client.shutdown(Shutdown::Write)?;
    } else {
        server.set_read_timeout(Some(Duration::from_millis(10)))?;
    }
    request_tool_approval(&mut server, "request-1", "run-1", &call())
        .map(|approval| approval.allowed)
        .map_err(|error| io::Error::other(format!("{error:?}")).into())
}

#[test]
fn response_accepts_only_emitted_run_and_exact_call() -> Result<(), Box<dyn std::error::Error>> {
    for response in [
        "",
        "not-json\n",
        "{\"op\":\"approve\",\"run\":\"client-1\",\"id\":\"call-1\",\"decision\":\"allow_once\"}\n",
        "{\"op\":\"approve\",\"run\":\"wrong\",\"id\":\"call-1\",\"decision\":\"allow_once\"}\n",
        "{\"op\":\"approve\",\"run\":\"run-1\",\"id\":\"wrong\",\"decision\":\"allow_once\"}\n",
        "{\"op\":\"approve\",\"run\":\"run-1\",\"id\":\"call-1\",\"decision\":\"deny\"}\n",
    ] {
        assert!(!approval(Some(response.as_bytes()))?, "{response:?}");
    }
    assert!(!approval(None)?);
    assert!(approval(Some(
        b"{\"op\":\"approve\",\"run\":\"run-1\",\"id\":\"call-1\",\"decision\":\"allow_once\"}\n"
    ))?);

    for (request_id, accepted) in [("other-request", false), ("request-1", true)] {
        let response = InteractionFrame::request(InteractionRequest::CommandResult {
            request_id: request_id.to_owned(),
            session: "default".to_owned(),
            command_id: "call-1".to_owned(),
            result: InteractionResult::Accepted,
        })
        .encode()
        .map_err(io::Error::other)?;
        assert_eq!(approval(Some(&response))?, accepted);
    }
    Ok(())
}

#[test]
fn response_denies_replayed_allow_once_for_previous_call() -> Result<(), Box<dyn std::error::Error>>
{
    let (mut client, mut server) = UnixStream::pair()?;
    client.write_all(
        b"{\"op\":\"approve\",\"run\":\"run-1\",\"id\":\"call-1\",\"decision\":\"allow_once\"}\n\
          {\"op\":\"approve\",\"run\":\"run-1\",\"id\":\"call-1\",\"decision\":\"allow_once\"}\n",
    )?;
    client.shutdown(Shutdown::Write)?;
    let first = request_tool_approval(&mut server, "request-1", "run-1", &call())
        .map_err(|error| io::Error::other(format!("{error:?}")))?;
    assert!(first.allowed);

    let mut second = call();
    second.id = "call-2".to_owned();
    let replayed = request_tool_approval(&mut server, "request-1", "run-1", &second)
        .map_err(|error| io::Error::other(format!("{error:?}")))?;
    assert!(!replayed.allowed);
    Ok(())
}
