use std::fs;
use std::io::{self, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use super::*;

fn fixture() -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let control = root.path().join("control");
    fs::create_dir_all(&control)?;
    fs::set_permissions(&control, fs::Permissions::from_mode(0o711))?;
    Ok((root, control))
}

fn write_model(root: &Path, model: &str, base_url: &str) -> io::Result<()> {
    let (provider, name) = model
        .split_once('/')
        .ok_or_else(|| io::Error::other("bad model"))?;
    let control = root.join("model").join(provider).join(format!("{name}.d"));
    fs::create_dir_all(&control)?;
    fs::write(control.join("default"), format!("base_url={base_url}\n"))
}

#[test]
fn plan_keeps_validated_fixed_base_without_dns() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    write_model(
        root.path(),
        "fixture/chat",
        "https://invalid.example:8443/v1/",
    )?;
    let targets = plan_targets(root.path(), "fixture/chat")?;
    let target = targets.first().ok_or("missing target")?;
    assert_eq!(target.base_url, "https://invalid.example:8443/v1");
    assert_eq!(target.authority, "https://invalid.example:8443");
    assert_eq!(target.base_path, "/v1");
    Ok(())
}

#[test]
fn plan_normalizes_root_and_custom_bases_to_effective_v1_paths()
-> Result<(), Box<dyn std::error::Error>> {
    for (base, expected) in [
        ("", "/v1"),
        ("/v1", "/v1"),
        ("/custom", "/custom/v1"),
        ("/custom/v1", "/custom/v1"),
    ] {
        let root = tempfile::tempdir()?;
        write_model(
            root.path(),
            "fixture/chat",
            &format!("https://example.test{base}"),
        )?;
        let targets = plan_targets(root.path(), "fixture/chat")?;
        let target = targets.first().ok_or("missing target")?;
        assert_eq!(target.base_url, format!("https://example.test{expected}"));
        assert_eq!(target.base_path, expected);
    }
    Ok(())
}

#[test]
fn plan_deduplicates_equivalent_effective_provider_bases() -> Result<(), Box<dyn std::error::Error>>
{
    for (primary, fallback) in [("", "/v1"), ("/custom", "/custom/v1")] {
        let root = tempfile::tempdir()?;
        write_model(
            root.path(),
            "fixture/primary",
            &format!("http://127.0.0.1:8001{primary}"),
        )?;
        write_model(
            root.path(),
            "fixture/fallback",
            &format!("http://127.0.0.1:8001{fallback}"),
        )?;
        fs::write(
            root.path().join("model/fixture/primary.d/fallback"),
            "fixture/fallback\n",
        )?;
        assert_eq!(plan_targets(root.path(), "fixture/primary")?.len(), 1);
    }
    Ok(())
}

#[test]
fn plan_rejects_provider_base_conflicts_before_resources() -> Result<(), Box<dyn std::error::Error>>
{
    let (root, control) = fixture()?;
    write_model(root.path(), "fixture/primary", "http://127.0.0.1:8001/v1")?;
    write_model(root.path(), "fixture/fallback", "http://127.0.0.1:8001/v2")?;
    fs::write(
        root.path().join("model/fixture/primary.d/fallback"),
        "fixture/fallback\n",
    )?;
    let result = ProviderEgress::create(
        &control,
        root.path(),
        "fixture/primary",
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
        "run1",
    );
    assert!(matches!(
        result,
        Err(ProviderEgressError::AuthorityConflict)
    ));
    assert_eq!(fs::read_dir(control)?.count(), 0);
    Ok(())
}

#[test]
fn plan_rejects_credentials_queries_and_fragments() -> Result<(), Box<dyn std::error::Error>> {
    for url in [
        "https://user@example.test/v1",
        "https://example.test/v1?q=1",
    ] {
        let root = tempfile::tempdir()?;
        write_model(root.path(), "fixture/chat", url)?;
        assert!(matches!(
            plan_targets(root.path(), "fixture/chat"),
            Err(ProviderEgressError::InvalidBaseUrl)
        ));
    }
    Ok(())
}

#[test]
fn plan_resolves_alias_fallback_and_deduplicates_provider() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let base = "http://127.0.0.1:8001/v1";
    write_model(root.path(), "fixture/primary", base)?;
    write_model(root.path(), "fixture/fallback", base)?;
    fs::write(
        root.path().join("model/fixture/primary.d/fallback"),
        "fixture/fallback\n",
    )?;
    std::os::unix::fs::symlink("/ctx/model/fixture/primary", root.path().join("model/main"))?;
    let targets = plan_targets(root.path(), "main")?;
    assert_eq!(targets.len(), 1);
    assert_eq!(targets.first().ok_or("missing target")?.provider, "fixture");
    Ok(())
}

#[test]
fn plan_skips_orphan_fallback_but_requires_primary_control()
-> Result<(), Box<dyn std::error::Error>> {
    let (root, control) = fixture()?;
    write_model(root.path(), "fixture/primary", "http://127.0.0.1:8001/v1")?;
    fs::write(
        root.path().join("model/fixture/primary.d/fallback"),
        "orphan/missing\n",
    )?;
    assert_eq!(plan_targets(root.path(), "fixture/primary")?.len(), 1);
    let result = ProviderEgress::create(
        &control,
        root.path(),
        "orphan/missing",
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
        "run1",
    );
    assert!(matches!(result, Err(ProviderEgressError::MissingControl)));
    assert_eq!(fs::read_dir(control)?.count(), 0);
    Ok(())
}

#[test]
fn provider_model_detection_resolves_provider_and_debug_aliases()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    write_model(root.path(), "fixture/chat", "http://127.0.0.1:8001/v1")?;
    fs::create_dir_all(root.path().join("model/debug/echo.d"))?;
    std::os::unix::fs::symlink("/ctx/model/fixture/chat", root.path().join("model/main"))?;
    assert!(is_provider_model(root.path(), "main")?);
    fs::remove_file(root.path().join("model/main"))?;
    std::os::unix::fs::symlink("/ctx/model/debug/echo", root.path().join("model/main"))?;
    assert!(!is_provider_model(root.path(), "main")?);
    Ok(())
}

#[test]
fn create_preserves_receipts_owner_and_replacement() -> Result<(), Box<dyn std::error::Error>> {
    let (root, control) = fixture()?;
    write_model(root.path(), "fixture/chat", "http://127.0.0.1:9/v1")?;
    let owner = (
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
    );
    let egress = ProviderEgress::create(
        &control,
        root.path(),
        "fixture/chat",
        owner.0,
        owner.1,
        "run1",
    )?;
    let directory = egress.host_dir().to_owned();
    let socket = egress.socket("fixture").ok_or("missing socket")?.to_owned();
    let metadata = fs::symlink_metadata(&socket)?;
    assert_eq!((metadata.uid(), metadata.gid()), owner);
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    fs::remove_file(&socket)?;
    let replacement = UnixListener::bind(&socket)?;
    drop(egress);
    assert!(socket.exists());
    assert!(directory.exists());
    drop(replacement);
    fs::remove_file(socket)?;
    fs::remove_dir(directory)?;
    Ok(())
}

#[test]
fn run_directory_keeps_runtime_owner_when_agent_differs_if_root()
-> Result<(), Box<dyn std::error::Error>> {
    let (root, control) = fixture()?;
    write_model(root.path(), "fixture/chat", "http://127.0.0.1:9/v1")?;
    let runtime = (
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
    );
    let agent = if runtime.0 == 0 {
        (65_534, 65_534)
    } else {
        runtime
    };
    let egress = ProviderEgress::create(
        &control,
        root.path(),
        "fixture/chat",
        agent.0,
        agent.1,
        "run1",
    )?;
    let directory = fs::symlink_metadata(egress.host_dir())?;
    let socket = fs::symlink_metadata(egress.socket("fixture").ok_or("missing socket")?)?;
    assert_eq!((directory.uid(), directory.gid()), runtime);
    assert_eq!(directory.mode() & 0o7777, 0o711);
    assert_eq!((socket.uid(), socket.gid()), agent);
    Ok(())
}

#[test]
fn root_agent_uid_can_use_http_adapter() -> Result<(), Box<dyn std::error::Error>> {
    if nix::unistd::geteuid().as_raw() != 0 {
        return Ok(());
    }
    let (root, control) = fixture()?;
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o711))?;
    let tcp = TcpListener::bind("127.0.0.1:0")?;
    write_model(
        root.path(),
        "fixture/chat",
        &format!("http://{}/v1", tcp.local_addr()?),
    )?;
    let upstream = thread::spawn(move || -> io::Result<()> {
        let (mut stream, _) = tcp.accept()?;
        let mut input = [0; 4096];
        let _read = stream.read(&mut input)?;
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
    });
    let egress = ProviderEgress::create(
        &control,
        root.path(),
        "fixture/chat",
        65_534,
        65_534,
        "run1",
    )?;
    let status = Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("runtime::egress::tests::cross_uid_http_helper")
        .arg("--ignored")
        .arg("--nocapture")
        .env(
            "CORTEXFS_EGRESS_CROSS_UID_SOCKET",
            egress.socket("fixture").ok_or("missing socket")?,
        )
        .status()?;
    assert!(status.success());
    upstream.join().map_err(|_panic| "upstream panicked")??;
    Ok(())
}

#[test]
#[ignore = "re-exec helper for root cross-uid egress test"]
fn cross_uid_http_helper() -> Result<(), Box<dyn std::error::Error>> {
    let Some(socket) = std::env::var_os("CORTEXFS_EGRESS_CROSS_UID_SOCKET") else {
        return Ok(());
    };
    nix::unistd::setgroups(&[])?;
    nix::unistd::setgid(nix::unistd::Gid::from_raw(65_534))?;
    nix::unistd::setuid(nix::unistd::Uid::from_raw(65_534))?;
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(b"POST /v1/responses HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}")?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
    assert!(response.ends_with(b"{}"));
    Ok(())
}

#[test]
fn listener_rejects_wrong_peer_without_reaching_upstream() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let tcp = TcpListener::bind("127.0.0.1:0")?;
    tcp.set_nonblocking(true)?;
    let target = ProviderTarget {
        provider: "fixture".to_owned(),
        base_url: format!("http://{}", tcp.local_addr()?),
        authority: format!("http://{}", tcp.local_addr()?),
        base_path: String::new(),
    };
    let socket = root.path().join("egress.sock");
    let listener = UnixListener::bind(&socket)?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&shutdown);
    let denied_uid = nix::unistd::getuid().as_raw().wrapping_add(1);
    let server = thread::spawn(move || serve(listener, target, denied_uid, stop));
    let mut client = UnixStream::connect(socket)?;
    client.write_all(b"POST /responses HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}")?;
    thread::sleep(Duration::from_millis(150));
    shutdown.store(true, Ordering::Release);
    server.join().map_err(|_panic| "server panicked")?;
    assert!(matches!(tcp.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock));
    Ok(())
}

#[test]
fn connection_headers_never_reach_curl_or_upstream() -> Result<(), Box<dyn std::error::Error>> {
    let (root, control) = fixture()?;
    let tcp = TcpListener::bind("127.0.0.1:0")?;
    tcp.set_nonblocking(true)?;
    write_model(
        root.path(),
        "fixture/chat",
        &format!("http://{}/v1", tcp.local_addr()?),
    )?;
    let egress = ProviderEgress::create(
        &control,
        root.path(),
        "fixture/chat",
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
        "run1",
    )?;
    for connection in [
        "Connection: authorization\r\nAuthorization: Bearer secret\r\n",
        "Connection: keep-alive, authorization\r\nAuthorization: Bearer secret\r\n",
        "cOnNeCtIoN: keep-alive\r\n",
    ] {
        let mut client = UnixStream::connect(egress.socket("fixture").ok_or("missing socket")?)?;
        write!(
            client,
            "POST /v1/responses HTTP/1.1\r\n{connection}Content-Length: 2\r\n\r\n{{}}"
        )?;
        let mut response = Vec::new();
        client.read_to_end(&mut response)?;
        assert!(response.starts_with(b"HTTP/1.1 400 Bad Request\r\n"));
    }
    thread::sleep(Duration::from_millis(100));
    assert!(matches!(
        tcp.accept(),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock
    ));
    Ok(())
}

#[test]
fn drop_stops_continuous_curl_within_one_second() -> Result<(), Box<dyn std::error::Error>> {
    let (root, control) = fixture()?;
    let tcp = TcpListener::bind("127.0.0.1:0")?;
    write_model(
        root.path(),
        "fixture/chat",
        &format!("http://{}/v1", tcp.local_addr()?),
    )?;
    let upstream = thread::spawn(move || -> io::Result<()> {
        let (mut stream, _) = tcp.accept()?;
        let mut input = [0; 4096];
        let _read = stream.read(&mut input)?;
        stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: text/event-stream\r\n\r\n")?;
        loop {
            stream.write_all(b"6\r\ndata\n\n\r\n")?;
            thread::sleep(Duration::from_millis(10));
        }
    });
    let egress = ProviderEgress::create(
        &control,
        root.path(),
        "fixture/chat",
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
        "run1",
    )?;
    let mut client = UnixStream::connect(egress.socket("fixture").ok_or("missing socket")?)?;
    client.write_all(b"POST /v1/chat/completions HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}")?;
    let mut response = [0; 32];
    client.read_exact(&mut response)?;
    let started = Instant::now();
    drop(egress);
    assert!(started.elapsed() < Duration::from_secs(1));
    drop(client);
    assert!(
        upstream
            .join()
            .map_err(|_panic| "upstream panicked")?
            .is_err()
    );
    Ok(())
}
