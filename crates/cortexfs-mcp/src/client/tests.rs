use super::*;
use std::{collections::BTreeMap, fs};

fn server(mode: &str) -> Server {
    Server {
        command: "/usr/bin/python3".to_owned(),
        args: vec![
            "-u".to_owned(),
            "-c".to_owned(),
            include_str!("mock.py").to_owned(),
        ],
        env: BTreeMap::from([
            ("CTXMCP_MOCK".to_owned(), mode.to_owned()),
            (
                "PKG_VERSION".to_owned(),
                env!("CARGO_PKG_VERSION").to_owned(),
            ),
        ]),
    }
}

fn server_with_pid(mode: &str, path: &std::path::Path) -> Server {
    let mut value = server(mode);
    value
        .env
        .insert("PID_FILE".to_owned(), path.to_string_lossy().into_owned());
    value
}

fn server_with_descendant(parent: &std::path::Path, descendant: &std::path::Path) -> Server {
    let mut value = server("descendant");
    value
        .env
        .insert("PID_FILE".to_owned(), parent.to_string_lossy().into_owned());
    value.env.insert(
        "DESC_PID_FILE".to_owned(),
        descendant.to_string_lossy().into_owned(),
    );
    value
}

fn wait_pid(path: &std::path::Path, timeout: Duration) -> io::Result<nix::unistd::Pid> {
    let deadline = Instant::now() + timeout;
    loop {
        match fs::read_to_string(path) {
            Ok(value) => {
                return value
                    .parse::<i32>()
                    .map(nix::unistd::Pid::from_raw)
                    .map_err(io::Error::other);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
}

fn assert_reaped(path: &std::path::Path) -> io::Result<()> {
    let pid = fs::read_to_string(path)?
        .parse::<i32>()
        .map(nix::unistd::Pid::from_raw)
        .map_err(io::Error::other)?;
    for result in [nix::sys::signal::kill(pid, None), nix::sys::signal::killpg(pid, None)] {
        assert_eq!(result, Err(nix::errno::Errno::ESRCH));
    }
    Ok(())
}

#[test]
fn modern_discovery_and_request_metadata_work_without_initialize() -> io::Result<()> {
    let mut client = Client::start(&server("modern"))?;
    assert_eq!(
        client
            .tools()?
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        ["echo", "sum"]
    );
    let mut client = Client::start(&server("modern"))?;
    assert_eq!(
        client.call("echo", &json!({"text":"hi"}))?.get("isError"),
        Some(&Value::Bool(false))
    );
    Ok(())
}

#[test]
fn invalid_negotiation_and_server_requests_are_rejected() {
    for mode in [
        "modernunsupported",
        "modernmissingcap",
        "modernbadresult",
        "unsupportedrequest",
        "malformedping",
        "unknownversion",
        "draftversion",
        "badversion",
        "missingtools",
        "badtools",
    ] {
        assert!(
            Client::start(&server(mode)).is_err(),
            "mode {mode} accepted"
        );
    }
}

#[test]
fn legacy_probe_errors_and_timeouts_fall_back_on_the_same_process() -> io::Result<()> {
    for mode in ["ok", "legacyprobelate"] {
        drop(Client::start(&server(mode))?.tools()?);
    }
    let mut client = Client::start(&server("ok"))?;
    assert_eq!(
        client.call("echo", &json!({"text":"hi"}))?.get("isError"),
        Some(&Value::Bool(false))
    );
    Ok(())
}

#[test]
fn server_ping_requests_with_string_and_number_ids_are_answered() -> io::Result<()> {
    for mode in ["pingstr", "pingnum"] {
        drop(Client::start(&server(mode))?.tools()?);
    }
    Ok(())
}

#[test]
fn stable_legacy_protocol_versions_are_accepted() -> io::Result<()> {
    for mode in ["ok", "stable20250618", "stable20250326", "legacy20241105"] {
        drop(Client::start(&server(mode))?.tools()?);
    }
    Ok(())
}

#[test]
fn required_tool_shape_and_object_schema_type_are_enforced() -> io::Result<()> {
    for mode in [
        "missingname",
        "missinginputschema",
        "missingschematype",
        "wrongschematype",
    ] {
        let mut client = Client::start(&server(mode))?;
        assert!(client.tools().is_err(), "mode {mode} accepted");
    }
    Ok(())
}

#[test]
fn task_support_defaults_and_stable_values_are_decoded() -> io::Result<()> {
    for (mode, expected) in [
        ("ok", TaskSupport::Forbidden),
        ("optionaltask", TaskSupport::Optional),
        ("requiredtask", TaskSupport::Required),
    ] {
        let mut client = Client::start(&server(mode))?;
        assert_eq!(
            client
                .tools()?
                .first()
                .map(|tool| tool.execution.task_support),
            Some(expected)
        );
    }
    let mut bad = Client::start(&server("badtask"))?;
    assert!(bad.tools().is_err());
    Ok(())
}

#[test]
fn timeout_and_oversize_are_rejected() {
    for (mode, kind) in [
        ("timeout", io::ErrorKind::TimedOut),
        ("oversize", io::ErrorKind::InvalidData),
    ] {
        assert_eq!(
            Client::start(&server(mode)).err().map(|error| error.kind()),
            Some(kind)
        );
    }
}

#[test]
fn stderr_has_an_independent_hard_limit() -> io::Result<()> {
    let root = tempfile::tempdir()?;
    let limit_pid = root.path().join("limit-pid");
    let mut limit = Client::start(&server_with_pid("stderrlimit", &limit_pid))?;
    drop(limit.tools()?);
    assert_reaped(&limit_pid)?;
    let overflow_pid = root.path().join("overflow-pid");
    assert_eq!(
        Client::start(&server_with_pid("stderroverflow", &overflow_pid))
            .and_then(|mut client| client.tools())
            .err()
            .map(|error| error.kind()),
        Some(io::ErrorKind::InvalidData)
    );
    assert_reaped(&overflow_pid)
}

#[test]
fn rpc_errors_and_duplicate_names_are_rejected() -> io::Result<()> {
    let mut error = Client::start(&server("error"))?;
    assert!(error.tools().is_err());
    let mut duplicate = Client::start(&server("duplicate"))?;
    assert!(duplicate.tools().is_err());
    Ok(())
}

#[test]
fn cleanup_is_bounded() -> io::Result<()> {
    let client = Client::start(&server("ok"))?;
    let started = Instant::now();
    drop(client);
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[test]
fn descendant_pipe_holder_is_killed_without_blocking_drop() -> io::Result<()> {
    let root = tempfile::tempdir()?;
    let parent = root.path().join("parent-pid");
    let descendant = root.path().join("descendant-pid");
    let client = Client::start(&server_with_descendant(&parent, &descendant))?;
    let descendant_pid = wait_pid(&descendant, Duration::from_secs(1))?;
    let started = Instant::now();
    drop(client);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_reaped(&parent)?;
    assert_eq!(
        nix::sys::signal::kill(descendant_pid, None),
        Err(nix::errno::Errno::ESRCH)
    );
    Ok(())
}

#[test]
fn cursor_notifications_and_call_errors_are_strict() -> io::Result<()> {
    for mode in ["invalidnotification", "slowframes"] {
        assert!(
            Client::start(&server(mode)).is_err(),
            "mode {mode} accepted"
        );
    }
    let mut cursor = Client::start(&server("badcursor"))?;
    assert!(cursor.tools().is_err());
    let mut call = Client::start(&server("callerror"))?;
    assert!(call.call("echo", &json!({})).is_err());
    let mut missing = Client::start(&server("modernmissingresulttype"))?;
    assert!(missing.tools().is_err());
    let mut input = Client::start(&server("moderninputrequired"))?;
    assert!(input.call("echo", &json!({})).is_err());
    Ok(())
}

#[test]
fn every_error_path_reaps_the_process_group() -> io::Result<()> {
    for mode in ["badversion", "timeout", "oversize", "modernunsupported"] {
        let root = tempfile::tempdir()?;
        let pid = root.path().join("pid");
        assert!(Client::start(&server_with_pid(mode, &pid)).is_err());
        assert_reaped(&pid)?;
    }
    let root = tempfile::tempdir()?;
    let pid = root.path().join("pid");
    let mut client = Client::start(&server_with_pid("callerror", &pid))?;
    assert!(client.call("echo", &json!({})).is_err());
    assert_reaped(&pid)
}

#[test]
fn successful_call_reaps_before_return() -> io::Result<()> {
    let root = tempfile::tempdir()?;
    let pid = root.path().join("pid");
    let mut client = Client::start(&server_with_pid("ok", &pid))?;
    client.call("echo", &json!({}))?;
    assert_reaped(&pid)
}
